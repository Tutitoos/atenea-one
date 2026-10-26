// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use atenea_coordinator::{Coordinator, config::Config, router};
use atenea_identity::{api_auth::sign_request, *};
use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tower::ServiceExt;

struct Lab {
    state: Arc<Coordinator>,
    router: Router,
    now: Arc<AtomicU64>,
    nonce: AtomicU64,
}
impl Lab {
    fn new(mut config: Config) -> Self {
        config.anchors = vec![hex::encode(signing_key([1; 32]).verifying_key().to_bytes())];
        let now = Arc::new(AtomicU64::new(1000));
        let clock = now.clone();
        let state = Coordinator::with_clock(config, Arc::new(move || clock.load(Ordering::SeqCst)))
            .unwrap();
        Self {
            router: router(state.clone()),
            state,
            now,
            nonce: AtomicU64::new(1),
        }
    }
    async fn request(&self, method: &str, path: &str, data: Value, id: u8) -> (StatusCode, Value) {
        let bytes = if data.is_null() {
            vec![]
        } else {
            serde_json::to_vec(&data).unwrap()
        };
        let mut nonce = [0; 16];
        nonce[..8].copy_from_slice(&self.nonce.fetch_add(1, Ordering::SeqCst).to_be_bytes());
        let h = sign_request(
            &signing_key([id; 32]),
            [id; 16],
            method,
            path,
            &bytes,
            self.now.load(Ordering::SeqCst),
            nonce,
        );
        self.raw(method, path, bytes, Some(h)).await
    }
    async fn raw(
        &self,
        method: &str,
        path: &str,
        bytes: Vec<u8>,
        header: Option<String>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(h) = header {
            req = req.header("authorization", h);
        }
        let mut req = req.body(Body::from(bytes)).unwrap();
        req.extensions_mut().insert(ConnectInfo(
            "192.0.2.10:1234".parse::<SocketAddr>().unwrap(),
        ));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }
    async fn announce(&self, id: u8, peers: &[u8]) -> (StatusCode, Value) {
        self.request(
            "POST",
            "/v1/announce",
            announcement(id, peers, 1, 4, 1200, "192.0.2.1:443"),
            id,
        )
        .await
    }
    fn advance(&self, secs: u64) {
        self.now.fetch_add(secs, Ordering::SeqCst);
    }
}
fn announcement(id: u8, peers: &[u8], root: u8, network: u8, end: u64, candidate: &str) -> Value {
    let root = signing_key([root; 32]);
    let issuer = signing_key([2; 32]);
    let cert = IssuerCert::issue(
        &root,
        [network; 16],
        1,
        500,
        5000,
        issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap();
    let cfg = DeviceConfig {
        network_id: [network; 16],
        device_id: [id; 16],
        issuer_serial: 1,
        config_version: 1,
        not_before: 900,
        not_after: end,
        noise_pubkey: [id + 30; 32],
        identity_pubkey: signing_key([id; 32]).verifying_key().to_bytes(),
        mtu: 1400,
        tunnel_addrs: vec![format!("10.0.0.{id}").parse().unwrap()],
        network_prefixes: vec![Prefix {
            addr: "10.0.0.0".parse().unwrap(),
            len: 24,
        }],
        peers: peers
            .iter()
            .map(|p| Peer {
                device_id: [*p; 16],
                noise_pubkey: [p + 30; 32],
                addrs: vec![format!("10.0.0.{p}").parse().unwrap()],
                endpoints: vec![],
                routes: vec![],
            })
            .collect(),
        rules: vec![],
        advertised: vec![],
        dns: atenea_identity::DnsConfig::default(),
    };
    json!({"config":STANDARD.encode(issue_config(&issuer,&cert,&cfg).unwrap()),"candidates":[candidate,"127.0.0.1:3",candidate]})
}
fn peer(id: u8) -> Value {
    json!({"peer":hex::encode([id;16])})
}

#[tokio::test]
async fn valid_config_replacement_expiry_and_sweeper() {
    let l = Lab::new(Config::default());
    let (status, result) = l.announce(3, &[4]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result, json!({"ttl_secs":60,"observed_ip":"192.0.2.10"}));
    l.advance(2);
    assert_eq!(
        l.request(
            "POST",
            "/v1/announce",
            announcement(3, &[4], 1, 4, 1030, "192.0.2.9:9"),
            3
        )
        .await
        .0,
        StatusCode::OK
    );
    l.announce(4, &[3]).await;
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(3), 4).await.1["candidates"],
        json!(["192.0.2.9:9"])
    );
    l.advance(58); // Configuración +30 y TTL expiran exactamente aquí.
    l.state.sweep();
    assert_eq!(
        l.raw("GET", "/healthz", vec![], None).await.1["presence"],
        1
    );
    l.advance(2);
    assert_eq!(
        l.raw("GET", "/healthz", vec![], None).await.1["presence"],
        0
    );
}

