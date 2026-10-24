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
            })
            .collect(),
        rules: vec![],
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
        StatusCode::UNAUTHORIZED
    );
    let l = Lab::new(Config {
        max_body_bytes: 1,
        ..Config::default()
    });
    assert_eq!(l.announce(3, &[]).await.0, StatusCode::PAYLOAD_TOO_LARGE);
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
