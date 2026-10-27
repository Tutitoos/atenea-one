// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;

use atenea_identity::{
    AgentState, LocalKeys, SigningKey, generate_seed, signing_key, verify_config,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Reply, Session, TestApp, now};
use serde_json::{Value, json};
use uuid::Uuid;

async fn network(t: &TestApp, user: &Session) -> (Uuid, SigningKey) {
    let org = t.org(user).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(user, org, &root, "10.83.0.0/24", "fd83::/64")
        .await;
    (net, root)
}

async fn enroll(
    t: &TestApp,
    user: &Session,
    net: Uuid,
    device: &Device,
    name: &str,
    auto: Option<bool>,
    approved: bool,
) -> Reply {
    let token = t.token(user, net, approved).await;
    let mut body = json!({"token":token,"bundle":hex::encode(device.bundle.encode()),"name":name});
    if let Some(auto) = auto {
        body["name_auto"] = json!(auto);
    }
    t.json("POST", "/api/v1/enroll", None, body).await
}

async fn signed(
    t: &TestApp,
    device: &Device,
    method: &str,
    path: &str,
    value: Option<Value>,
) -> Reply {
    let body = value
        .map(|v| serde_json::to_vec(&v).unwrap())
        .unwrap_or_default();
    let auth = atenea_identity::api_auth::sign_request(
        &device.key,
        *device.id.as_bytes(),
        method,
        path,
        &body,
        now(),
        *Uuid::new_v4().as_bytes(),
    );
    t.send(
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", auth)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
}

async fn report(t: &TestApp, device: &Device, hostname: Option<&str>) -> Reply {
    // La cuota de informes no es el comportamiento bajo prueba.
    sqlx::query(
        "UPDATE device_reports SET reported_at=now()-interval '6 seconds' WHERE device_id=$1",
    )
    .bind(device.id)
    .execute(&t.pool)
    .await
    .unwrap();
    let mut body = json!({"paths":[]});
    if let Some(hostname) = hostname {
        body["hostname"] = json!(hostname);
    }
    signed(t, device, "POST", "/api/v1/device/status", Some(body)).await
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

fn assert_name(items: &Value, device: Uuid, name: &str, source: &str, hostname: Value) {
    let item = entry(items, device);
    assert_eq!(item["name"], name);
    assert_eq!(item["name_source"], source);
    assert_eq!(item["hostname"], hostname);
}

async fn patch(t: &TestApp, user: &Session, net: Uuid, device: &Device, body: Value) -> Reply {
    t.json(
        "PATCH",
        &format!("/api/v1/networks/{net}/devices/{}", device.id),
        Some(user),
        body,
    )
    .await
}

#[tokio::test]
async fn enrolamiento_distingue_nombre_manual_y_automatico_y_trim_unicode() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (net, _) = network(&t, &user).await;
    for (auto, source) in [
        (None, "admin"),
        (Some(false), "admin"),
        (Some(true), "device"),
    ] {
        let device = Device::new();
        let reply = enroll(&t, &user, net, &device, "  Portátil 🦊  ", auto, true).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.json);
        assert_name(
            &inventory(&t, &user, net).await,
            device.id,
            "Portátil 🦊",
            source,
            if auto == Some(true) {
                json!("Portátil 🦊")
            } else {
                Value::Null
            },
        );
    }
    t.finish().await;
}

#[tokio::test]
async fn informe_conserva_hostname_omitido_y_solo_renombra_fuente_device() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (net, _) = network(&t, &user).await;
    for (auto, source) in [(false, "admin"), (true, "device")] {
        let device = Device::new();
        assert_eq!(
            enroll(&t, &user, net, &device, "Original", Some(auto), true)
                .await
                .status,
            StatusCode::CREATED
        );
        for hostname in ["  Máquina 🦊  ", "Servidor nuevo"] {
            assert_eq!(
                report(&t, &device, Some(hostname)).await.status,
                StatusCode::NO_CONTENT
            );
            let expected = if auto { hostname.trim() } else { "Original" };
            assert_name(
                &inventory(&t, &user, net).await,
                device.id,
                expected,
                source,
                json!(hostname.trim()),
            );
            assert_eq!(
                report(&t, &device, None).await.status,
                StatusCode::NO_CONTENT
            );
            assert_name(
                &inventory(&t, &user, net).await,
                device.id,
                expected,
                source,
                json!(hostname.trim()),
            );
        }
    }
    t.finish().await;
}