#[tokio::test]
async fn configurations_and_authentication_rejected_uniformly() {
    let l = Lab::new(Config::default());
    for data in [
        announcement(3, &[], 9, 4, 1200, "192.0.2.1:1"),
        announcement(3, &[], 1, 4, 950, "192.0.2.1:1"),
    ] {
        assert_eq!(
            l.request("POST", "/v1/announce", data, 3).await,
            (
                StatusCode::FORBIDDEN,
                json!({"error":{"code":"invalid_config","message":"invalid_config"}})
            )
        );
    }
    let mut corrupt = announcement(3, &[], 1, 4, 1200, "192.0.2.1:1");
    let mut file = STANDARD
        .decode(corrupt["config"].as_str().unwrap())
        .unwrap();
    *file.last_mut().unwrap() ^= 1;
    corrupt["config"] = json!(STANDARD.encode(file));
    assert_eq!(
        l.request("POST", "/v1/announce", corrupt, 3).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        l.request(
            "POST",
            "/v1/announce",
            announcement(3, &[], 1, 4, 1200, "192.0.2.1:1"),
            4
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let bytes = serde_json::to_vec(&announcement(3, &[], 1, 4, 1200, "192.0.2.1:1")).unwrap();
    let good = sign_request(
        &signing_key([3; 32]),
        [3; 16],
        "POST",
        "/v1/announce",
        &bytes,
        1000,
        [1; 16],
    );
    let invalid = sign_request(
        &signing_key([8; 32]),
        [3; 16],
        "POST",
        "/v1/announce",
        &bytes,
        1000,
        [2; 16],
    );
    let skew = sign_request(
        &signing_key([3; 32]),
        [3; 16],
        "POST",
        "/v1/announce",
        &bytes,
        939,
        [3; 16],
    );
    for h in [invalid, skew, "nonsense".into()] {
        assert_eq!(
            l.raw("POST", "/v1/announce", bytes.clone(), Some(h))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        l.raw("POST", "/v1/announce", bytes.clone(), Some(good.clone()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        l.raw("POST", "/v1/announce", bytes, Some(good)).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn mutual_authorization_and_uniform_unavailability() {
    let l = Lab::new(Config::default());
    l.announce(3, &[4, 5, 6]).await;
    l.announce(4, &[]).await;
    l.announce(5, &[3]).await;
    l.request(
        "POST",
        "/v1/announce",
        announcement(6, &[3], 1, 9, 1200, "192.0.2.1:1"),
        6,
    )
    .await;
    let absent = l.request("POST", "/v1/rendezvous", peer(7), 3).await;
    assert_eq!(absent.0, StatusCode::NOT_FOUND);
    for id in [4, 6] {
        assert_eq!(
            l.request("POST", "/v1/rendezvous", peer(id), 3).await,
            absent
        );
    }
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(3), 4).await,
        absent
    );
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(5), 3).await.0,
        StatusCode::OK
    );
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(3), 8).await.0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn long_poll_delivery_bounded_queue_and_event_expiry() {
    let l = Arc::new(Lab::new(Config {
        presence_ttl_secs: 1000,
        rendezvous_per_minute: 100,
        ..Config::default()
    }));
    l.announce(3, &[4]).await;
    l.announce(4, &[3]).await;
    let poll = {
        let l = l.clone();
        tokio::spawn(async move { l.request("GET", "/v1/events?wait=30", Value::Null, 4).await })
    };
    tokio::task::yield_now().await;
    l.request("POST", "/v1/rendezvous", peer(4), 3).await;
    let (status, value) = tokio::time::timeout(std::time::Duration::from_secs(1), poll)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["events"][0]["kind"], "connect");
    for _ in 0..20 {
        l.advance(1);
        assert_eq!(
            l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
            StatusCode::OK
        );
    }
    let value = l
        .request("GET", "/v1/events?wait=0", Value::Null, 4)
        .await
        .1;
    assert_eq!(value["events"].as_array().unwrap().len(), 16);
    assert_eq!(value["events"][0]["seq"], 6);
    l.advance(1);
    l.request("POST", "/v1/rendezvous", peer(4), 3).await;
    l.advance(30);
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 4)
            .await
            .1,
        json!({"events":[]})
    );
}

