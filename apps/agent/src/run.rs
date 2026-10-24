//! Bucle de E/S: TUN, UDP, temporizador, recarga de configuración firmada e IPC.
//! Toda la lógica de protocolo vive en `engine`; la verificación, en `atenea-identity`.

use crate::config::{Config, load_identity_key, load_noise_key};
use crate::engine::{Engine, NatStatus, Out};
use crate::net::{self, Stun, from_wire, to_wire};
use crate::state;
use atenea_identity::{AgentState, LocalKeys, MAX_BODY_LEN, verify_config};
use atenea_nat_traversal::looks_like_stun;
use atenea_policies::prefix_contains;
use atenea_tunnel::Tun;
use clap::{Parser, Subcommand};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UdpSocket, UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;

#[derive(Parser)]
#[command(name = "atenea-agent", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ejecuta el agente en primer plano (requiere CAP_NET_ADMIN).
    Run {
        #[arg(short, long)]
        config: PathBuf,
    },
}

pub fn main() -> ExitCode {
    tracing_subscriber::fmt().with_max_level(log_level()).init();
    let Cmd::Run { config } = Cli::parse().cmd;
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Nivel desde `ATENEA_LOG` (error|warn|info|debug); por defecto info.
fn log_level() -> tracing::Level {
    match std::env::var("ATENEA_LOG").as_deref() {
        Ok("error") => tracing::Level::ERROR,
        Ok("warn") => tracing::Level::WARN,
        Ok("debug") => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Estado de ejecución que cambia con cada configuración aplicada.
struct Runtime {
    cfg: Config,
    local: LocalKeys,
    state: AgentState,
    tun: Tun,
    engine: Engine,
    /// (mtime, longitud) del último fichero de configuración procesado, para no reprocesarlo.
    seen: Option<(SystemTime, u64)>,
    /// MTU y direcciones ya instaladas en la interfaz.
    iface: Option<(usize, Vec<(IpAddr, u8)>)>,
    last_saved: u64,
}

impl Runtime {
    /// Lee, verifica, persiste y aplica la configuración firmada si el fichero cambió.
    /// Un rechazo no revoca la vigente: sigue hasta que caduque su lease.
    fn poll_config(&mut self, unix: u64) {
        let Ok(meta) = std::fs::metadata(&self.cfg.config_file) else {
            return;
        };
        let key = (meta.modified().unwrap_or(UNIX_EPOCH), meta.len());
        if self.seen == Some(key) {
            return;
        }
        self.seen = Some(key);
        if meta.len() as usize > MAX_BODY_LEN + 512 {
            self.reject("too_large");
            return;
        }
        let bytes = match std::fs::read(&self.cfg.config_file) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("configuración ilegible: {e}");
                self.seen = None; // reintentar en el siguiente ciclo
                return;
            }
        };
        let v = match verify_config(&bytes, &self.local, &self.state, unix) {
            Ok(v) => v,
            Err(e) => {
                self.reject(e.code());
                return;
            }
        };
        // Se persiste ANTES de aplicar: un fallo no aplica nada y conserva la configuración anterior.
        if !v.noop || v.state != self.state {
            if let Err(e) = state::save(&self.cfg.state_dir, &v.state) {
                tracing::error!("no se pudo persistir el estado: {e}");
                self.reject("state_persist_failed");
                return;
            }
            self.last_saved = unix;
        }
        let addrs: Vec<(IpAddr, u8)> = v
            .config
            .tunnel_addrs
            .iter()
            .filter_map(|a| {
                v.config
                    .network_prefixes
                    .iter()
                    .find(|p| prefix_contains(p, a))
                    .map(|p| (*a, p.len))
            })
            .collect();
        let wanted = (usize::from(v.config.mtu), addrs);
        if self.iface.as_ref() != Some(&wanted) {
            if let Err(e) = self.tun.configure(wanted.0, &wanted.1) {
                tracing::error!("configurando la interfaz: {e}");
                self.reject("interface_failed");
                return;
            }
            self.iface = Some(wanted);
        }
        self.engine.apply_config(&v.config);
        tracing::info!(
            version = v.config.config_version,
            noop = v.noop,
            "configuración aplicada"
        );
        self.state = v.state;
    }

    fn reject(&mut self, code: &'static str) {
        tracing::warn!(code, "configuración rechazada");
        self.engine.note_config_error(code);
    }

    /// Mantiene la hora máxima observada (anti-rollback de reloj) y la persiste como mucho cada 60 s.
    fn housekeeping(&mut self, unix: u64) {
        self.state.max_time_observed = self.state.max_time_observed.max(unix);
        if unix.saturating_sub(self.last_saved) >= 60 {
            if let Err(e) = state::save(&self.cfg.state_dir, &self.state) {
                tracing::error!("no se pudo persistir el estado: {e}");
            }
            self.last_saved = unix;
        }
    }
}

async fn run(path: PathBuf) -> Result<(), String> {
    let cfg = Config::load(&path)?;
    let noise = load_noise_key(&cfg.noise_key_file)?;
    let identity = load_identity_key(&cfg.identity_key_file)?;
    let local = LocalKeys {
        noise_pub: noise.public,
        identity_pub: identity.verifying_key().to_bytes(),
    };
    let state = match state::load(&cfg.state_dir)? {
        Some(s) if s.network_id == cfg.network_id && s.device_id == cfg.device_id => s,
        Some(_) => return Err("el estado persistido pertenece a otra red o dispositivo".into()),
        None => {
            let s = AgentState::new(cfg.network_id, cfg.device_id, cfg.trust_anchor);
            state::save(&cfg.state_dir, &s).map_err(|e| format!("estado inicial: {e}"))?;
            s
        }
    };
    let tun = Tun::create(&cfg.interface).map_err(|e| format!("TUN {}: {e}", cfg.interface))?;
    let udp = UdpSocket::bind(cfg.listen)
        .await
        .map_err(|e| format!("UDP {}: {e}", cfg.listen))?;
    let ipc = bind_ipc(&cfg.ipc_socket)?;
    let (sync_tx, sync_rx) =
        tokio::sync::watch::channel(cfg.sync.as_ref().map(|s| crate::sync::Status {
            url: s.url.clone(),
            ..Default::default()
        }));
    let sync_task = cfg.sync.clone().map(|sync| {
        let path = cfg.config_file.clone();
        let device = cfg.device_id;
        let identity = identity.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(sync.interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let sync = sync.clone();
                let identity = identity.clone();
                let path = path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    crate::sync::fetch(&sync, &identity, device, &path, unix_now())
                })
                .await
                .unwrap_or(Err("fallo de la tarea de sincronización"));
                sync_tx.send_modify(|s| {
                    if let Some(s) = s {
                        s.record(result, unix_now());
                    }
                });
            }
        })
    });
    let engine = Engine::new(cfg.network_id, noise, Instant::now());
    let dual = cfg.listen.is_ipv6();
    let device = cfg.device_id;

    // Coordinator (fase 4): anuncio de presencia, avisos de pares y descubrimiento de candidatos.
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel::<CoordMsg>(64);
    let (cand_tx, cand_rx) = tokio::sync::watch::channel::<Vec<SocketAddr>>(Vec::new());
    let announce_now = Arc::new(Notify::new());
    let (coord_tx, coord_rx) =
        tokio::sync::watch::channel(cfg.coordinator.as_ref().map(|c| CoordStatus {
            url: c.url.clone(),
            ..Default::default()
        }));
    let mut coord_tasks = Vec::new();
    if let Some(c) = cfg.coordinator.clone() {
        let (c1, id1, path1, rx1, n1, tx1) = (
            c.clone(),
            identity.clone(),
            cfg.config_file.clone(),
            cand_rx.clone(),
            announce_now.clone(),
            coord_tx.clone(),
        );
        coord_tasks.push(tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(20));
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! { _ = every.tick() => {}, _ = n1.notified() => {} }
                let Ok(bytes) = std::fs::read(&path1) else {
                    continue;
                };
                if !bytes.starts_with(b"AOCF") {
                    continue;
                }
                let cands = rx1.borrow().clone();
                let (c, k) = (c1.clone(), id1.clone());
                let res = tokio::task::spawn_blocking(move || {
                    crate::coord::announce(&c, &k, device, &bytes, &cands, unix_now())
                })
                .await
                .unwrap_or(Err("fallo de la tarea"));
                tx1.send_modify(|s| {
                    if let Some(s) = s {
                        s.announces += 1;
                        match res {
                            Ok(()) => {
                                s.last_announce_ok_unix = Some(unix_now());
                                s.last_error = None;
                            }
                            Err(e) => s.last_error = Some(e),
                        }
                    }
                });
            }
        }));
        let (c2, id2, tx2, ctx2) = (
            c.clone(),
            identity.clone(),
            msg_tx.clone(),
            coord_tx.clone(),
        );
        coord_tasks.push(tokio::spawn(async move {
            loop {
                let (c, k) = (c2.clone(), id2.clone());
                let res = tokio::task::spawn_blocking(move || {
                    crate::coord::events(&c, &k, device, unix_now(), 25)
                })
                .await
                .unwrap_or(Err("fallo de la tarea"));
                match res {
                    Ok(events) => {
                        for e in events {
                            ctx2.send_modify(|s| {
                                if let Some(s) = s {
                                    s.events += 1;
                                }
                            });
                            let _ = tx2.send(CoordMsg::Candidates(e.peer, e.candidates)).await;
                        }
                    }
                    // Aún sin presencia o coordinator caído: reintento con pausa.
                    Err(_) => tokio::time::sleep(Duration::from_secs(3)).await,
                }
            }
        }));
    }
    let mut stun = cfg.coordinator.as_ref().map(|c| Stun::new(c.stun.clone()));
    let mut stun_started: Option<Instant> = None;

    let mut rt = Runtime {
        cfg,
        local,
        state,
        tun,
        engine,
        seen: None,
        iface: None,
        last_saved: unix_now(),
    };
    tracing::info!(interface = %rt.tun.name(), listen = %rt.cfg.listen, "agente en marcha; sin tráfico hasta cargar una configuración vigente");
    rt.poll_config(unix_now());

    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
    // Paso fino (250 ms) para el sondeo con jitter; lo caro (configuración, estado) cada segundo.
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut stun_refresh = tokio::time::interval(Duration::from_secs(30));
    let mut ticks: u64 = 0;
    let mut tun_buf = vec![0u8; 4096];
    let mut udp_buf = vec![0u8; 4096];

    loop {
        let outs = tokio::select! {
            r = rt.tun.read_packet(&mut tun_buf) => match r {
                Ok(n) => rt.engine.on_tun(&tun_buf[..n], Instant::now(), unix_now()),
                Err(e) => return Err(format!("lectura TUN: {e}")),
            },
            r = udp.recv_from(&mut udp_buf) => match r {
                Ok((n, from)) => {
                    let data = &udp_buf[..n];
                    if looks_like_stun(data) {
                        // Respuesta STUN en el socket de datos compartido (ver nat-traversal).
                        if let Some(s) = stun.as_mut() {
                            s.on_response(data);
                        }
                        Vec::new()
                    } else {
                        rt.engine.on_udp(from_wire(from), data, Instant::now(), unix_now())
                    }
                }
                Err(e) => { tracing::debug!("recv_from: {e}"); continue }
            },
            _ = tick.tick() => {
                let unix = unix_now();
                let now = Instant::now();
                ticks += 1;
                if ticks.is_multiple_of(4) {
                    rt.poll_config(unix);
                    rt.housekeeping(unix);
                }
                // STUN: tras la respuesta (o 3 s), recalcula candidatos y NAT y anuncia si cambian.
                if let (Some(s), Some(started)) = (stun.as_mut(), stun_started) {
                    s.expire(now, Duration::from_secs(3));
                    if s.complete() || now.duration_since(started) >= Duration::from_secs(3) {
                        stun_started = None;
                        let (mapping, reflexive) = s.result();
                        let mut cands = net::local_addrs(&rt.cfg.interface, rt.cfg.listen.port()).await;
                        if let Some(r) = reflexive && !cands.contains(&r) {
                            cands.push(r);
                        }
                        rt.engine.set_nat(NatStatus {
                            mapping: match mapping { "independent" => "independent", "dependent" => "dependent", _ => "unknown" },
                            reflexive: reflexive.map(|r| r.to_string()),
                            candidates: cands.iter().map(|c| c.to_string()).collect(),
                        });
                        if *cand_tx.borrow() != cands {
                            tracing::info!(mapping, candidates = cands.len(), "candidatos propios actualizados");
                            let _ = cand_tx.send(cands);
                            announce_now.notify_one();
                        }
                    }
                }
                // Rendezvous pendientes (sin ruta confirmada y con tráfico o ruta perdida).
                if let Some(c) = rt.cfg.coordinator.clone() {
                    for peer in rt.engine.take_rendezvous(now) {
                        let (c, k, tx, st) = (c.clone(), identity.clone(), msg_tx.clone(), coord_tx.clone());
                        tokio::spawn(async move {
                            let res = tokio::task::spawn_blocking(move || {
                                crate::coord::rendezvous(&c, &k, device, peer, unix_now())
                            })
                            .await
                            .unwrap_or(Err("fallo de la tarea"));
                            st.send_modify(|s| {
                                if let Some(s) = s {
                                    s.rendezvous += 1;
                                    if let Err(e) = &res {
                                        s.last_error = Some(e);
                                    }
                                }
                            });
                            if let Ok(cands) = res {
                                let _ = tx.send(CoordMsg::Candidates(peer, cands)).await;
                            }
                        });
                    }
                }
                rt.engine.on_tick(now, unix)
            },
            _ = stun_refresh.tick(), if stun.is_some() => {
                if let Some(s) = stun.as_mut() {
                    s.start(&udp, dual, Instant::now()).await;
                    stun_started = Some(Instant::now());
                }
                Vec::new()
            },
            Some(CoordMsg::Candidates(peer, cands)) = msg_rx.recv() => {
                rt.engine.on_peer_candidates(&peer, cands, Instant::now(), unix_now())
            },
            r = ipc.accept() => {
                let sync = sync_rx.borrow().clone();
                let coord = coord_rx.borrow().clone();
                if let Ok((stream, _)) = r && handle_ipc(stream, &rt.engine, sync, coord).await { break; }
                continue;
            }
            _ = term.recv() => break,
            _ = int.recv() => break,
        };
        for o in outs {
            match o {
                Out::Udp(to, dg) => {
                    let Some(wire) = to_wire(to, dual) else {
                        continue;
                    };
                    if let Err(e) = udp.send_to(&dg, wire).await {
                        tracing::debug!("send_to {to}: {e}");
                    }
                }
                Out::Tun(p) => {
                    if let Err(e) = rt.tun.write_packet(&p).await {
                        tracing::debug!("escritura TUN: {e}");
                    }
                }
            }
        }
    }
    if let Some(task) = sync_task {
        task.abort();
    }
    for t in coord_tasks {
        t.abort();
    }
    tracing::info!("cerrando: la interfaz y sus rutas se eliminan al cerrar la TUN");
    let _ = state::save(&rt.cfg.state_dir, &rt.state);
    let _ = std::fs::remove_file(&rt.cfg.ipc_socket);
    Ok(())
}

