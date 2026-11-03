// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-018 fase 5: lectura interna de las raíces de las redes para el coordinator. Va en un router y un listener
//! distintos del público, con secreto Bearer; el router público no la sirve nunca.
mod common;

use atenea_control_plane::{AppState, config::Config, internal};
use atenea_identity::{generate_seed, signing_key};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use common::TestApp;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use tower::ServiceExt;

const SECRET: &str = "secreto-interno-de-prueba-0123456789";
const PATH: &str = "/internal/v1/root-anchors";

async fn get(app: &axum::Router, auth: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().uri(PATH);
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let r = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let body = to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn root_hex(seed: [u8; 32]) -> String {
    hex::encode(signing_key(seed).verifying_key().to_bytes())
}

#[tokio::test]
async fn lista_las_raices_de_las_redes_solo_con_el_secreto() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let internal = internal::router(
        AppState::new(t.pool.clone(), Config::default(), t.issuer.clone()),
        SECRET,
    );
    // Sin redes: lista vacía (el coordinator conserva sus anclas estáticas).
    let ok = format!("Bearer {SECRET}");
    assert_eq!(
        get(&internal, Some(&ok)).await,
        (StatusCode::OK, serde_json::json!({"anchors": []}))
    );
    let s = t.user().await;
    let org = t.org(&s).await;
    let (a, b) = (generate_seed(), generate_seed());
    t.network(&s, org, &signing_key(a), "10.77.0.0/24", "fd77::/64")
        .await;
    t.network(&s, org, &signing_key(b), "10.78.0.0/24", "fd78::/64")
        .await;
    let (status, body) = get(&internal, Some(&ok)).await;
    assert_eq!(status, StatusCode::OK);
    let mut want = vec![root_hex(a), root_hex(b)];
    want.sort();
    assert_eq!(body["anchors"], serde_json::json!(want));
    // Sin cabecera, con otro secreto, con otro esquema o con el secreto sin «Bearer»: 401 sin datos.
    for auth in [
        None,
        Some("Bearer otro-secreto-distinto-0123456789"),
        Some(&format!("Basic {SECRET}")[..]),
        Some(SECRET),
        Some("Bearer "),
    ] {
        let (status, body) = get(&internal, auth).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{auth:?}");
        assert!(body.get("anchors").is_none(), "{auth:?}: {body}");
    }
    t.finish().await;
}

#[tokio::test]
async fn el_router_publico_no_sirve_la_ruta_interna() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    // Aun con un secreto correcto: la ruta no existe en el listener público.
    let (status, _) = get(&t.app, Some(&format!("Bearer {SECRET}"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    t.finish().await;
}

fn secret_file(dir: &std::path::Path, text: &str, mode: u32) -> std::path::PathBuf {
    let path = dir.join("anchors-token");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[test]
fn la_configuracion_interna_es_todo_o_nada_y_exige_un_secreto_decente() {
    let dir = std::env::temp_dir().join(format!("atenea-cp-internal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let load = |listen: Option<&str>, file: Option<std::path::PathBuf>| {
        internal::Access::load(&Config {
            internal_listen: listen.map(str::to_string),
            internal_secret_file: file,
            ..Config::default()
        })
    };
    // Apagado por defecto.
    assert!(load(None, None).unwrap().is_none());
    let good = secret_file(&dir, &format!("{SECRET}\n"), 0o400);
    let access = load(Some("127.0.0.1:0"), Some(good.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(access.secret, SECRET);
    // Una mitad sin la otra impide arrancar.
    assert!(load(Some("127.0.0.1:0"), None).is_err());
    assert!(load(None, Some(good.clone())).is_err());
    // Dirección inválida, fichero ausente, permisos laxos, secreto corto o con espacios.
    assert!(load(Some("no-es-direccion"), Some(good.clone())).is_err());
    assert!(load(Some("127.0.0.1:0"), Some(dir.join("no-existe"))).is_err());
    for mode in [0o440, 0o644, 0o640, 0o700] {
        let bad = secret_file(&dir, SECRET, mode);
        assert!(load(Some("127.0.0.1:0"), Some(bad)).is_err(), "{mode:o}");
    }
    for text in ["corto", "con espacios dentro del secreto 1234567890"] {
        let bad = secret_file(&dir, text, 0o600);
        assert!(load(Some("127.0.0.1:0"), Some(bad)).is_err(), "{text}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
