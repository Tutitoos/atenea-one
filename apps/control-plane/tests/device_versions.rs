// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-025: versiones instaladas por dispositivo (`/device/status`, `/device/peers`, inventario).
mod common;

use atenea_identity::{generate_seed, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Reply, TestApp, now};
use serde_json::{Value, json};
use uuid::Uuid;

const COMMIT: &str = "dfe72aefc123edc288d1daead1c812825e0a2a00";

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

async fn versions_report(t: &TestApp, device: &Device, versions: Value) -> Reply {
    report(t, device, json!({"paths":[],"versions":versions})).await
}

async fn peers(t: &TestApp, device: &Device) -> Value {
    let reply = signed(t, device, "GET", "/api/v1/device/peers", None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    reply.json
}

async fn stored(t: &TestApp, device: &Device) -> Value {
    sqlx::query_scalar("SELECT versions FROM devices WHERE id=$1")
        .bind(device.id)
        .fetch_one(&t.pool)
        .await
        .unwrap()
}

async fn age_versions(t: &TestApp, device: &Device, secs: i64) {
    sqlx::query("UPDATE devices SET versions_at=now()-make_interval(secs=>$2) WHERE id=$1")
        .bind(device.id)
        .bind(secs as f64)
        .execute(&t.pool)
        .await
        .unwrap();
}

async fn versions_age_secs(t: &TestApp, device: &Device) -> f64 {
    sqlx::query_scalar(
        "SELECT extract(epoch FROM now()-versions_at)::float8 FROM devices WHERE id=$1",
    )
    .bind(device.id)
    .fetch_one(&t.pool)
    .await
    .unwrap()
}

struct Setup {
    t: TestApp,
    user: common::Session,
    net: Uuid,
    /// Informa versiones; tiene política hacia `viewer`.
    reporter: Device,
    viewer: Device,
    /// Sin política con `reporter`.
    outsider: Device,
}

async fn setup() -> Option<Setup> {
    let t = TestApp::new().await?;
    let user = t.user().await;
    let org = t.org(&user).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&user, org, &root, "10.84.0.0/24", "fd84::/64")
        .await;
    let (reporter, viewer, outsider) = (Device::new(), Device::new(), Device::new());
    for device in [&reporter, &viewer, &outsider] {
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
            json!({"src":{"kind":"device","device_id":reporter.id},"dst":{"kind":"device","device_id":viewer.id},"proto":6,"port_min":443,"port_max":443}),
        )
        .await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.json);
    Some(Setup {
        t,
        user,
        net,
        reporter,
        viewer,
        outsider,
    })
}

#[tokio::test]
async fn versiones_las_ve_un_par_con_politica_sin_caducar_y_distinguen_sin_informar() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        user,
        net,
        reporter,
        viewer,
        outsider,
    } = &s;
    // Nunca informó: lista vacía y `versions_at` nulo («Sin informar»).
    assert_eq!(
        peers(t, viewer).await,
        json!([{"device_id":reporter.id,"name":"Par","role":"device","services":[],"versions":[],"versions_at":null}])
    );
    let list = json!([
        {"component":"agent","version":"0.1.31","commit":COMMIT},
        {"component":"cli","version":"0.1.31","commit":COMMIT}
    ]);
    let reply = versions_report(t, reporter, list.clone()).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    let seen = peers(t, viewer).await;
    assert_eq!(seen[0]["versions"], list);
    assert!(seen[0]["versions_at"].is_string(), "{seen}");
    // Un par sin política con quien informa ni lo ve ni ve sus versiones.
    assert_eq!(peers(t, outsider).await, json!([]));
    // A diferencia de los servicios, no caducan a los 120 s: se devuelve la última lista con su fecha.
    age_versions(t, reporter, 2 * 24 * 3600).await;
    let old = peers(t, viewer).await;
    assert_eq!(old[0]["versions"], list);
    assert!(old[0]["versions_at"].is_string());
    // El inventario de la organización las lleva también; quien no informó, vacío y nulo.
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
    assert_eq!(item(reporter.id)["versions"], list);
    assert!(item(reporter.id)["versions_at"].is_string());
    assert_eq!(item(viewer.id)["versions"], json!([]));
    assert!(item(viewer.id)["versions_at"].is_null());
    s.t.finish().await;
}

