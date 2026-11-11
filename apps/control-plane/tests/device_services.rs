// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-024: servicios publicados por dispositivo (`/device/status`, `/device/peers`, inventario).
mod common;

use atenea_identity::{generate_seed, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Reply, TestApp, now};
use serde_json::{Value, json};
use uuid::Uuid;

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

/// Informe con el cuerpo dado tal cual; la cuota de 5 s no es el comportamiento bajo prueba.
async fn report(t: &TestApp, device: &Device, body: Value) -> Reply {
    sqlx::query(
        "UPDATE device_reports SET reported_at=now()-interval '6 seconds' WHERE device_id=$1",
    )
    .bind(device.id)
    .execute(&t.pool)
    .await
    .unwrap();
    signed(t, device, "POST", "/api/v1/device/status", Some(body)).await
}

async fn services_report(t: &TestApp, device: &Device, services: Value) -> Reply {
    report(t, device, json!({"paths":[],"services":services})).await
}

async fn peers(t: &TestApp, device: &Device) -> Value {
    let reply = signed(t, device, "GET", "/api/v1/device/peers", None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    reply.json
}

async fn stored(t: &TestApp, device: &Device) -> Value {
    sqlx::query_scalar("SELECT services FROM devices WHERE id=$1")
        .bind(device.id)
        .fetch_one(&t.pool)
        .await
        .unwrap()
}

async fn age_services(t: &TestApp, device: &Device, secs: i32) {
    sqlx::query("UPDATE devices SET services_at=now()-make_interval(secs=>$2) WHERE id=$1")
        .bind(device.id)
        .bind(f64::from(secs))
        .execute(&t.pool)
        .await
        .unwrap();
}

struct Setup {
    t: TestApp,
    user: common::Session,
    net: Uuid,
    /// Publica servicios; tiene política hacia `viewer`.
    publisher: Device,
    viewer: Device,
    /// Sin política con `publisher`.
    outsider: Device,
}

async fn setup() -> Option<Setup> {
    let t = TestApp::new().await?;
    let user = t.user().await;
    let org = t.org(&user).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&user, org, &root, "10.83.0.0/24", "fd83::/64")
        .await;
    let (publisher, viewer, outsider) = (Device::new(), Device::new(), Device::new());
    for device in [&publisher, &viewer, &outsider] {
        let token = t.token(&user, net, true).await;
        let r = t
            .json(
                "POST",
                "/api/v1/enroll",
                None,
                json!({"token":token,"bundle":hex::encode(device.bundle.encode()),"name":"Par"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    }
    t.certificate(&user, net, &root).await;
    let policy = t
        .json(
            "POST",
            &format!("/api/v1/networks/{net}/policies"),
            Some(&user),
            json!({"src":{"kind":"device","device_id":publisher.id},"dst":{"kind":"device","device_id":viewer.id},"proto":6,"port_min":443,"port_max":443}),
        )
        .await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.json);
    Some(Setup {
        t,
        user,
        net,
        publisher,
        viewer,
        outsider,
    })
}

#[tokio::test]
async fn servicios_solo_los_ve_un_par_con_politica_y_dentro_de_120_segundos() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        user,
        net,
        publisher,
        viewer,
        outsider,
    } = &s;
    // Sin informe de servicios: el par sale con lista vacía.
    assert_eq!(
        peers(t, viewer).await,
        json!([{"device_id":publisher.id,"name":"Par","role":"device","services":[]}])
    );
    let reply = services_report(
        t,
        publisher,
        json!([
            {"port":6006,"proto":"tcp","label":"  Storybook  "},
            {"port":8443,"proto":"tcp","scheme":"https"}
        ]),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    let expected = json!([
        {"port":6006,"proto":"tcp","scheme":"http","label":"Storybook"},
        {"port":8443,"proto":"tcp","scheme":"https"}
    ]);
    assert_eq!(peers(t, viewer).await[0]["services"], expected);
    // Un par sin política con el publicador no lo ve y por tanto no ve sus servicios.
    assert_eq!(peers(t, outsider).await, json!([]));
    // 119 s: todavía vigente; 121 s: se omite de /device/peers pero sigue en el inventario.
    age_services(t, publisher, 119).await;
    assert_eq!(peers(t, viewer).await[0]["services"], expected);
    age_services(t, publisher, 121).await;
    assert_eq!(peers(t, viewer).await[0]["services"], json!([]));
    let inv = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices"),
            Some(user),
            Value::Null,
        )
        .await;
    assert_eq!(inv.status, StatusCode::OK, "{}", inv.json);
    let item = |id: Uuid| {
        inv.json
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id.to_string())
            .unwrap()
            .clone()
    };
    assert_eq!(item(publisher.id)["services"], expected);
    assert!(item(publisher.id)["services_at"].is_string());
    assert_eq!(item(viewer.id)["services"], json!([]));
    assert!(item(viewer.id)["services_at"].is_null());
    s.t.finish().await;
}

