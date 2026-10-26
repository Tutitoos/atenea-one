// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{config::Config, rate::Rates};
use atenea_nat_traversal::{Message, encode_binding_success, looks_like_stun, parse};
use std::{
    io,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::{net::UdpSocket, sync::watch};

/// Dos sockets comparten el cubo por IP; la cancelación no deja tareas ni sockets vivos.
pub async fn serve(
    sockets: [UdpSocket; 2],
    config: Config,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let limits = Arc::new(Mutex::new(Rates::<IpAddr>::new(config.max_rate_entries)));
    let start = Instant::now();
    let [first, second] = sockets;
    tokio::select! {
        result=socket_loop(first,&config,limits.clone(),start)=>result,
        result=socket_loop(second,&config,limits,start)=>result,
        _=async {while !*stop.borrow() {if stop.changed().await.is_err(){break;}}}=>Ok(()),
    }
}
async fn socket_loop(
    socket: UdpSocket,
    config: &Config,
    limits: Arc<Mutex<Rates<IpAddr>>>,
    start: Instant,
) -> io::Result<()> {
    let mut buf = [0; 129];
    loop {
        let (len, source) = socket.recv_from(&mut buf).await?;
        let data = &buf[..len];
        if !looks_like_stun(data) {
            continue;
        }
        let Ok(Message::BindingRequest { tx }) = parse(data) else {
            continue;
        };
        if !limits.lock().expect("límites STUN").allow(
            source.ip(),
            start.elapsed().as_secs(),
            f64::from(config.stun_per_second),
            config.stun_burst,
        ) {
            continue;
        }
        let response = encode_binding_success(&tx, source);
        socket.send_to(&response, source).await?;
    }
}
/// Enlaza ambos puertos antes de iniciar el servicio: un fallo no deja medio servidor activo.
pub async fn bind(config: &Config) -> io::Result<[UdpSocket; 2]> {
    let ip: IpAddr = config
        .stun_bind
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let first = UdpSocket::bind((ip, config.stun_ports[0])).await?;
    let second = UdpSocket::bind((ip, config.stun_ports[1])).await?;
    Ok([first, second])
}
#[cfg(test)]
mod tests {
    use super::*;
    use atenea_nat_traversal::encode_binding_request;
    use std::time::Duration;
    #[tokio::test]
    async fn udp_binding_garbage_and_shared_rate_limit() {
        let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addresses = [a.local_addr().unwrap(), b.local_addr().unwrap()];
        let (tx, rx) = watch::channel(false);
        let config = Config {
            stun_burst: 1,
            stun_per_second: 1,
            ..Config::default()
        };
        let task = tokio::spawn(serve([a, b], config, rx));
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut buf = [0; 128];
        client.send_to(b"garbage", addresses[0]).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), client.recv(&mut buf))
                .await
                .is_err()
        );
        let request = encode_binding_request(&[7; 12]);
        client.send_to(&request, addresses[0]).await.unwrap();
        let n = tokio::time::timeout(Duration::from_secs(1), client.recv(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            parse(&buf[..n]),
            Ok(Message::BindingSuccess {
                tx: [7; 12],
                mapped: client.local_addr().unwrap()
            })
        );
        assert!(n <= 44);
        client.send_to(&request, addresses[1]).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), client.recv(&mut buf))
                .await
                .is_err()
        );
        tx.send(true).unwrap();
        task.await.unwrap().unwrap();
    }
}