#[tokio::test]
async fn patch_nombre_fija_admin_y_restaurar_sistema_requiere_hostname_sin_combinacion() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (net, _) = network(&t, &user).await;
    let device = Device::new();
    assert_eq!(
        enroll(&t, &user, net, &device, "Original", None, true)
            .await
            .status,
        StatusCode::CREATED
    );
    patch(&t, &user, net, &device, json!({"use_system_name":true}))
        .await
        .error(StatusCode::CONFLICT, "no_system_name");
    assert_name(
        &inventory(&t, &user, net).await,
        device.id,
        "Original",
        "admin",
        Value::Null,
    );
    assert_eq!(
        report(&t, &device, Some("Sistema")).await.status,
        StatusCode::NO_CONTENT
    );
    assert!(
        patch(&t, &user, net, &device, json!({"name":"  Nombre 🦊  "}))
            .await
            .status
            .is_success()
    );
    assert_name(
        &inventory(&t, &user, net).await,
        device.id,
        "Nombre 🦊",
        "admin",
        json!("Sistema"),
    );
    assert_eq!(
        report(&t, &device, Some("Sistema nuevo")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_name(
        &inventory(&t, &user, net).await,
        device.id,
        "Nombre 🦊",
        "admin",
        json!("Sistema nuevo"),
    );
    patch(
        &t,
        &user,
        net,
        &device,
        json!({"name":"Rechazado","use_system_name":true}),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_request");
    assert_name(
        &inventory(&t, &user, net).await,
        device.id,
        "Nombre 🦊",
        "admin",
        json!("Sistema nuevo"),
    );
    assert!(
        patch(&t, &user, net, &device, json!({"use_system_name":true}))
            .await
            .status
            .is_success()
    );
    assert_name(
        &inventory(&t, &user, net).await,
        device.id,
        "Sistema nuevo",
        "device",
        json!("Sistema nuevo"),
    );
    assert_eq!(
        report(&t, &device, Some("Sistema final")).await.status,
        StatusCode::NO_CONTENT
    );
    assert_name(
        &inventory(&t, &user, net).await,
        device.id,
        "Sistema final",
        "device",
        json!("Sistema final"),
    );
    t.finish().await;
}

#[tokio::test]
async fn nombres_invalidos_no_mutan_datos_y_limite_cuenta_caracteres_unicode() {
    let Some(t) = TestApp::configured(atenea_control_plane::config::Config {
        enroll_per_minute: 10000,
        device_per_minute: 10000,
        ..Default::default()
    })
    .await
    else {
        return;
    };
    let user = t.user().await;
    let (net, _) = network(&t, &user).await;
    let device = Device::new();
    assert_eq!(
        enroll(&t, &user, net, &device, "Original", Some(true), true)
            .await
            .status,
        StatusCode::CREATED
    );
    assert_eq!(
        report(&t, &device, Some("Conservado")).await.status,
        StatusCode::NO_CONTENT
    );
    let mut invalid = vec![String::new(), "   ".into(), "a".repeat(65)];
    for cp in (0..=0x1f)
        .chain(0x7f..=0x9f)
        .chain(0x202a..=0x202e)
        .chain(0x2066..=0x2069)
    {
        invalid.push(format!("a{}b", char::from_u32(cp).unwrap()));
    }
    // También los controles que trim eliminaría deben rechazarse, no desaparecer.
    invalid.extend(["\nNombre".into(), "Nombre\t".into()]);
    for name in invalid {
        let rejected = Device::new();
        enroll(&t, &user, net, &rejected, &name, None, true)
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_name");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM devices WHERE id=$1")
            .bind(rejected.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "nombre inválido aceptado: {name:?}");
        patch(&t, &user, net, &device, json!({"name":name}))
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_name");
        report(&t, &device, Some(&name))
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_report");
        assert_name(
            &inventory(&t, &user, net).await,
            device.id,
            "Conservado",
            "device",
            json!("Conservado"),
        );
    }
    for name in ["á".repeat(64), "🦊".repeat(64), "Équipe de José 🦊".into()] {
        let accepted = Device::new();
        assert_eq!(
            enroll(&t, &user, net, &accepted, &name, None, true)
                .await
                .status,
            StatusCode::CREATED
        );
        assert!(
            patch(&t, &user, net, &device, json!({"name":name}))
                .await
                .status
                .is_success()
        );
        assert_eq!(
            report(&t, &device, Some(&name)).await.status,
            StatusCode::NO_CONTENT
        );
        assert_name(
            &inventory(&t, &user, net).await,
            device.id,
            &name,
            "admin",
            json!(name),
        );
    }
    t.finish().await;
}

#[tokio::test]
async fn peers_publica_solo_pares_config_y_nombres_actuales_sin_cache() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (net, root) = network(&t, &user).await;
    let mut devices = Vec::new();
    for approved in [true, true, true, true, false] {
        let device = Device::new();
        assert_eq!(
            enroll(&t, &user, net, &device, "Par", None, approved)
                .await
                .status,
            StatusCode::CREATED
        );
        devices.push(device);
    }
    let [caller, visible, hidden, revoked, pending] = devices.as_slice() else {
        unreachable!()
    };
    t.certificate(&user, net, &root).await;
    let policy = t.json("POST", &format!("/api/v1/networks/{net}/policies"), Some(&user),
        json!({"src":{"kind":"device","device_id":visible.id},"dst":{"kind":"device","device_id":caller.id},"proto":6,"port_min":443,"port_max":443})).await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.json);
    assert_eq!(
        t.action(&user, net, revoked.id, "revoke").await.status,
        StatusCode::OK
    );
    assert!(
        patch(
            &t,
            &user,
            net,
            visible,
            json!({"name":"  Par visible 🦊  "})
        )
        .await
        .status
        .is_success()
    );
    let reply = signed(&t, caller, "GET", "/api/v1/device/peers", None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(reply.headers["cache-control"], "no-store");
    assert_eq!(
        reply.json,
        json!([{"device_id":visible.id,"name":"Par visible 🦊"}])
    );
    let cfg = t.config(caller, now(), *Uuid::new_v4().as_bytes()).await;
    assert_eq!(cfg.status, StatusCode::OK, "{}", cfg.json);
    let verified = verify_config(
        &cfg.body,
        &LocalKeys {
            noise_pub: caller.bundle.noise_pubkey,
            identity_pub: caller.bundle.identity_pubkey,
        },
        &AgentState::new(
            *net.as_bytes(),
            *caller.id.as_bytes(),
            root.verifying_key().to_bytes(),
        ),
        now(),
    )
    .unwrap();
    assert_eq!(verified.config.peers.len(), 1);
    assert_eq!(verified.config.peers[0].device_id, *visible.id.as_bytes());
    for excluded in [caller, hidden, revoked, pending] {
        assert!(
            !reply
                .json
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["device_id"] == excluded.id.to_string())
        );
    }
    // Sin políticas no hay nombres divulgados, igual que la configuración vacía.
    let removed = t
        .json(
            "DELETE",
            &format!(
                "/api/v1/networks/{net}/policies/{}",
                common::id(&policy.json)
            ),
            Some(&user),
            Value::Null,
        )
        .await;
    assert!(removed.status.is_success());
    let empty = signed(&t, caller, "GET", "/api/v1/device/peers", None).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.json, json!([]));
    assert_eq!(empty.headers["cache-control"], "no-store");
    t.finish().await;
}