#[tokio::test]
async fn servicios_se_sustituyen_la_ausencia_no_toca_y_el_agente_antiguo_sigue_valido() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        publisher,
        viewer,
        ..
    } = &s;
    // Informe antiguo, sin el campo.
    let old = report(t, publisher, json!({"paths":[],"hostname":"equipo-a"})).await;
    assert_eq!(old.status, StatusCode::NO_CONTENT, "{}", old.json);
    let at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT services_at FROM devices WHERE id=$1")
            .bind(publisher.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert!(at.is_none());
    assert_eq!(stored(t, publisher).await, json!([]));
    services_report(t, publisher, json!([{"port":80,"proto":"tcp"}])).await;
    assert_eq!(
        peers(t, viewer).await[0]["services"],
        json!([{"port":80,"proto":"tcp","scheme":"http"}])
    );
    // Ausente (y null) no cambia lo guardado ni la marca de tiempo.
    age_services(t, publisher, 30).await;
    for body in [json!({"paths":[]}), json!({"paths":[],"services":null})] {
        assert_eq!(
            report(t, publisher, body).await.status,
            StatusCode::NO_CONTENT
        );
    }
    let age: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM now()-services_at)::float8 FROM devices WHERE id=$1",
    )
    .bind(publisher.id)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert!((29.0..60.0).contains(&age), "{age}");
    assert_eq!(
        stored(t, publisher).await,
        json!([{"port":80,"proto":"tcp","scheme":"http"}])
    );
    // Sustitución total.
    services_report(
        t,
        publisher,
        json!([{"port":3000,"proto":"tcp","scheme":"https"}]),
    )
    .await;
    assert_eq!(
        peers(t, viewer).await[0]["services"],
        json!([{"port":3000,"proto":"tcp","scheme":"https"}])
    );
    // Lista vacía: vacía lo guardado y renueva la marca.
    age_services(t, publisher, 100).await;
    let reply = services_report(t, publisher, json!([])).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    assert_eq!(stored(t, publisher).await, json!([]));
    let age: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM now()-services_at)::float8 FROM devices WHERE id=$1",
    )
    .bind(publisher.id)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert!(age < 30.0, "{age}");
    assert_eq!(peers(t, viewer).await[0]["services"], json!([]));
    s.t.finish().await;
}

#[tokio::test]
async fn servicios_validacion_estricta_y_sin_efectos_si_falla() {
    let Some(s) = setup().await else { return };
    let Setup { t, publisher, .. } = &s;
    let ok = json!([{"port":80,"proto":"tcp"}]);
    assert_eq!(
        services_report(t, publisher, ok.clone()).await.status,
        StatusCode::NO_CONTENT
    );
    let many: Vec<Value> = (1..=65).map(|p| json!({"port":p,"proto":"tcp"})).collect();
    let max: Vec<Value> = (1..=64).map(|p| json!({"port":p,"proto":"tcp"})).collect();
    let cases = [
        // Semántica: invalid_request.
        (json!(many), "invalid_request"),
        (json!([{"port":0,"proto":"tcp"}]), "invalid_request"),
        (
            json!([{"port":80,"proto":"tcp"},{"port":80,"proto":"tcp","scheme":"https"}]),
            "invalid_request",
        ),
        (
            json!([{"port":80,"proto":"tcp","label":"a".repeat(33)}]),
            "invalid_request",
        ),
        (
            json!([{"port":80,"proto":"tcp","label":"mal\u{202e}texto"}]),
            "invalid_request",
        ),
        (
            json!([{"port":80,"proto":"tcp","label":"con\ncontrol"}]),
            "invalid_request",
        ),
        (
            json!([{"port":80,"proto":"tcp","label":"   "}]),
            "invalid_request",
        ),
        // Forma: la deserialización estricta del informe responde invalid_report.
        (json!([{"port":80,"proto":"udp"}]), "invalid_report"),
        (json!([{"port":80}]), "invalid_report"),
        (
            json!([{"port":80,"proto":"tcp","scheme":"ftp"}]),
            "invalid_report",
        ),
        (json!([{"port":65536,"proto":"tcp"}]), "invalid_report"),
        (json!([{"port":-1,"proto":"tcp"}]), "invalid_report"),
        (
            json!([{"port":80,"proto":"tcp","process":"nginx"}]),
            "invalid_report",
        ),
        (json!({"port":80,"proto":"tcp"}), "invalid_report"),
    ];
    for (services, code) in cases {
        services_report(t, publisher, services)
            .await
            .error(StatusCode::BAD_REQUEST, code);
    }
    // Un rechazo no toca lo guardado.
    assert_eq!(
        stored(t, publisher).await,
        json!([{"port":80,"proto":"tcp","scheme":"http"}])
    );
    // Límites exactos aceptados: 64 servicios y etiqueta de 32 caracteres.
    let reply = services_report(t, publisher, json!(max)).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    let label = "ñ".repeat(32);
    let reply = services_report(
        t,
        publisher,
        json!([{"port":80,"proto":"tcp","label":label}]),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    assert_eq!(stored(t, publisher).await[0]["label"], label);
    s.t.finish().await;
}

#[test]
fn openapi_describe_servicios() {
    let doc = serde_json::to_value(atenea_control_plane::openapi()).unwrap();
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["DevicePeer"]["required"],
        json!(["device_id", "name", "role", "services"])
    );
    assert_eq!(
        schemas["DeviceService"]["required"],
        json!(["port", "proto", "scheme"])
    );
    assert_eq!(
        schemas["DeviceServiceInput"]["required"],
        json!(["port", "proto"])
    );
    assert_eq!(schemas["DeviceServiceLabel"]["maxLength"], 32);
    assert!(
        !schemas["DeviceReportInput"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("services"))
    );
}
