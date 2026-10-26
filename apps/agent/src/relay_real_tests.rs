// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Integración local: HTTP firmado real, tokens reales y relay UDP real.
use super::*;
use crate::{coord, run::RelayRequests};
use atenea_identity::{DeviceConfig, IssuerCert, Peer, Prefix, issue_config, signing_key};
use base64::Engine as _;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncReadExt;

fn assert_grant_times(grant: &RelayEndpoint, issued: u64) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&grant.token)
        .unwrap();
    let token = wire::RelayToken::parse(&bytes).unwrap();
    assert_eq!((token.not_before, token.not_after), (issued, issued + 300));
    assert_eq!(token.relay_id, grant.relay_id);
}

struct TestDir(std::path::PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct TestTask(tokio::task::JoinHandle<()>);
impl Drop for TestTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn signed_config(id: u8, identity: &SigningKey, peer: u8, unix: u64) -> Vec<u8> {
    let issuer = signing_key([9; 32]);
    let cert = IssuerCert::issue(
        &signing_key([1; 32]),
        [1; 16],
        1,
        500,
        4000,
        issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap();
    issue_config(
        &issuer,
        &cert,
        &DeviceConfig {
            network_id: [1; 16],
            device_id: [id; 16],
            issuer_serial: 1,
            config_version: unix,
            not_before: unix,
            not_after: unix + 300,
            noise_pubkey: [id + 30; 32],
            identity_pubkey: identity.verifying_key().to_bytes(),
            mtu: 1400,
            tunnel_addrs: vec![format!("10.0.0.{id}").parse().unwrap()],
            network_prefixes: vec![Prefix {
                addr: "10.0.0.0".parse().unwrap(),
                len: 24,
            }],
            peers: vec![Peer {
                device_id: [peer; 16],
                noise_pubkey: [peer + 30; 32],
                addrs: vec![format!("10.0.0.{peer}").parse().unwrap()],
                endpoints: vec![],
                routes: vec![],
            }],
            rules: vec![],
            advertised: vec![],
            dns: Default::default(),
        },
    )
    .unwrap()
}

async fn announce(
    config: &crate::config::CoordinatorConfig,
    key: &SigningKey,
    id: u8,
    peer: u8,
    unix: u64,
) {
    let (config, key, signed) = (
        config.clone(),
        key.clone(),
        signed_config(id, key, peer, unix),
    );
    tokio::task::spawn_blocking(move || {
        coord::announce(&config, &key, [id; 16], &signed, &[], unix)
    })
    .await
    .unwrap()
    .unwrap();
}
async fn inverse(
    config: &crate::config::CoordinatorConfig,
    key: &SigningKey,
    id: u8,
    unix: u64,
) -> Vec<RelayEndpoint> {
    let (config, key) = (config.clone(), key.clone());
    tokio::task::spawn_blocking(move || {
        coord::events(&config, &key, [id; 16], unix, 0)
            .unwrap()
            .into_iter()
            .find_map(|event| {
                if let coord::Event::Relay { grant, .. } = event {
                    Some(grant.relays)
                } else {
                    None
                }
            })
            .expect("concesión inversa real")
    })
    .await
    .unwrap()
}
async fn relay_status(path: &str) -> serde_json::Value {
    let mut stream = tokio::net::UnixStream::connect(path).await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
async fn ready(
    events: &mut mpsc::Receiver<RelayEvent>,
    client: &RelayClient,
    peer: DeviceId,
    grants: &[RelayEndpoint],
    status_path: &str,
) -> usize {
    timeout(Duration::from_secs(12), async {
        let mut attempts = 1;
        loop {
            match events.recv().await.unwrap() {
                RelayEvent::Ready { .. } => break attempts,
                event @ RelayEvent::Down { .. } => {
                    let status = client.status();
                    eprintln!(
                        "initial_timeout_source={:?} destination={:?}",
                        client.hub.get().unwrap().socket.local_addr(),
                        grants[0].udp
                    );
                    assert_eq!(
                        status.entries[0].last_error.as_deref(),
                        Some("udp_timeout"),
                        "Down inicial no recuperable: {event:?}; {status:?}"
                    );
                    assert!(
                        attempts < 3,
                        "más de tres aperturas iniciales; {status:?}; relay={}",
                        relay_status(status_path).await
                    );
                    attempts += 1;
                    client.open(peer, grants.to_vec());
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap()
}
async fn received(events: &mut mpsc::Receiver<RelayEvent>, payload: &[u8]) {
    timeout(Duration::from_secs(4), async {
        loop {
            match events.recv().await.unwrap() {
                RelayEvent::Rx { datagram, .. } => {
                    assert_eq!(datagram, payload);
                    break;
                }
                event @ (RelayEvent::Down { .. } | RelayEvent::Ready { .. }) => {
                    panic!("worker recreado: {event:?}")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn real_coordinator_and_relay_renew_before_expiry_without_down() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = TestDir(std::env::temp_dir().join(format!(
        "ar-{}",
        atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..4])
    )));
    std::fs::create_dir(&dir.0).unwrap();
    let signer_path = dir.0.join("signer");
    let mut signer = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&signer_path)
        .unwrap();
    writeln!(signer, "{}", atenea_crypto::hex::encode(&[3; 32])).unwrap();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    std::fs::write(dir.0.join("cert"), cert.cert.pem()).unwrap();
    std::fs::write(dir.0.join("key"), cert.signing_key.serialize_pem()).unwrap();
    let clock = Arc::new(AtomicU64::new(1000));
    let relay_clock = clock.clone();
    let status_path = dir.0.join("status").display().to_string();
    let relay = atenea_relay::Relay::bind_with_clock(
        atenea_relay::config::Config {
            relay_id: "06".repeat(16),
            // Reservar la familia de los hubs evita colisiones efímeras dual-stack en macOS.
            udp_listen: "[::]:0".parse().unwrap(),
            tcp_listen: "127.0.0.1:0".parse().unwrap(),
            metrics_listen: "127.0.0.1:0".into(),
            coordinator_keys: vec![atenea_crypto::hex::encode(
                &signing_key([3; 32]).verifying_key().to_bytes(),
            )],
            tls_cert_file: dir.0.join("cert").display().to_string(),
            tls_key_file: dir.0.join("key").display().to_string(),
            status_socket: status_path.clone(),
            ..Default::default()
        },
        Arc::new(move || relay_clock.load(Ordering::SeqCst) * 1000),
    )
    .await
    .unwrap();
    let relay_addr = format!("127.0.0.1:{}", relay.udp_addr().unwrap().port());
    let (stop, stopped) = watch::channel(false);
    let _relay_task = TestTask(tokio::spawn(async move {
        relay.run(stopped).await.unwrap();
    }));
    let coord_clock = clock.clone();
    let coordinator = atenea_coordinator::Coordinator::with_clock(
        atenea_coordinator::config::Config {
            anchors: vec![atenea_crypto::hex::encode(
                &signing_key([1; 32]).verifying_key().to_bytes(),
            )],
            relay_signing_key_file: Some(signer_path),
            relays: vec![atenea_coordinator::config::Relay {
                relay_id: "06".repeat(16),
                name: "real".into(),
                udp: Some(relay_addr.clone()),
                tls: None,
                tls_name: None,
            }],
            ..Default::default()
        },
        Arc::new(move || coord_clock.load(Ordering::SeqCst)),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = crate::config::CoordinatorConfig {
        url: format!("http://{}", listener.local_addr().unwrap()),
        stun: vec![],
        relay: "auto".into(),
        relay_ca_file: None,
        insecure_http: true,
    };
    let _coordinator_task = TestTask(tokio::spawn(async move {
        axum::serve(
            listener,
            atenea_coordinator::router(coordinator)
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    }));
    let keys = [signing_key([42; 32]), signing_key([43; 32])];
    let ids = [2_u8, 5_u8];
    let mut clients = Vec::new();
    let mut events = Vec::new();
    for i in 0..2 {
        announce(&config, &keys[i], ids[i], ids[1 - i], 1000).await;
        let (mut client, rx) =
            RelayClient::new(Default::default(), keys[i].clone(), [ids[i]; 16], [1; 16]);
        let client_clock = clock.clone();
        client.clock = Arc::new(move || client_clock.load(Ordering::SeqCst));
        clients.push(client);
        events.push(rx);
    }
    let (c, key) = (config.clone(), keys[0].clone());
    let initial =
        tokio::task::spawn_blocking(move || coord::relay(&c, &key, [2; 16], [5; 16], 1000))
            .await
            .unwrap()
            .unwrap();
    for grant in &initial.relays {
        assert_grant_times(grant, 1000);
    }
    let mut grants = [initial.relays, inverse(&config, &keys[1], 5, 1000).await];
    let mut initial_attempts = Vec::new();
    for i in 0..2 {
        // Solo el destino de laboratorio es loopback; token, HTTP y ALLOC son reales.
        clients[i].open([ids[1 - i]; 16], grants[i].clone());
        initial_attempts.push(
            ready(
                &mut events[i],
                &clients[i],
                [ids[1 - i]; 16],
                &grants[i],
                &status_path,
            )
            .await,
        );
    }
    eprintln!("initial_open_attempts={initial_attempts:?}");
    let allocation_ids: Vec<_> = clients
        .iter()
        .enumerate()
        .map(|(i, client)| {
            client
                .hub
                .get()
                .unwrap()
                .routes
                .lock()
                .unwrap()
                .get(&[ids[1 - i]; 16])
                .unwrap()
                .id
        })
        .collect();
    let start = std::time::Instant::now();
    let mut requests = [RelayRequests::default(), RelayRequests::default()];
    let mut renewals = 0;
    for elapsed in (20..=340).step_by(20) {
        let unix = 1000 + elapsed;
        clock.store(unix, Ordering::SeqCst);
        for i in 0..2 {
            announce(&config, &keys[i], ids[i], ids[1 - i], unix).await;
        }
        for i in 0..2 {
            for peer in clients[i].renewal_requests(unix) {
                requests[i].want_renewal(peer);
            }
            let monotonic = start + Duration::from_secs(elapsed);
            for peer in requests[i].take(monotonic) {
                renewals += 1;
                assert_eq!((i, elapsed), (0, 240), "solicitante estable y margen real");
                let (c, key, own) = (config.clone(), keys[i].clone(), ids[i]);
                let grant = tokio::task::spawn_blocking(move || {
                    coord::relay(&c, &key, [own; 16], peer, unix)
                })
                .await
                .unwrap()
                .unwrap();
                requests[i].finished(peer, monotonic, true, None);
                grants[i] = grant.relays;
                grants[1 - i] = inverse(&config, &keys[1 - i], ids[1 - i], unix).await;
                for endpoint in grants.iter().flatten() {
                    assert_grant_times(endpoint, unix);
                }
                for j in 0..2 {
                    clients[j].open([ids[1 - j]; 16], grants[j].clone());
                }
                timeout(Duration::from_secs(4), async {
                    while clients
                        .iter()
                        .any(|client| client.status().entries[0].renew_at <= 1300)
                    {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
            }
        }
        if elapsed % 60 == 0 {
            let before = relay_status(&status_path).await["traffic"]["allocations"]
                .as_u64()
                .unwrap();
            // Disparar ALLOC periódico sin avanzar los timers de E/S del SO.
            for i in 0..2 {
                clients[i].open([ids[1 - i]; 16], grants[i].clone());
            }
            timeout(Duration::from_secs(4), async {
                while relay_status(&status_path).await["traffic"]["allocations"]
                    .as_u64()
                    .unwrap()
                    < before + 2
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        let mut payload = vec![0; atenea_protocol::DATA_OVERHEAD + 8];
        payload[0] = atenea_protocol::VERSION;
        payload[1] = atenea_protocol::TYPE_DATA;
        payload[14..22].copy_from_slice(&unix.to_be_bytes());
        for i in 0..2 {
            clients[i].send([ids[1 - i]; 16], [6; 16], RelayKind::Udp, payload.clone());
            received(&mut events[1 - i], &payload).await;
        }
        for rx in &mut events {
            while let Ok(event) = rx.try_recv() {
                assert!(
                    !matches!(event, RelayEvent::Down { .. } | RelayEvent::Ready { .. }),
                    "{event:?}"
                );
            }
        }
    }
    assert_eq!(
        renewals, 1,
        "un solicitante estable; el otro recibe la inversa"
    );
    assert_eq!(relay_status(&status_path).await["allocations"], 2);
    for (i, client) in clients.iter().enumerate() {
        assert_eq!(
            client
                .hub
                .get()
                .unwrap()
                .routes
                .lock()
                .unwrap()
                .get(&[ids[1 - i]; 16])
                .unwrap()
                .id,
            allocation_ids[i]
        );
        assert_eq!(client.status().entries[0].state, "ready");
    }
    stop.send_replace(true);
}
