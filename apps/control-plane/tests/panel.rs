// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;

use atenea_control_plane::{AppState, config::Config, panel};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Reply, Session, TestApp, now};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, client::IntoClientRequest},
};
use uuid::Uuid;

async fn network(t: &TestApp, user: &Session) -> (Uuid, Uuid) {
    let org = t.org(user).await;
    let root = atenea_identity::signing_key(atenea_identity::generate_seed());
    let net = t
        .network(user, org, &root, "10.80.0.0/24", "fd80::/64")
        .await;
    (org, net)
}

async fn device(t: &TestApp, user: &Session, net: Uuid, approved: bool) -> Device {
    let device = Device::new();
    let token = t.token(user, net, approved).await;
    assert_eq!(t.enroll(&token, &device).await.status, StatusCode::CREATED);
    device
}

async fn report(t: &TestApp, device: &Device, value: Value) -> Reply {
    let body = serde_json::to_vec(&value).unwrap();
    let auth = atenea_identity::api_auth::sign_request(
        &device.key,
        *device.id.as_bytes(),
        "POST",
        "/api/v1/device/status",
        &body,
        now(),
        *Uuid::new_v4().as_bytes(),
    );
    t.send(
        Request::builder()
            .method("POST")
            .uri("/api/v1/device/status")
            .header("content-type", "application/json")
            .header("authorization", auth)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
}

async fn inventory(t: &TestApp, user: &Session, net: Uuid) -> Value {
    let reply = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices"),
            Some(user),
            Value::Null,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    reply.json
}

fn entry(items: &Value, device: Uuid) -> &Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == device.to_string())
        .unwrap()
}

#[tokio::test]
async fn inventory_reports_validation_and_device_authentication() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (_, net) = network(&t, &user).await;
    let approved = device(&t, &user, net, true).await;
    let peer = device(&t, &user, net, true).await;
    let pending = device(&t, &user, net, false).await;
    let revoked = device(&t, &user, net, true).await;
    assert_eq!(
        t.action(&user, net, revoked.id, "revoke").await.status,
        StatusCode::OK
    );
    let outsider = t.user().await;
    let (_, foreign_net) = network(&t, &outsider).await;
    let foreign = device(&t, &outsider, foreign_net, true).await;
    let initial = inventory(&t, &user, net).await;
    let active = entry(&initial, approved.id);
    assert!(active["ipv4"].as_str().unwrap().starts_with("10.80.0."));
    assert!(active["ipv6"].as_str().unwrap().starts_with("fd80:"));
    assert!(active["enrolled_at"].is_string());
    assert!(active["approved_at"].is_string());
    assert!(active["report"].is_null());
    assert_eq!(active["connection"], "unknown");
    assert!(active["last_config_at"].is_null());
    for dev in [&pending, &revoked] {
        let item = entry(&initial, dev.id);
        assert!(item["ipv4"].is_null());
        assert!(item["ipv6"].is_null());
    }
    assert!(entry(&initial, revoked.id)["revoked_at"].is_string());
    let path =
        json!({"peer": peer.id, "path": "direct", "last_rx_secs": 3, "rtt_ms": 20, "relay": null});
    for body in [
        json!({"paths": [{"peer":foreign.id,"path":"direct"}]}),
        json!({"paths": [{"peer":peer.id,"path":"invalid"}]}),
        json!({"paths": vec![path.clone();257]}),
        json!({"paths": [{"peer":"not-a-uuid","path":"none"}]}),
        json!({"nat_mapping":"invalid","paths":[]}),
        json!({"paths":[{"peer":peer.id,"path":"direct","last_rx_secs":-1}]}),
        json!({"paths":[],"unexpected":true}),
        json!({"paths":[{"peer":peer.id,"path":"direct","relay":"x".repeat(65536)}]}),
    ] {
        assert_eq!(
            report(&t, &approved, body).await.status,
            StatusCode::BAD_REQUEST
        );
    }
    for dev in [&pending, &revoked] {
        report(&t, dev, json!({"paths":[]}))
            .await
            .error(StatusCode::FORBIDDEN, "device_not_approved");
    }
    t.json("POST", "/api/v1/device/status", None, json!({"paths":[]}))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    for (key, timestamp) in [(&peer.key, now()), (&approved.key, now() - 120)] {
        let bytes = b"{\"paths\":[]}";
        let auth = atenea_identity::api_auth::sign_request(
            key,
            *approved.id.as_bytes(),
            "POST",
            "/api/v1/device/status",
            bytes,
            timestamp,
            *Uuid::new_v4().as_bytes(),
        );
        t.send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/device/status")
                .header("authorization", auth)
                .header("content-type", "application/json")
                .body(Body::from(bytes.as_slice()))
                .unwrap(),
        )
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    let body = json!({"nat_mapping":"independent","paths":[path.clone()]});
    let audit_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(
        report(&t, &approved, body.clone()).await.status,
        StatusCode::NO_CONTENT
    );
    report(&t, &approved, body)
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    let audit_after: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(audit_before, audit_after);
    let items = inventory(&t, &user, net).await;
    let saved = &entry(&items, approved.id)["report"];
    assert!(saved["reported_at"].is_string());
    assert_eq!(entry(&items, approved.id)["connection"], "online");
    assert!(saved["age_secs"].as_u64().unwrap() < 90);
    assert_eq!(saved["nat_mapping"], "independent");
    assert_eq!(saved["paths"], json!([path]));
    sqlx::query(
        "UPDATE device_reports SET reported_at=now()-interval '6 seconds' WHERE device_id=$1",
    )
    .bind(approved.id)
    .execute(&t.pool)
    .await
    .unwrap();
    assert_eq!(
        report(&t, &approved, json!({"paths":[]})).await.status,
        StatusCode::NO_CONTENT
    );
    let items = inventory(&t, &user, net).await;
    assert_eq!(entry(&items, approved.id)["report"]["paths"], json!([]));
    assert!(entry(&items, approved.id)["report"]["nat_mapping"].is_null());
    for (seconds, connection) in [(90, "stale"), (600, "offline")] {
        sqlx::query("UPDATE device_reports SET reported_at=now()-make_interval(secs => $2) WHERE device_id=$1")
            .bind(approved.id).bind(f64::from(seconds)).execute(&t.pool).await.unwrap();
        let items = inventory(&t, &user, net).await;
        assert_eq!(entry(&items, approved.id)["connection"], connection);
    }
    t.finish().await;
}