#[tokio::test]
async fn rate_presence_nonce_body_and_poll_limits() {
    let l = Lab::new(Config {
        max_presences: 1,
        ..Config::default()
    });
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::OK);
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(l.announce(4, &[]).await.0, StatusCode::TOO_MANY_REQUESTS);
    let l = Lab::new(Config::default());
    l.announce(3, &[4]).await;
    l.announce(4, &[3]).await;
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
        StatusCode::OK
    );
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let l = Lab::new(Config {
        rendezvous_per_minute: 1,
        ..Config::default()
    });
    l.announce(3, &[4]).await;
    l.announce(4, &[3]).await;
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
        StatusCode::OK
    );
    l.advance(1);
    assert_eq!(
        l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let l = Lab::new(Config {
        max_nonces: 1,
        ..Config::default()
    });
    l.announce(3, &[]).await;
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let l = Lab::new(Config {
        max_body_bytes: 1,
        ..Config::default()
    });
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn device_nonce_quota_preserves_other_network() {
    let l = Lab::new(Config {
        max_nonces: 64,
        ..Config::default()
    });
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::OK);
    assert_eq!(
        l.request(
            "POST",
            "/v1/announce",
            announcement(4, &[], 1, 5, 1200, "192.0.2.2:443"),
            4,
        )
        .await
        .0,
        StatusCode::OK
    );
    for _ in 0..100 {
        let status = l
            .request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .0;
        assert!(matches!(
            status,
            StatusCode::OK | StatusCode::TOO_MANY_REQUESTS
        ));
    }
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 4)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn device_poll_quota_preserves_other_network_and_cancellation() {
    let l = Arc::new(Lab::new(Config::default()));
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::OK);
    assert_eq!(
        l.request(
            "POST",
            "/v1/announce",
            announcement(4, &[], 1, 5, 1200, "192.0.2.2:443"),
            4,
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut polls = Vec::new();
    for _ in 0..2 {
        let l = l.clone();
        polls.push(tokio::spawn(async move {
            l.request("GET", "/v1/events?wait=30", Value::Null, 3).await
        }));
        tokio::task::yield_now().await;
    }
    l.advance(2);
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::OK);
    assert!(l.state.metrics_text().contains("atenea_long_polls 2\n"));
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 4)
            .await
            .0,
        StatusCode::OK
    );
    for poll in polls {
        poll.abort();
        let _ = poll.await;
    }
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .0,
        StatusCode::OK
    );
    assert!(l.state.metrics_text().contains("atenea_long_polls 0\n"));
}

