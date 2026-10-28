// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;

use atenea_control_plane::config::Config;
use atenea_identity::{SigningKey, generate_seed, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Reply, Session, TestApp, now};
use serde_json::{Value, json};
use uuid::Uuid;

const INFO: &str = "/api/v1/server/info";
const PEERS: &str = "/api/v1/device/peers";

async fn network(
    t: &TestApp,
    owner: &Session,
    org: Uuid,
    v4: &str,
    v6: &str,
) -> (Uuid, SigningKey) {
    let root = signing_key(generate_seed());
    let net = t.network(owner, org, &root, v4, v6).await;
    (net, root)
}

fn request(
    device: &Device,
    path: &str,
    body: &[u8],
    timestamp: u64,
    nonce: [u8; 16],
) -> Request<Body> {
    let auth = atenea_identity::api_auth::sign_request(
        &device.key,
        *device.id.as_bytes(),
        "GET",
        path,
        body,
        timestamp,
        nonce,
    );
    Request::builder()
        .uri(path)
        .header("authorization", auth)
        .body(Body::from(body.to_vec()))
        .unwrap()
}

async fn signed(t: &TestApp, device: &Device, path: &str) -> Reply {
    t.send(request(
        device,
        path,
        b"",
        now(),
        *Uuid::new_v4().as_bytes(),
    ))
    .await
}

async fn inventory(t: &TestApp, owner: &Session, net: Uuid) -> Value {
    let r = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices"),
            Some(owner),
            Value::Null,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    r.json
}

fn role(items: &Value, device: &Device) -> Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == device.id.to_string())
        .unwrap()["role"]
        .clone()
}

async fn patch(
    t: &TestApp,
    session: Option<&Session>,
    net: Uuid,
    device: &Device,
    value: Value,
) -> Reply {
    t.json(
        "PATCH",
        &format!("/api/v1/networks/{net}/devices/{}", device.id),
        session,
        value,
    )
    .await
}

#[tokio::test]
async fn token_hereda_rol_y_clientes_anteriores_siguen_siendo_device() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, _) = network(&t, &owner, org, "10.101.0.0/24", "fd01:101::/64").await;
    for (requested, expected, auto) in [
        (None, "device", true),
        (Some("device"), "device", true),
        (Some("server"), "server", true),
        (None, "device", false),
        (Some("device"), "device", false),
        (Some("server"), "server", false),
    ] {
        let mut body = json!({"ttl_secs":600,"auto_approve":auto});
        if let Some(value) = requested {
            body["role"] = json!(value);
        }
        let token = t
            .json(
                "POST",
                &format!("/api/v1/networks/{net}/enrollment-tokens"),
                Some(&owner),
                body,
            )
            .await;
        assert_eq!(token.status, StatusCode::CREATED, "{}", token.json);
        let device = Device::new();
        // El rol lo decide el token, nunca el cuerpo que presenta el dispositivo.
        let enrolled = t
            .json(
                "POST",
                "/api/v1/enroll",
                None,
                json!({
                    "token":token.json["token"],"bundle":hex::encode(device.bundle.encode()),
                    "name":"Equipo","role":if expected == "server" { "device" } else { "server" }
                }),
            )
            .await;
        assert_eq!(enrolled.status, StatusCode::CREATED, "{}", enrolled.json);
        assert_eq!(role(&inventory(&t, &owner, net).await, &device), expected);
    }
    for invalid in [
        Value::Null,
        json!("relay"),
        json!("SERVER"),
        json!(""),
        json!(false),
        json!(7),
        json!({}),
    ] {
        t.json(
            "POST",
            &format!("/api/v1/networks/{net}/enrollment-tokens"),
            Some(&owner),
            json!({"ttl_secs":600,"role":invalid}),
        )
        .await
        .error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    t.finish().await;
}

#[tokio::test]
async fn solicitud_sin_token_no_permite_autoproclamarse_server() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, _) = network(&t, &owner, org, "10.102.0.0/24", "fd01:102::/64").await;
    let opened = t
        .json(
            "PUT",
            &format!("/api/v1/networks/{net}/join-policy"),
            Some(&owner),
            json!({"policy":"con_solicitud"}),
        )
        .await;
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.json);
    let device = Device::new();
    let joined = t.json("POST", &format!("/api/v1/networks/{net}/join-requests"), None,
        json!({"bundle":hex::encode(device.bundle.encode()),"name":"Servidor pretendido","role":"server"})).await;
    assert_eq!(joined.status, StatusCode::CREATED, "{}", joined.json);
    assert_eq!(role(&inventory(&t, &owner, net).await, &device), "device");
    assert_eq!(
        t.action(&owner, net, device.id, "approve").await.status,
        StatusCode::OK
    );
    assert_eq!(role(&inventory(&t, &owner, net).await, &device), "device");
    t.finish().await;
}