#[tokio::test]
async fn versiones_se_sustituyen_la_ausencia_no_toca_y_el_agente_antiguo_sigue_valido() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        reporter,
        viewer,
        ..
    } = &s;
    // Informe de un agente anterior (con servicios, sin versiones): válido y no crea ni toca versiones.
    let old = report(
        t,
        reporter,
        json!({"paths":[],"hostname":"equipo-a","services":[{"port":80,"proto":"tcp"}]}),
    )
    .await;
    assert_eq!(old.status, StatusCode::NO_CONTENT, "{}", old.json);
    let at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT versions_at FROM devices WHERE id=$1")
            .bind(reporter.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert!(at.is_none());
    assert_eq!(stored(t, reporter).await, json!([]));
    let first = json!([{"component":"agent","version":"0.1.30","commit":"unknown"}]);
    versions_report(t, reporter, first.clone()).await;
    assert_eq!(peers(t, viewer).await[0]["versions"], first);
    // Ausente y null no cambian lo guardado ni la marca de tiempo.
    age_versions(t, reporter, 30).await;
    for body in [json!({"paths":[]}), json!({"paths":[],"versions":null})] {
        assert_eq!(
            report(t, reporter, body).await.status,
            StatusCode::NO_CONTENT
        );
    }
    let age = versions_age_secs(t, reporter).await;
    assert!((29.0..60.0).contains(&age), "{age}");
    assert_eq!(stored(t, reporter).await, first);
    // Sustitución total (el componente que ya no está instalado desaparece).
    let second = json!([
        {"component":"agent","version":"0.1.31","commit":COMMIT},
        {"component":"desktop","version":"0.1.31-rc.1+b2","commit":"dfe72ae"}
    ]);
    versions_report(t, reporter, second.clone()).await;
    assert_eq!(peers(t, viewer).await[0]["versions"], second);
    // Lista vacía: «no hay nada instalado que informar»; vacía lo guardado y renueva la marca.
    age_versions(t, reporter, 100).await;
    let reply = versions_report(t, reporter, json!([])).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    assert_eq!(stored(t, reporter).await, json!([]));
    assert!(versions_age_secs(t, reporter).await < 30.0);
    let seen = peers(t, viewer).await;
    assert_eq!(seen[0]["versions"], json!([]));
    assert!(seen[0]["versions_at"].is_string());
    s.t.finish().await;
}

#[tokio::test]
async fn versiones_validacion_estricta_y_sin_efectos_si_falla() {
    let Some(s) = setup().await else { return };
    let Setup { t, reporter, .. } = &s;
    let ok = json!([{"component":"cli","version":"0.1.31","commit":COMMIT}]);
    assert_eq!(
        versions_report(t, reporter, ok.clone()).await.status,
        StatusCode::NO_CONTENT
    );
    let entry = |component: &str, version: &str, commit: &str| json!({"component":component,"version":version,"commit":commit});
    let cases = [
        // Más de un componente de cada tipo o más de tres.
        json!([entry("agent", "1", COMMIT), entry("agent", "2", COMMIT)]),
        json!([
            entry("agent", "1", COMMIT),
            entry("cli", "1", COMMIT),
            entry("desktop", "1", COMMIT),
            entry("agent", "1", COMMIT)
        ]),
        // Componente desconocido, vacío o con otra forma.
        json!([entry("relay", "1", COMMIT)]),
        json!([entry("Agent", "1", COMMIT)]),
        json!([entry("", "1", COMMIT)]),
        // Versión: vacía, con espacios o signos, o de más de 32 caracteres.
        json!([entry("agent", "", COMMIT)]),
        json!([entry("agent", "0.1 31", COMMIT)]),
        json!([entry("agent", "0.1.31;id", COMMIT)]),
        json!([entry("agent", "0.1.31\n", COMMIT)]),
        json!([entry("agent", "ñ.1", COMMIT)]),
        json!([entry("agent", &"1".repeat(33), COMMIT)]),
        // Commit: mayúsculas, demasiado corto o largo, no hexadecimal o un `unknown` mal escrito.
        json!([entry("agent", "1", "DFE72AEFC123")]),
        json!([entry("agent", "1", "dfe72a")]),
        json!([entry("agent", "1", &"a".repeat(41))]),
        json!([entry("agent", "1", "gggggggg")]),
        json!([entry("agent", "1", "Unknown")]),
        json!([entry("agent", "1", "")]),
        // Campos de más, de menos o de otro tipo.
        json!([{"component":"agent","version":"1","commit":COMMIT,"path":"/usr/bin/atenea"}]),
        json!([{"component":"agent","version":"1"}]),
        json!([{"component":"agent","version":1,"commit":COMMIT}]),
        json!({"component":"agent","version":"1","commit":COMMIT}),
    ];
    for versions in cases {
        report(
            t,
            reporter,
            json!({"paths":[],"hostname":"no-debe-guardarse","versions":versions}),
        )
        .await
        .error(StatusCode::BAD_REQUEST, "invalid_report");
    }
    // Un rechazo no toca nada del informe: ni las versiones ni el nombre del sistema que iba con él.
    assert_eq!(stored(t, reporter).await, ok);
    let hostname: Option<String> = sqlx::query_scalar("SELECT hostname FROM devices WHERE id=$1")
        .bind(reporter.id)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_ne!(hostname.as_deref(), Some("no-debe-guardarse"));
    // Límites exactos aceptados: tres componentes, versión de 32 caracteres y commit de 7 y de 40.
    let max = json!([
        entry("agent", &"1".repeat(32), "abcdef0"),
        entry("cli", "1.2.3-rc.1+b5", COMMIT),
        entry("desktop", "0.0.0-dev", "unknown")
    ]);
    let reply = versions_report(t, reporter, max.clone()).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.json);
    assert_eq!(stored(t, reporter).await, max);
    s.t.finish().await;
}

#[test]
fn openapi_describe_versiones() {
    let doc = serde_json::to_value(atenea_control_plane::openapi()).unwrap();
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["DeviceVersion"]["required"],
        json!(["component", "version", "commit"])
    );
    assert_eq!(
        schemas["DeviceVersion"]["properties"]["component"]["enum"],
        json!(["agent", "cli", "desktop"])
    );
    for schema in ["Device", "DevicePeer"] {
        let required = schemas[schema]["required"].as_array().unwrap();
        for field in ["versions", "versions_at"] {
            assert!(required.contains(&json!(field)), "{schema}.{field}");
        }
    }
    assert!(
        !schemas["DeviceReportInput"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("versions"))
    );
}
