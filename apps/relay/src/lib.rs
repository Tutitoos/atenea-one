// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
pub mod alloc;
pub mod config;
pub mod limits;
mod state;
mod tcp;

use alloc::Binding;
use config::Config;
use rand::RngCore;
use state::{Output, State};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, UdpSocket, UnixListener},
    sync::{mpsc, watch},
    task::JoinSet,
};

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
pub(crate) struct Shared {
    state: State,
    clients: HashMap<u64, (IpAddr, mpsc::Sender<Vec<u8>>)>,
}
pub(crate) struct Runtime {
    shared: Mutex<Shared>,
    udp: UdpSocket,
}
impl Runtime {
    pub(crate) async fn dispatch(&self, outputs: Vec<Output>) {
        for (binding, body) in outputs {
            let forwarded = matches!(
                atenea_relay_core::parse(&body),
                Ok(atenea_relay_core::Message::Data { .. })
            );
            let bytes = body.len().saturating_sub(atenea_relay_core::DATA_OVERHEAD) as u64;
            let result = match binding {
                Binding::Udp(a) => self
                    .udp
                    .send_to(&body, a)
                    .await
                    .map(|_| ())
                    .map_err(|_| "udp_write"),
                Binding::Tcp(id, _) => {
                    let s = self.shared.lock().unwrap();
                    match s.clients.get(&id) {
                        Some((_, tx)) => tx.try_send(body).map_err(|_| "tcp_queue"),
                        None => Err("tcp_closed"),
                    }
                }
            };
            let mut s = self.shared.lock().unwrap();
            if let Err(reason) = result {
                s.state.drop_packet(reason);
            } else if forwarded {
                let c = &mut s.state.counters;
                c.forwarded_packets += 1;
                c.forwarded_bytes += bytes;
                match binding {
                    Binding::Udp(_) => {
                        c.udp_packets += 1;
                        c.udp_bytes += bytes;
                    }
                    Binding::Tcp(..) => {
                        c.tcp_packets += 1;
                        c.tcp_bytes += bytes;
                    }
                }
            }
        }
    }
    /// Devuelve `true` si la respuesta incluye `ALLOC_OK` para `binding` (sin recorrer la tabla).
    pub(crate) async fn message(&self, binding: Binding, body: &[u8]) -> bool {
        let outputs = self
            .shared
            .lock()
            .unwrap()
            .state
            .process(binding, body, now());
        let allocated = outputs.iter().any(|(to, out)| {
            *to == binding && out.get(2) == Some(&atenea_relay_core::ty::ALLOC_OK)
        });
        self.dispatch(outputs).await;
        allocated
    }

    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let s = self.shared.lock().unwrap();
        let udp = s
            .state
            .table
            .entries
            .values()
            .filter(|a| matches!(a.binding, Binding::Udp(_)))
            .count();
        serde_json::json!({"relay_id":s.state.config.relay_id,"allocations":s.state.table.entries.len(),"bound_udp":udp,"bound_tcp":s.state.table.entries.len()-udp,"tcp_connections":s.clients.len(),"traffic":s.state.counters})
    }
    fn metrics_text(&self) -> String {
        use std::fmt::Write;
        let snapshot = self.snapshot();
        let mut out = String::new();
        for (name, key) in [
            ("atenea_relay_allocations", "allocations"),
            ("atenea_relay_udp_sessions", "bound_udp"),
            ("atenea_relay_tls_sessions", "bound_tcp"),
            ("atenea_relay_tls_connections", "tcp_connections"),
        ] {
            let _ = writeln!(out, "# TYPE {name} gauge\n{name} {}", snapshot[key]);
        }
        let s = self.shared.lock().unwrap();
        let c = &s.state.counters;
        for (transport, direction, packets, bytes) in [
            ("udp", "egress", c.udp_packets, c.udp_bytes),
            ("tls", "egress", c.tcp_packets, c.tcp_bytes),
            (
                "udp",
                "ingress",
                c.udp_received_packets,
                c.udp_received_bytes,
            ),
            (
                "tls",
                "ingress",
                c.tcp_received_packets,
                c.tcp_received_bytes,
            ),
        ] {
            let _ = writeln!(
                out,
                "atenea_relay_packets_total{{transport=\"{transport}\",direction=\"{direction}\"}} {packets}\natenea_relay_bytes_total{{transport=\"{transport}\",direction=\"{direction}\"}} {bytes}"
            );
        }
        for reason in [
            "malformed",
            "cookie",
            "cookie_rate",
            "unexpected",
            "unbound",
            "size",
            "allocation_rate",
            "no_peer",
            "udp_write",
            "tcp_queue",
            "tcp_closed",
            "tcp_capacity",
            "tcp_protocol",
        ] {
            let _ = writeln!(
                out,
                "atenea_relay_drops_total{{reason=\"{reason}\"}} {}",
                c.drops.get(reason).unwrap_or(&0)
            );
        }
        for code in 1..=11 {
            let _ = writeln!(
                out,
                "atenea_relay_rejects_total{{code=\"{code}\"}} {}",
                c.rejects.get(&code).unwrap_or(&0)
            );
        }
        let _ = writeln!(out, "atenea_relay_allocations_total {}", c.allocations);
        out
    }
}