#[tokio::test]
async fn solo_owner_admin_cambian_rol_y_auditan_transiciones_efectivas() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, _) = network(&t, &owner, org, "10.103.0.0/24", "fd01:103::/64").await;
    let admin = t.user().await;
    let member = t.user().await;
    let outsider = t.user().await;
    for (user, membership) in [(&admin, "admin"), (&member, "member")] {
        let added = t
            .json(
                "PUT",
                &format!("/api/v1/orgs/{org}/members/{}", user.user),
                Some(&owner),
                json!({"role":membership}),
            )
            .await;
        assert!(added.status.is_success(), "{}", added.json);
    }
    let device = Device::new();
    let token = t.token(&owner, net, true).await;
    assert_eq!(t.enroll(&token, &device).await.status, StatusCode::CREATED);
    patch(&t, Some(&member), net, &device, json!({"role":"server"}))
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    patch(&t, Some(&outsider), net, &device, json!({"role":"server"}))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    let path = format!("/api/v1/networks/{net}/devices/{}", device.id);
    let body = serde_json::to_vec(&json!({"role":"server"})).unwrap();
    let auth = atenea_identity::api_auth::sign_request(
        &device.key,
        *device.id.as_bytes(),
        "PATCH",
        &path,
        &body,
        now(),
        *Uuid::new_v4().as_bytes(),
    );
    let denied = t
        .send(
            Request::builder()
                .method("PATCH")
                .uri(&path)
                .header("authorization", auth)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED, "{}", denied.json);
    assert_eq!(role(&inventory(&t, &owner, net).await, &device), "device");
    for invalid in [
        Value::Null,
        json!("relay"),
        json!(""),
        json!(false),
        json!(1),
        json!({}),
    ] {
        patch(&t, Some(&owner), net, &device, json!({"role":invalid}))
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    for (user, next) in [(&owner, "server"), (&owner, "server"), (&admin, "device")] {
        let changed = patch(&t, Some(user), net, &device, json!({"role":next})).await;
        assert_eq!(changed.status, StatusCode::OK, "{}", changed.json);
        assert_eq!(role(&inventory(&t, &owner, net).await, &device), next);
    }
    let events: Vec<(String, Value)> = sqlx::query_as("SELECT actor_id,detail FROM audit_events WHERE action='device.role' AND target=$1 ORDER BY id")
        .bind(device.id.to_string()).fetch_all(&t.pool).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, owner.user.to_string());
    assert_eq!(events[0].1["previous"], "device");
    assert_eq!(events[0].1["new"], "server");
    assert_eq!(events[1].0, admin.user.to_string());
    assert_eq!(events[1].1["previous"], "server");
    assert_eq!(events[1].1["new"], "device");
    t.finish().await;
}

#[tokio::test]
async fn peers_incluye_rol_actual_solo_de_pares_permitidos_de_su_red() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, root) = network(&t, &owner, org, "10.104.0.0/24", "fd01:104::/64").await;
    let (other, _) = network(&t, &owner, org, "10.105.0.0/24", "fd01:105::/64").await;
    let caller = Device::new();
    let visible = Device::new();
    let hidden = Device::new();
    let foreign = Device::new();
    for (device, network) in [
        (&caller, net),
        (&visible, net),
        (&hidden, net),
        (&foreign, other),
    ] {
        let token = t.token(&owner, network, true).await;
        assert_eq!(t.enroll(&token, device).await.status, StatusCode::CREATED);
        let changed = patch(&t, Some(&owner), network, device, json!({"role":"server"})).await;
        assert_eq!(changed.status, StatusCode::OK, "{}", changed.json);
    }
    t.certificate(&owner, net, &root).await;
    let policy = t.json("POST", &format!("/api/v1/networks/{net}/policies"), Some(&owner),
        json!({"src":{"kind":"device","device_id":caller.id},"dst":{"kind":"device","device_id":visible.id},"proto":6,"port_min":443,"port_max":443})).await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.json);
    let peers = signed(&t, &caller, PEERS).await;
    assert_eq!(peers.status, StatusCode::OK, "{}", peers.json);
    assert_eq!(
        peers.json,
        json!([{"device_id":visible.id,"name":"dispositivo sintético","role":"server"}])
    );
    assert_eq!(
        patch(&t, Some(&owner), net, &visible, json!({"role":"device"}))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(signed(&t, &caller, PEERS).await.json[0]["role"], "device");
    let removed = t
        .json(
            "DELETE",
            &format!(
                "/api/v1/networks/{net}/policies/{}",
                common::id(&policy.json)
            ),
            Some(&owner),
            Value::Null,
        )
        .await;
    assert!(removed.status.is_success());
    assert_eq!(signed(&t, &caller, PEERS).await.json, json!([]));
    t.finish().await;
}