#[tokio::test]
async fn peers_exige_firma_y_aprobacion_igual_que_config() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    let (net, root) = network(&t, &user).await;
    let active = Device::new();
    let pending = Device::new();
    let revoked = Device::new();
    for (device, approved) in [(&active, true), (&pending, false), (&revoked, true)] {
        assert_eq!(
            enroll(&t, &user, net, device, "Par", None, approved)
                .await
                .status,
            StatusCode::CREATED
        );
    }
    assert_eq!(
        t.action(&user, net, revoked.id, "revoke").await.status,
        StatusCode::OK
    );
    t.certificate(&user, net, &root).await;
    for device in [&pending, &revoked] {
        signed(&t, device, "GET", "/api/v1/device/peers", None)
            .await
            .error(StatusCode::FORBIDDEN, "device_not_approved");
        t.config(device, now(), *Uuid::new_v4().as_bytes())
            .await
            .error(StatusCode::FORBIDDEN, "device_not_approved");
    }
    let mut forged = Device::new();
    forged.id = active.id;
    let unknown = Device::new();
    for device in [&forged, &unknown] {
        signed(&t, device, "GET", "/api/v1/device/peers", None)
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
        t.config(device, now(), *Uuid::new_v4().as_bytes())
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    for path in ["/api/v1/device/peers", "/api/v1/device/config"] {
        t.send(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
        for timestamp in [now() - 61, now() + 61] {
            let auth = atenea_identity::api_auth::sign_request(
                &active.key,
                *active.id.as_bytes(),
                "GET",
                path,
                b"",
                timestamp,
                *Uuid::new_v4().as_bytes(),
            );
            t.send(
                Request::builder()
                    .uri(path)
                    .header("authorization", auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
        }
        let auth = atenea_identity::api_auth::sign_request(
            &active.key,
            *active.id.as_bytes(),
            "GET",
            path,
            b"",
            now(),
            *Uuid::new_v4().as_bytes(),
        );
        let first = t
            .send(
                Request::builder()
                    .uri(path)
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(first.status, StatusCode::OK, "{}", first.json);
        t.send(
            Request::builder()
                .uri(path)
                .header("authorization", auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    let config_auth = atenea_identity::api_auth::sign_request(
        &active.key,
        *active.id.as_bytes(),
        "GET",
        "/api/v1/device/config",
        b"",
        now(),
        *Uuid::new_v4().as_bytes(),
    );
    t.send(
        Request::builder()
            .uri("/api/v1/device/peers")
            .header("authorization", config_auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    t.finish().await;
}

#[test]
fn openapi_describe_nombres_y_peers_firmados() {
    let doc = serde_json::to_value(atenea_control_plane::openapi()).unwrap();
    let schemas = &doc["components"]["schemas"];
    assert_eq!(schemas["DeviceName"]["minLength"], 1);
    assert_eq!(schemas["DeviceName"]["maxLength"], 64);
    assert_eq!(
        schemas["DeviceNameSource"]["enum"],
        json!(["admin", "device"])
    );
    assert_eq!(
        schemas["Enrollment"]["properties"]["name_auto"]["default"],
        false
    );
    assert!(
        !schemas["Enrollment"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("name_auto"))
    );
    for field in ["name_source", "hostname"] {
        assert!(schemas["Device"]["properties"].get(field).is_some());
        assert!(
            schemas["Device"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
    }
    assert_eq!(
        schemas["DeviceUpdate"]["properties"]["use_system_name"]["type"],
        "boolean"
    );
    assert!(
        schemas["DeviceReportInput"]["properties"]
            .get("hostname")
            .is_some()
    );
    assert!(
        !schemas["DeviceReportInput"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("hostname"))
    );
    assert_eq!(
        schemas["DevicePeer"]["required"],
        json!(["device_id", "name"])
    );
    let peers = &doc["paths"]["/api/v1/device/peers"]["get"];
    assert_eq!(
        peers["security"],
        doc["paths"]["/api/v1/device/config"]["get"]["security"]
    );
    assert!(peers["security"].as_array().is_some_and(|v| !v.is_empty()));
    assert_eq!(
        peers["responses"]["200"]["content"]["application/json"]["schema"]["type"],
        "array"
    );
    assert_eq!(
        peers["responses"]["200"]["content"]["application/json"]["schema"]["items"]["$ref"],
        "#/components/schemas/DevicePeer"
    );
    for code in ["401", "403"] {
        assert!(peers["responses"].get(code).is_some());
    }
    assert!(
        doc["paths"]["/api/v1/networks/{net}/devices/{dev}"]["patch"]["responses"]
            .get("409")
            .is_some()
    );
}