/// Mensajes de las tareas del coordinator hacia el bucle principal.
enum CoordMsg {
    /// Candidatos de un par (por rendezvous propio o por aviso del coordinator): solo pistas.
    Candidates(atenea_identity::DeviceId, Vec<SocketAddr>),
}

#[derive(Clone, Default, serde::Serialize)]
struct CoordStatus {
    url: String,
    last_announce_ok_unix: Option<u64>,
    last_error: Option<&'static str>,
    announces: u64,
    events: u64,
    rendezvous: u64,
}

fn bind_ipc(path: &std::path::Path) -> Result<UnixListener, String> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        // Un socket vivo significa otro agente con la misma configuración.
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(format!("{}: ya hay un agente activo", path.display()));
        }
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let l = UnixListener::bind(path).map_err(|e| format!("IPC {}: {e}", path.display()))?;
    // Solo el propietario (root); el reparto por usuario/grupo llega con la fase 7.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    Ok(l)
}

/// Una petición por línea (`status` | `stop`) y una respuesta JSON por línea. `true` = parar.
async fn handle_ipc(
    stream: UnixStream,
    engine: &Engine,
    sync: Option<crate::sync::Status>,
    coord: Option<CoordStatus>,
) -> bool {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line)).await;
    if !matches!(read, Ok(Ok(n)) if n > 0) {
        return false;
    }
    let (reply, stop) = match line.trim() {
        "status" => {
            let mut status =
                serde_json::to_value(engine.status(Instant::now(), unix_now())).unwrap_or_default();
            status["sync"] = serde_json::to_value(sync).unwrap_or_default();
            status["coordinator"] = serde_json::to_value(coord).unwrap_or_default();
            (status.to_string(), false)
        }
        "stop" => (r#"{"ok":true}"#.to_string(), true),
        _ => (r#"{"error":"orden desconocida"}"#.to_string(), false),
    };
    let _ = reader
        .get_mut()
        .write_all(format!("{reply}\n").as_bytes())
        .await;
    stop
}