#[tokio::test]
async fn server_info_firma_aprobacion_replay_y_ventana_temporal() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, _) = network(&t, &owner, org, "10.106.0.0/24", "fd01:106::/64").await;
    let active = Device::new();
    let pending = Device::new();
    let revoked = Device::new();
    for (device, auto) in [(&active, true), (&pending, false), (&revoked, true)] {
        let token = t.token(&owner, net, auto).await;
        assert_eq!(t.enroll(&token, device).await.status, StatusCode::CREATED);
    }
    assert_eq!(
        t.action(&owner, net, revoked.id, "revoke").await.status,
        StatusCode::OK
    );
    for device in [&pending, &revoked] {
        signed(&t, device, INFO)
            .await
            .error(StatusCode::FORBIDDEN, "device_not_approved");
    }
    let mut forged = Device::new();
    forged.id = active.id;
    for device in [&forged, &Device::new()] {
        signed(&t, device, INFO)
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    for authorization in [None, Some("invalid")] {
        let mut builder = Request::builder().uri(INFO);
        if let Some(value) = authorization {
            builder = builder.header("authorization", value);
        }
        t.send(builder.body(Body::empty()).unwrap())
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    for timestamp in [now() - 61, now() + 61] {
        t.send(request(
            &active,
            INFO,
            b"",
            timestamp,
            *Uuid::new_v4().as_bytes(),
        ))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    for timestamp in [now() - 59, now() + 59] {
        let accepted = t
            .send(request(
                &active,
                INFO,
                b"",
                timestamp,
                *Uuid::new_v4().as_bytes(),
            ))
            .await;
        assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.json);
    }
    let timestamp = now();
    let nonce = *Uuid::new_v4().as_bytes();
    let first = t.send(request(&active, INFO, b"", timestamp, nonce)).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.json);
    t.send(request(&active, INFO, b"", timestamp, nonce))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    // La firma no es transferible desde peers ni a un cuerpo diferente.
    let mut wrong_path = request(&active, PEERS, b"", now(), *Uuid::new_v4().as_bytes());
    *wrong_path.uri_mut() = INFO.parse().unwrap();
    t.send(wrong_path)
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    let mut wrong_body = request(&active, INFO, b"", now(), *Uuid::new_v4().as_bytes());
    *wrong_body.body_mut() = Body::from("alterado");
    t.send(wrong_body)
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    t.finish().await;
}