pub struct Relay {
    runtime: Arc<Runtime>,
    tcp: TcpListener,
    status: UnixListener,
    metrics: tokio::task::JoinHandle<()>,
    tls: tokio_rustls::TlsAcceptor,
    status_path: String,
}
impl Relay {
    pub async fn bind(config: Config) -> io::Result<Self> {
        let keys = config.validate().map_err(io::Error::other)?;
        let tls = tcp::tls_config(&config)?;
        let udp = UdpSocket::bind(config.udp_listen).await?;
        let tcp = TcpListener::bind(config.tcp_listen).await?;
        let metrics_listener = tokio::net::TcpListener::bind(&config.metrics_listen).await?;
        // No se elimina una ruta preexistente: podría pertenecer a otro proceso.
        let status = UnixListener::bind(&config.status_socket)?;
        let status_path = config.status_socket.clone();
        let mut secret = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        let runtime = Arc::new(Runtime {
            udp,
            shared: Mutex::new(Shared {
                state: State::new(config, keys, secret),
                clients: HashMap::new(),
            }),
        });
        let render = runtime.clone();
        let metrics = tokio::spawn(async move {
            let _ = axum_metrics(metrics_listener, render).await;
        });
        Ok(Self {
            runtime,
            tcp,
            status,
            metrics,
            tls,
            status_path,
        })
    }
    pub fn udp_addr(&self) -> io::Result<SocketAddr> {
        self.runtime.udp.local_addr()
    }
    pub fn tcp_addr(&self) -> io::Result<SocketAddr> {
        self.tcp.local_addr()
    }
    pub fn status(&self) -> serde_json::Value {
        self.runtime.snapshot()
    }
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
        let mut tasks = JoinSet::new();
        let mut buf = [0; 2049];
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
        let mut next_id = 0u64;
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                result = self.runtime.udp.recv_from(&mut buf) => { let (n,a) = result?; self.runtime.message(Binding::Udp(a),&buf[..n]).await; },
                result = self.tcp.accept() => {
                    let (stream,addr) = result?;
                    let mut s = self.runtime.shared.lock().unwrap(); let c = &s.state.config;
                    let group = limits::ip_group(addr.ip());
                    if s.clients.len() >= c.tcp_total || s.clients.values().filter(|(ip,_)| *ip == group).count() >= c.tcp_per_ip {
                        s.state.drop_packet("tcp_capacity"); continue;
                    }
                    next_id = next_id.checked_add(1).ok_or_else(|| io::Error::other("identificadores TCP agotados"))?;
                    let id = next_id; let (tx,rx) = mpsc::channel(c.tcp_queue); s.clients.insert(id,(group,tx)); drop(s);
                    let runtime = self.runtime.clone(); let tls = self.tls.clone(); let stop = shutdown.clone();
                    tasks.spawn(async move { tcp::connection(runtime,stream,addr,id,rx,tls,stop).await; });
                },
                result = self.status.accept() => {
                    let (mut stream,_) = result?; let mut body = serde_json::to_vec(&self.runtime.snapshot())?; body.push(b'\n');
                    // Una lectura lenta no ocupa una tarea ni bloquea el relay indefinidamente.
                    let _ = tokio::time::timeout(std::time::Duration::from_millis(100),stream.write_all(&body)).await;
                },
                _ = tick.tick() => { let out = self.runtime.shared.lock().unwrap().state.sweep(now()); self.runtime.dispatch(out).await; },
                Some(result) = tasks.join_next(), if !tasks.is_empty() => { if result.is_err() { tracing::warn!("tarea TCP terminada"); } },
            }
        }
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        Ok(())
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.metrics.abort();
        let _ = std::fs::remove_file(&self.status_path);
    }
}

async fn axum_metrics(listener: TcpListener, runtime: Arc<Runtime>) -> io::Result<()> {
    // El listener no se comparte con TLS ni con el socket público UDP.
    atenea_observability::serve_listener(listener, Arc::new(move || runtime.metrics_text())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn full_tcp_queue_drops_new_packet() {
        let ip = "127.0.0.1".parse().unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        let runtime = Runtime {
            udp: UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            shared: Mutex::new(Shared {
                state: State::new(
                    Config {
                        relay_id: "11".repeat(16),
                        ..Config::default()
                    },
                    vec![],
                    [0; 32],
                ),
                clients: HashMap::from([(1, (ip, tx))]),
            }),
        };
        let body = atenea_relay_core::encode_data(1, b"xx");
        runtime
            .dispatch(vec![
                (Binding::Tcp(1, ip), body.clone()),
                (Binding::Tcp(1, ip), body.clone()),
            ])
            .await;
        assert_eq!(rx.recv().await.unwrap(), body);
        assert!(rx.try_recv().is_err());
        assert_eq!(
            runtime.shared.lock().unwrap().state.counters.drops["tcp_queue"],
            1
        );
    }
}