#[tokio::test]
async fn network_nonce_quota_preserves_other_network() {
    let l = Lab::new(Config {
        max_nonces: 64,
        ..Config::default()
    });
    for id in 3..=6 {
        assert_eq!(l.announce(id, &[]).await.0, StatusCode::OK);
    }
    assert_eq!(
        l.request(
            "POST",
            "/v1/announce",
            announcement(7, &[], 1, 5, 1200, "192.0.2.2:443"),
            7
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut accepted = 4;
    for id in 3..=6 {
        for _ in 0..15 {
            let status = l
                .request("GET", "/v1/events?wait=0", Value::Null, id)
                .await
                .0;
            if status == StatusCode::OK {
                accepted += 1;
            } else {
                assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
            }
        }
    }
    assert_eq!(accepted, 32);
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 7)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn network_poll_quota_preserves_other_network() {
    let l = Arc::new(Lab::new(Config {
        max_long_polls: 16,
        ..Config::default()
    }));
    for id in 3..=5 {
        assert_eq!(l.announce(id, &[]).await.0, StatusCode::OK);
    }
    assert_eq!(
        l.request(
            "POST",
            "/v1/announce",
            announcement(6, &[], 1, 5, 1200, "192.0.2.2:443"),
            6
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut polls = Vec::new();
    for id in [3, 3, 4, 4] {
        let l = l.clone();
        polls.push(tokio::spawn(async move {
            l.request("GET", "/v1/events?wait=30", Value::Null, id)
                .await
        }));
        tokio::task::yield_now().await;
    }
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 5)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 6)
            .await
            .0,
        StatusCode::OK
    );
    for poll in polls {
        poll.abort();
        let _ = poll.await;
    }
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 5)
            .await
            .0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn cancelled_poll_releases_capacity_and_sweeper_runs() {
    let l = Arc::new(Lab::new(Config {
        max_long_polls: 1,
        ..Config::default()
    }));
    l.announce(3, &[]).await;
    let poll = {
        let l = l.clone();
        tokio::spawn(async move { l.request("GET", "/v1/events?wait=30", Value::Null, 3).await })
    };
    tokio::task::yield_now().await;
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    poll.abort();
    let _ = poll.await;
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .0,
        StatusCode::OK
    );
    let (tx, rx) = tokio::sync::watch::channel(false);
    l.advance(60);
    let task = tokio::spawn(l.state.clone().run_sweeper(rx));
    tokio::task::yield_now().await;
    tx.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(
        l.raw("GET", "/healthz", vec![], None).await.1["presence"],
        0
    );
}

struct RelayKey(std::path::PathBuf);
impl RelayKey {
    fn new() -> Self {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path =
            std::env::temp_dir().join(format!("atenea-relay-key-{}", hex::encode(generate_seed())));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", hex::encode([90; 32])).unwrap();
        Self(path)
    }
    fn config(&self) -> Config {
        self.config_with_relays(2)
    }
    fn config_with_relays(&self, count: u8) -> Config {
        Config {
            relay_signing_key_file: Some(self.0.clone()),
            relays: (1..=count)
                .map(|id| {
                    serde_json::from_value(json!({
                        "relay_id":hex::encode([id; 16]),
                        "name":format!("lab-{id}"),
                        "udp":"relay.example:443",
                        "tls":"[2001:db8::1]:443",
                        "tls_name":"relay.example"
                    }))
                    .unwrap()
                })
                .collect(),
            ..Config::default()
        }
    }
}
impl Drop for RelayKey {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}

fn checked_token(
    value: &Value,
    relay: u8,
    src: u8,
    dst: u8,
    now: u64,
) -> atenea_relay_core::RelayToken {
    assert_eq!(value["relay_id"], hex::encode([relay; 16]));
    let bytes = STANDARD.decode(value["token"].as_str().unwrap()).unwrap();
    assert_eq!(bytes.len(), 189);
    let token = atenea_relay_core::RelayToken::parse(&bytes).unwrap();
    token
        .verify(&[signing_key([90; 32]).verifying_key().to_bytes()], now)
        .unwrap();
    assert_eq!(token.relay_id, [relay; 16]);
    assert_eq!(token.network_id, [4; 16]);
    assert_eq!(token.src_device, [src; 16]);
    assert_eq!(token.dst_device, [dst; 16]);
    assert_eq!(
        token.src_identity_pub,
        signing_key([src; 32]).verifying_key().to_bytes()
    );
    assert_eq!(token.not_before, now);
    assert_eq!(token.not_after, now + 300);
    token
}