#[tokio::test]
async fn server_info_solo_certificado_propio_y_query_no_cambia_red() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, root) = network(&t, &owner, org, "10.107.0.0/24", "fd01:107::/64").await;
    let (other, foreign_root) = network(&t, &owner, org, "10.108.0.0/24", "fd01:108::/64").await;
    let device = Device::new();
    let token = t.token(&owner, net, true).await;
    assert_eq!(t.enroll(&token, &device).await.status, StatusCode::CREATED);
    t.certificate(&owner, other, &foreign_root).await;
    let no_cert = signed(&t, &device, INFO).await;
    assert_eq!(no_cert.status, StatusCode::OK, "{}", no_cert.json);
    assert_eq!(no_cert.headers["cache-control"], "no-store");
    assert!(
        no_cert
            .json
            .as_object()
            .unwrap()
            .contains_key("issuer_cert_not_after_unix")
    );
    assert_eq!(no_cert.json["issuer_cert_not_after_unix"], Value::Null);
    let query_without_cert = signed(&t, &device, &format!("{INFO}?network_id={other}")).await;
    assert_eq!(
        query_without_cert.status,
        StatusCode::OK,
        "{}",
        query_without_cert.json
    );
    assert_eq!(
        query_without_cert.json["issuer_cert_not_after_unix"],
        Value::Null
    );
    let expires = now() + 1800;
    let cert = atenea_identity::IssuerCert::issue(
        &root,
        *net.as_bytes(),
        1,
        now() - 60,
        expires,
        t.issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap();
    let uploaded = t
        .send(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/networks/{net}/issuer-cert"))
                .header("cookie", format!("atenea_session={}", owner.token))
                .header("x-csrf-token", &owner.csrf)
                .header("content-type", "application/octet-stream")
                .body(Body::from(cert.encode().to_vec()))
                .unwrap(),
        )
        .await;
    assert!(uploaded.status.is_success(), "{}", uploaded.json);
    for path in [
        INFO.to_owned(),
        format!("{INFO}?network_id={other}&net={other}"),
    ] {
        let before = now();
        let info = signed(&t, &device, &path).await;
        assert_eq!(info.status, StatusCode::OK, "{}", info.json);
        assert_eq!(info.headers["cache-control"], "no-store");
        assert_eq!(info.json["issuer_cert_not_after_unix"], expires);
        let time = info.json["server_time_unix"].as_u64().unwrap();
        assert!((before..=now()).contains(&time));
        assert_eq!(
            info.json["control_plane"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            !info.json["control_plane"]["commit"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        let mut keys: Vec<_> = info
            .json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "control_plane",
                "issuer_cert_not_after_unix",
                "server_time_unix"
            ]
        );
    }
    t.finish().await;
}

#[tokio::test]
async fn server_info_y_peers_comparten_cuota_por_dispositivo() {
    let Some(t) = TestApp::configured(Config {
        device_per_minute: 2,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, root) = network(&t, &owner, org, "10.109.0.0/24", "fd01:109::/64").await;
    t.certificate(&owner, net, &root).await;
    let device = Device::new();
    let another = Device::new();
    for dev in [&device, &another] {
        let token = t.token(&owner, net, true).await;
        assert_eq!(t.enroll(&token, dev).await.status, StatusCode::CREATED);
    }
    assert_eq!(signed(&t, &device, INFO).await.status, StatusCode::OK);
    assert_eq!(signed(&t, &device, PEERS).await.status, StatusCode::OK);
    signed(&t, &device, INFO)
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert_eq!(signed(&t, &another, INFO).await.status, StatusCode::OK);
    assert_eq!(signed(&t, &another, PEERS).await.status, StatusCode::OK);
    signed(&t, &another, PEERS)
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    t.finish().await;
}

#[tokio::test]
async fn server_info_y_peers_rechazan_cuerpos_mayores_de_2mib() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, root) = network(&t, &owner, org, "10.110.0.0/24", "fd01:110::/64").await;
    let device = Device::new();
    let token = t.token(&owner, net, true).await;
    assert_eq!(t.enroll(&token, &device).await.status, StatusCode::CREATED);
    t.certificate(&owner, net, &root).await;
    for path in [INFO, PEERS] {
        let accepted = vec![b'a'; 2 * 1024 * 1024];
        let ok = t
            .send(request(
                &device,
                path,
                &accepted,
                now(),
                *Uuid::new_v4().as_bytes(),
            ))
            .await;
        assert_eq!(ok.status, StatusCode::OK, "{}", ok.json);
        let oversized = vec![b'a'; 2 * 1024 * 1024 + 1];
        let rejected = t
            .send(request(
                &device,
                path,
                &oversized,
                now(),
                *Uuid::new_v4().as_bytes(),
            ))
            .await;
        assert_eq!(
            rejected.status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "{}",
            rejected.json
        );
    }
    t.finish().await;
}

#[tokio::test]
async fn migracion_rol_preserva_dispositivos_y_tokens_anteriores_como_device() {
    use sqlx::Executor;

    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (net, _) = network(&t, &owner, org, "10.111.0.0/24", "fd01:111::/64").await;
    let existing = Device::new();
    let consumed = t.token(&owner, net, true).await;
    assert_eq!(
        t.enroll(&consumed, &existing).await.status,
        StatusCode::CREATED
    );
    let legacy_token = t.token(&owner, net, true).await;
    t.pool.execute("ALTER TABLE devices DROP COLUMN role; ALTER TABLE enrollment_tokens DROP COLUMN role; DELETE FROM _sqlx_migrations WHERE version=8;").await.unwrap();
    atenea_control_plane::db::migrate(&t.pool, None)
        .await
        .unwrap();
    assert_eq!(role(&inventory(&t, &owner, net).await, &existing), "device");
    let token_roles: Vec<String> = sqlx::query_scalar("SELECT role FROM enrollment_tokens")
        .fetch_all(&t.pool)
        .await
        .unwrap();
    assert!(!token_roles.is_empty());
    assert!(token_roles.iter().all(|value| value == "device"));
    let new_device = Device::new();
    assert_eq!(
        t.enroll(&legacy_token, &new_device).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        role(&inventory(&t, &owner, net).await, &new_device),
        "device"
    );
    atenea_control_plane::db::migrate(&t.pool, None)
        .await
        .unwrap();
    assert_eq!(role(&inventory(&t, &owner, net).await, &existing), "device");
    t.finish().await;
}