#[tokio::test]
async fn metrics_sample_real_reports_and_isolate_networks() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (_, net) = network(&t, &user).await;
    let stranger = t.user().await;
    let path = format!("/api/v1/networks/{net}/metrics");
    t.json("GET", &path, Some(&stranger), Value::Null)
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    let empty = t.json("GET", &path, Some(&user), Value::Null).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.json, json!([]));
    let mut devices = Vec::new();
    for _ in 0..6 {
        devices.push(device(&t, &user, net, true).await);
    }
    let _pending = device(&t, &user, net, false).await;
    for (index, kind) in ["direct", "relay-udp", "probing", "none", "direct"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            report(
                &t,
                &devices[index],
                json!({"paths":[{"peer":devices[5].id,"path":kind}]})
            )
            .await
            .status,
            StatusCode::NO_CONTENT
        );
    }
    let at = chrono::DateTime::from_timestamp(now() as i64, 0).unwrap();
    sqlx::query("UPDATE device_reports SET reported_at=$1 WHERE device_id=$2")
        .bind(at - chrono::Duration::minutes(11))
        .bind(devices[4].id)
        .execute(&t.pool)
        .await
        .unwrap();
    let state = AppState::new(t.pool.clone(), Config::default(), t.issuer.clone());
    panel::sample(&state, at).await.unwrap();
    let samples = t.json("GET", &path, Some(&user), Value::Null).await;
    assert_eq!(samples.status, StatusCode::OK);
    let sample = &samples.json[0];
    for (key, count) in [
        ("total", 6),
        ("direct", 1),
        ("relay", 1),
        ("probing", 1),
        ("unknown", 2),
        ("offline", 1),
    ] {
        assert_eq!(sample[key], count);
    }
    panel::sample(&state, at + chrono::Duration::minutes(1))
        .await
        .unwrap();
    let limited = t
        .json("GET", &format!("{path}?limit=1"), Some(&user), Value::Null)
        .await;
    assert_eq!(limited.json.as_array().unwrap().len(), 1);
    assert_eq!(
        limited.json[0]["at"],
        json!(at + chrono::Duration::minutes(1))
    );
    let future =
        (at + chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    assert_eq!(
        t.json(
            "GET",
            &format!("{path}?since={future}"),
            Some(&user),
            Value::Null
        )
        .await
        .json,
        json!([])
    );
    assert_eq!(
        t.json(
            "GET",
            &format!("{path}?limit=1001"),
            Some(&user),
            Value::Null
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    panel::sample(&state, at + chrono::Duration::days(8))
        .await
        .unwrap();
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM network_samples WHERE network_id=$1")
            .bind(net)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(remaining, 1);
    drop(state);
    t.finish().await;
}

#[tokio::test]
async fn static_spa_fallback_preserves_api_errors_and_security_headers() {
    let dir = std::env::temp_dir().join(format!("atenea-panel-{}", Uuid::new_v4()));
    let Some(t) = TestApp::configured(Config {
        web_dir: Some(dir.clone()),
        ..Config::default()
    })
    .await
    else {
        return;
    };
    tokio::fs::create_dir(&dir).await.unwrap();
    tokio::fs::write(dir.join("index.html"), "<main>Panel de prueba</main>")
        .await
        .unwrap();
    tokio::fs::write(dir.join("app-deadbeef.js"), "export {};")
        .await
        .unwrap();
    for path in ["/", "/networks/example", "/missing.js"] {
        let reply = t
            .send(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.body, b"<main>Panel de prueba</main>");
        assert_eq!(reply.headers["cache-control"], "no-cache");
        assert_eq!(reply.headers["x-content-type-options"], "nosniff");
        assert_eq!(reply.headers["referrer-policy"], "same-origin");
        assert_eq!(
            reply.headers["content-security-policy"],
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"
        );
    }
    let asset = t
        .send(
            Request::builder()
                .uri("/app-deadbeef.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(asset.status, StatusCode::OK);
    assert_eq!(asset.body, b"export {};");
    assert!(
        asset.headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    for path in [
        "/api/v1/unknown",
        "/auth/unknown",
        "/healthz/unknown",
        "/metrics",
    ] {
        let reply = t
            .send(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert_ne!(reply.body, b"<main>Panel de prueba</main>");
    }
    t.finish().await;
    tokio::fs::remove_dir_all(dir).await.unwrap();
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn websocket(url: &str, session: &Session, origin: &str) -> Result<Socket, Error> {
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "cookie",
        format!("atenea_session={}", session.token).parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    connect_async(request).await.map(|(socket, _)| socket)
}

async fn event(socket: &mut Socket) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.expect("WebSocket abierto").unwrap();
            if message.is_text() {
                return serde_json::from_str(message.to_text().unwrap()).unwrap();
            }
        }
    })
    .await
    .expect("evento WebSocket en plazo")
}

fn rejected(result: Result<Socket, Error>, status: u16) {
    match result {
        Err(Error::Http(response)) => assert_eq!(response.status().as_u16(), status),
        Err(error) => panic!("fallo de handshake inesperado: {error}"),
        Ok(_) => panic!("handshake aceptado; se esperaba {status}"),
    }
}

#[tokio::test]
async fn websocket_real_tcp_hello_approval_origin_and_connection_limits() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let (org, net) = network(&t, &owner).await;
    let pending = device(&t, &owner, net, false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = t.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let url = format!("ws://{address}/api/v1/networks/{net}/events");
    let origin = "http://127.0.0.1:8080";
    rejected(
        websocket(&url, &owner, "https://foreign.example").await,
        403,
    );
    let stranger = t.user().await;
    rejected(websocket(&url, &stranger, origin).await, 404);
    let mut sockets = Vec::new();
    let mut first = websocket(&url, &owner, origin).await.unwrap();
    let hello = event(&mut first).await;
    assert_eq!(hello["type"], "hello");
    assert!(hello["seq"].is_u64());
    assert_eq!(
        t.action(&owner, net, pending.id, "approve").await.status,
        StatusCode::OK
    );
    let changed = event(&mut first).await;
    assert_eq!(changed["type"], "changed");
    assert_eq!(changed["scope"], "devices");
    assert!(changed["seq"].as_u64().unwrap() > hello["seq"].as_u64().unwrap());
    sockets.push(first);
    for _ in 1..8 {
        let mut socket = websocket(&url, &owner, origin).await.unwrap();
        assert_eq!(event(&mut socket).await["type"], "hello");
        sockets.push(socket);
    }
    rejected(websocket(&url, &owner, origin).await, 429);
    for _ in 0..7 {
        let user = t.user().await;
        sqlx::query("INSERT INTO memberships(org_id,user_id,role) VALUES($1,$2,'member')")
            .bind(org)
            .bind(user.user)
            .execute(&t.pool)
            .await
            .unwrap();
        for _ in 0..8 {
            let mut socket = websocket(&url, &user, origin).await.unwrap();
            assert_eq!(event(&mut socket).await["type"], "hello");
            sockets.push(socket);
        }
    }
    assert_eq!(sockets.len(), 64);
    let extra = t.user().await;
    sqlx::query("INSERT INTO memberships(org_id,user_id,role) VALUES($1,$2,'member')")
        .bind(org)
        .bind(extra.user)
        .execute(&t.pool)
        .await
        .unwrap();
    rejected(websocket(&url, &extra, origin).await, 429);
    for mut socket in sockets {
        let _ = socket.close(None).await;
    }
    server.abort();
    let _ = server.await;
    t.finish().await;
}