#[tokio::test]
async fn relay_two_and_four_audiences_inverse_events_and_grant_limits() {
    let key = RelayKey::new();
    for count in [2, 4] {
        let config = key.config_with_relays(count);
        let endpoints = serde_json::to_value(&config.relays).unwrap();
        let l = Lab::new(config);
        assert_eq!(l.announce(3, &[4]).await.0, StatusCode::OK);
        assert_eq!(l.announce(4, &[3]).await.0, StatusCode::OK);
        assert_eq!(
            l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
            StatusCode::OK
        );
        let (status, response) = l.request("POST", "/v1/relay", peer(4), 3).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["ttl_secs"], 300);
        assert!(response.get("token").is_none());
        assert_eq!(response.as_object().unwrap().len(), 2);
        assert_eq!(
            response["relays"].as_array().unwrap().len(),
            usize::from(count)
        );
        let events = l
            .request("GET", "/v1/events?wait=0", Value::Null, 4)
            .await
            .1;
        assert_eq!(events["events"][0]["kind"], "connect");
        let event = &events["events"][1];
        assert_eq!(event["kind"], "relay");
        assert_eq!(event["peer"], hex::encode([3; 16]));
        assert_eq!(event["seq"], 2);
        assert_eq!(event["ttl_secs"], 300);
        assert!(event.get("token").is_none());
        assert!(event.get("candidates").is_none());
        assert_eq!(
            event["relays"].as_array().unwrap().len(),
            usize::from(count)
        );
        let mut token_ids = std::collections::HashSet::new();
        for (index, relay) in (1..=count).enumerate() {
            let grant = &response["relays"][index];
            let inverse_grant = &event["relays"][index];
            for (name, value) in endpoints[index].as_object().unwrap() {
                assert_eq!(&grant[name], value);
                assert_eq!(&inverse_grant[name], value);
            }
            let token = checked_token(grant, relay, 3, 4, 1000);
            let inverse = checked_token(inverse_grant, relay, 4, 3, 1000);
            assert!(token_ids.insert(token.token_id));
            assert!(token_ids.insert(inverse.token_id));
        }
        assert_eq!(
            l.request("POST", "/v1/relay", peer(3), 4).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        l.advance(5);
        let (status, second) = l.request("POST", "/v1/relay", peer(3), 4).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            second["relays"].as_array().unwrap().len(),
            usize::from(count)
        );
        let second_events = l
            .request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .1;
        assert_eq!(second_events["events"].as_array().unwrap().len(), 1);
        let inverse = &second_events["events"][0];
        assert_eq!(inverse["peer"], hex::encode([4; 16]));
        assert_eq!(
            inverse["relays"].as_array().unwrap().len(),
            usize::from(count)
        );
        for (index, relay) in (1..=count).enumerate() {
            let token = checked_token(&second["relays"][index], relay, 4, 3, 1005);
            let reverse = checked_token(&inverse["relays"][index], relay, 3, 4, 1005);
            assert!(token_ids.insert(token.token_id));
            assert!(token_ids.insert(reverse.token_id));
        }
        assert_eq!(token_ids.len(), 4 * usize::from(count));
        l.advance(5);
        assert_eq!(
            l.request("POST", "/v1/relay", peer(4), 3).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        l.advance(50);
        assert_eq!(l.announce(3, &[4]).await.0, StatusCode::OK);
        assert_eq!(l.announce(4, &[3]).await.0, StatusCode::OK);
        assert_eq!(
            l.request("POST", "/v1/relay", peer(4), 3).await.0,
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn relay_authorization_matches_rendezvous_and_missing_configuration() {
    let key = RelayKey::new();
    let l = Lab::new(key.config());
    l.announce(3, &[4, 5, 6]).await;
    l.announce(4, &[]).await;
    l.announce(5, &[3]).await;
    l.request(
        "POST",
        "/v1/announce",
        announcement(6, &[3], 1, 9, 1200, "192.0.2.1:1"),
        6,
    )
    .await;
    for id in [4, 6, 7] {
        assert_eq!(
            l.request("POST", "/v1/relay", peer(id), 3).await,
            l.request("POST", "/v1/rendezvous", peer(id), 3).await
        );
    }
    assert_eq!(
        l.request("POST", "/v1/relay", peer(3), 4).await,
        l.request("POST", "/v1/rendezvous", peer(3), 4).await
    );
    assert_eq!(
        l.request("POST", "/v1/relay", peer(3), 8).await,
        l.request("POST", "/v1/rendezvous", peer(3), 8).await
    );
    assert_eq!(
        l.raw(
            "POST",
            "/v1/relay",
            serde_json::to_vec(&peer(5)).unwrap(),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        l.request("POST", "/v1/relay", peer(5), 3).await.0,
        StatusCode::OK
    );
    for config in [
        Config::default(),
        Config {
            relays: key.config().relays,
            ..Config::default()
        },
        Config {
            relay_signing_key_file: Some(key.0.clone()),
            ..Config::default()
        },
    ] {
        let l = Lab::new(config);
        l.announce(3, &[4]).await;
        l.announce(4, &[3]).await;
        let response = l.request("POST", "/v1/relay", peer(4), 3).await;
        assert_eq!(
            response,
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":{"code":"relay_unavailable","message":"relay_unavailable"}})
            )
        );
        assert_eq!(
            l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn relay_device_limit_and_shared_bounded_expiring_queue() {
    let key = RelayKey::new();
    let mut config = key.config_with_relays(4);
    config.rendezvous_per_minute = 100;
    let l = Lab::new(config);
    let peers: Vec<u8> = (4..=14).collect();
    l.announce(3, &peers).await;
    for id in &peers {
        l.announce(*id, &[3]).await;
    }
    for id in 4..14 {
        assert_eq!(
            l.request("POST", "/v1/relay", peer(id), 3).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(
        l.request("POST", "/v1/relay", peer(14), 3).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let l = Lab::new(Config {
        rendezvous_per_minute: 100,
        ..key.config()
    });
    l.announce(3, &[4]).await;
    l.announce(4, &[3]).await;
    for _ in 0..16 {
        assert_eq!(
            l.request("POST", "/v1/rendezvous", peer(4), 3).await.0,
            StatusCode::OK
        );
        l.advance(1);
    }
    assert_eq!(
        l.request("POST", "/v1/relay", peer(4), 3).await.0,
        StatusCode::OK
    );
    let events = l
        .request("GET", "/v1/events?wait=0", Value::Null, 4)
        .await
        .1;
    assert_eq!(events["events"].as_array().unwrap().len(), 16);
    assert_eq!(events["events"][0]["seq"], 2);
    assert_eq!(events["events"][15]["kind"], "relay");
    l.advance(5);
    assert_eq!(
        l.request("POST", "/v1/relay", peer(3), 4).await.0,
        StatusCode::OK
    );
    l.advance(30);
    assert_eq!(
        l.request("GET", "/v1/events?wait=0", Value::Null, 3)
            .await
            .1,
        json!({"events":[]})
    );
}

#[test]
fn relay_config_validation_key_permissions_and_print_command() {
    use std::os::unix::fs::PermissionsExt;
    let key = RelayKey::new();
    let mut config = key.config();
    config.anchors = vec![hex::encode(signing_key([1; 32]).verifying_key().to_bytes())];
    assert!(config.validate().is_ok());
    for mut relays in [
        json!([{"name":"empty"}]),
        json!([{"name":"bad","udp":"host"}]),
        json!([{"name":"bad","udp":"host:0"}]),
        json!([{"name":"bad","udp":"host:65536"}]),
        json!([{"name":"bad","tls":"host:443"}]),
        json!([{"name":"bad","udp":":443"}]),
        json!([{"name":"","udp":"host:443"}]),
        json!([{"name":"  ","udp":"host:443"}]),
    ] {
        relays[0]["relay_id"] = json!(hex::encode([1; 16]));
        config.relays = serde_json::from_value(relays).unwrap();
        assert!(Coordinator::new(config.clone()).is_err());
    }
    let valid = json!({"relay_id":hex::encode([1; 16]),"name":"lab","udp":"host:443"});
    let mut missing_id = valid.clone();
    missing_id.as_object_mut().unwrap().remove("relay_id");
    assert!(serde_json::from_value::<atenea_coordinator::config::Relay>(missing_id).is_err());
    for id in [
        "",
        "1",
        "111111111111111111111111111111111",
        "11111111111111111111111111111111111",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "gggggggggggggggggggggggggggggggg",
        "00000000000000000000000000000000",
    ] {
        let mut relay = valid.clone();
        relay["relay_id"] = json!(id);
        config.relays = serde_json::from_value(json!([relay])).unwrap();
        assert!(Coordinator::new(config.clone()).is_err());
    }
    for duplicate_id in [true, false] {
        config = key.config();
        config.anchors = vec![hex::encode(signing_key([1; 32]).verifying_key().to_bytes())];
        if duplicate_id {
            config.relays[1].relay_id = config.relays[0].relay_id.clone();
        } else {
            config.relays[1].name = config.relays[0].name.clone();
        }
        assert!(Coordinator::new(config.clone()).is_err());
    }
    config = key.config_with_relays(4);
    config.anchors = vec![hex::encode(signing_key([1; 32]).verifying_key().to_bytes())];
    assert!(Coordinator::new(config.clone()).is_ok());
    config.relays.push(
        serde_json::from_value(json!({
            "relay_id":hex::encode([5; 16]),"name":"fifth","udp":"host:443"
        }))
        .unwrap(),
    );
    assert!(Coordinator::new(config.clone()).is_err());
    config = key.config();
    config.relay_token_ttl_secs = 601;
    assert!(config.validate().is_err());
    config.relay_token_ttl_secs = 0;
    assert!(config.validate().is_err());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_atenea-coordinator"))
        .arg("--print-relay-key")
        .env_remove("ATENEA_COORD_CONFIG")
        .env("ATENEA_COORD_RELAY_SIGNING_KEY_FILE", &key.0)
        .env(
            "ATENEA_COORD_RELAYS",
            r#"[{"relay_id":"11111111111111111111111111111111","name":"lab","udp":"relay.example:443"}]"#,
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        hex::encode(signing_key([90; 32]).verifying_key().to_bytes())
    );
    for invalid in [
        r#"[{"relay_id":"11111111111111111111111111111111","name":"lab","udp":"host"}]"#,
        r#"[{"name":"lab","udp":"host:443"}]"#,
        r#"[{"relay_id":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","name":"lab","udp":"host:443"}]"#,
        "not json",
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_atenea-coordinator"))
            .env_remove("ATENEA_COORD_CONFIG")
            .env(
                "ATENEA_COORD_ANCHORS",
                format!(
                    "[\"{}\"]",
                    hex::encode(signing_key([1; 32]).verifying_key().to_bytes())
                ),
            )
            .env("ATENEA_COORD_RELAYS", invalid)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    for mode in [0o400, 0o600] {
        std::fs::set_permissions(&key.0, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(config.relay_signing_key().unwrap().is_some());
    }
    for mode in [0o440, 0o640, 0o644, 0o604, 0o700] {
        std::fs::set_permissions(&key.0, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(config.relay_signing_key().is_err());
    }
    std::fs::set_permissions(&key.0, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&key.0, "abcd").unwrap();
    assert!(config.relay_signing_key().is_err());
}
