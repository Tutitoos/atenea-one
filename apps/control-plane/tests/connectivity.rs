// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-018 fase 2: el código de unión lleva el STUN y el hash de la CA del relay, y la CA se sirve en
//! `GET /api/v1/relay-ca`. Sin configuración el servidor se comporta como la v0.1.8.
mod common;

use atenea_control_plane::{
    config::Config,
    connectivity::{Connectivity, RelayCa},
};
use atenea_identity::{generate_seed, join_code::JoinCode, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{Reply, Session, TestApp};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const URL: &str = "https://control.example.test";
const STUN: &str = "relay.example.test";

/// Certificado SINTÉTICO (no es X.509 real): el control plane solo exige una secuencia DER íntegra y calcula
/// su SHA-256. Las pruebas con la CA real del relay son del laboratorio de la fase 7.
fn fake_ca_der() -> Vec<u8> {
    let mut der = vec![0x30, 10, 0x02, 0x01, 0x01, 0x04, 0x05];
    der.extend_from_slice(b"ca-ok");
    der
}

fn pem_of(der: &[u8]) -> String {
    let body = STANDARD.encode(der);
    format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n")
}

fn der_of(pem: &str) -> Vec<u8> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    STANDARD.decode(body).unwrap()
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn config() -> Config {
    Config {
        enroll_per_minute: 10000,
        public_url: URL.into(),
        ..Config::default()
    }
}

fn with_ca() -> Connectivity {
    Connectivity::new(
        STUN,
        Some(RelayCa::from_pem(&pem_of(&fake_ca_der())).unwrap()),
    )
    .unwrap()
}

struct Lab {
    t: TestApp,
    owner: Session,
    net: Uuid,
}

async fn lab(config: Config, connectivity: Option<Connectivity>) -> Option<Lab> {
    let t = match connectivity {
        Some(c) => TestApp::with_connectivity(config, c).await?,
        None => TestApp::configured(config).await?,
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.77.0.0/24", "fd77::/64")
        .await;
    Some(Lab { t, owner, net })
}

impl Lab {
    async fn code(&self, query: &str) -> Reply {
        self.t
            .json(
                "GET",
                &format!("/api/v1/networks/{}/join-code{query}", self.net),
                Some(&self.owner),
                Value::Null,
            )
            .await
    }

    async fn token(&self, query: &str) -> Reply {
        self.t
            .json(
                "POST",
                &format!("/api/v1/networks/{}/enrollment-tokens{query}", self.net),
                Some(&self.owner),
                json!({"ttl_secs":600}),
            )
            .await
    }

    async fn relay_ca(&self) -> Reply {
        // Sin sesión ni CSRF: lo descarga un cliente que todavía no está unido a la red.
        self.t
            .send(
                Request::builder()
                    .uri("/api/v1/relay-ca")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
    }
}

fn decoded(r: &Reply) -> JoinCode {
    JoinCode::decode(r.json["code"].as_str().expect("código")).unwrap()
}

#[tokio::test]
async fn sin_configurar_el_codigo_no_lleva_bloque_y_no_hay_ca() {
    let Some(l) = lab(config(), None).await else {
        return;
    };
    let r = l.code("").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["connectivity"], Value::Null);
    assert_eq!(decoded(&r).connectivity, None);
    l.relay_ca().await.error(StatusCode::NOT_FOUND, "not_found");
    let t = l.token("").await;
    assert_eq!(t.status, StatusCode::CREATED, "{}", t.json);
    assert_eq!(t.json["connectivity"], Value::Null);
    assert_eq!(decoded(&t).connectivity, None);
    l.t.finish().await;
}

#[tokio::test]
async fn configurado_el_codigo_lleva_el_stun_y_el_hash_de_la_ca() {
    let Some(l) = lab(config(), Some(with_ca())).await else {
        return;
    };
    let der = fake_ca_der();
    let r = l.code("").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let conn = decoded(&r).connectivity.expect("bloque de conectividad");
    assert_eq!(conn.stun_host, STUN);
    assert_eq!(conn.relay_ca_sha256, Some(sha(&der)));
    assert_eq!(
        r.json["connectivity"],
        json!({"stun_host": STUN, "relay_ca_sha256": hex::encode(sha(&der))})
    );

    // La CA servida es la que anuncia el hash del código, sin sesión y con caché corta.
    let ca = l.relay_ca().await;
    assert_eq!(ca.status, StatusCode::OK);
    assert_eq!(
        ca.headers["content-type"].to_str().unwrap(),
        "application/x-pem-file"
    );
    assert_eq!(
        ca.headers["cache-control"].to_str().unwrap(),
        "public, max-age=300"
    );
    let pem = String::from_utf8(ca.body).unwrap();
    assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
    assert!(pem.ends_with("-----END CERTIFICATE-----\n"));
    assert_eq!(sha(&der_of(&pem)), conn.relay_ca_sha256.unwrap());
    l.t.finish().await;
}

#[tokio::test]
async fn solo_stun_sin_ca_no_lleva_hash_y_no_sirve_ca() {
    let c = Connectivity::new(STUN, None).unwrap();
    let Some(l) = lab(config(), Some(c)).await else {
        return;
    };
    let r = l.code("").await;
    let conn = decoded(&r).connectivity.expect("bloque");
    assert_eq!(conn.stun_host, STUN);
    assert_eq!(conn.relay_ca_sha256, None);
    assert_eq!(
        r.json["connectivity"],
        json!({"stun_host": STUN, "relay_ca_sha256": null})
    );
    l.relay_ca().await.error(StatusCode::NOT_FOUND, "not_found");
    l.t.finish().await;
}

#[tokio::test]
async fn el_token_conserva_su_semantica_y_lleva_el_bloque() {
    let Some(l) = lab(config(), Some(with_ca())).await else {
        return;
    };
    let t = l.token("").await;
    assert_eq!(t.status, StatusCode::CREATED, "{}", t.json);
    let code = decoded(&t);
    assert_eq!(code.token.as_deref(), t.json["token"].as_str());
    assert_eq!(code.connectivity.expect("bloque").stun_host, STUN);
    assert_eq!(t.json["connectivity"]["stun_host"], STUN);
    // El token sigue siendo de un solo uso y el enrolamiento no cambia.
    assert!(t.json["join"]["token"].is_string());
    l.t.finish().await;
}

#[tokio::test]
async fn conectividad_0_devuelve_el_codigo_sin_bloque_para_clientes_antiguos() {
    let Some(l) = lab(config(), Some(with_ca())).await else {
        return;
    };
    let r = l.code("?conectividad=0").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["connectivity"], Value::Null);
    assert_eq!(decoded(&r).connectivity, None);
    let t = l.token("?conectividad=0").await;
    assert_eq!(t.status, StatusCode::CREATED, "{}", t.json);
    assert_eq!(t.json["connectivity"], Value::Null);
    let code = decoded(&t);
    assert_eq!(code.connectivity, None);
    assert!(code.token.is_some());
    // `1` es el valor por omisión; cualquier otro es una petición inválida.
    assert!(
        decoded(&l.code("?conectividad=1").await)
            .connectivity
            .is_some()
    );
    for bad in ["?conectividad=2", "?conectividad=si", "?conectividad="] {
        l.code(bad)
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    l.t.finish().await;
}

#[tokio::test]
async fn si_el_bloque_no_cabe_el_codigo_es_409_y_sin_bloque_funciona() {
    let mut cfg = config();
    cfg.public_url = format!("https://{}", "a".repeat(92));
    let c = Connectivity::new(
        &"a".repeat(100),
        Some(RelayCa::from_pem(&pem_of(&fake_ca_der())).unwrap()),
    )
    .unwrap();
    let Some(l) = lab(cfg, Some(c)).await else {
        return;
    };
    l.code("")
        .await
        .error(StatusCode::CONFLICT, "code_too_long");
    let sin = l.code("?conectividad=0").await;
    assert_eq!(sin.status, StatusCode::OK, "{}", sin.json);
    assert_eq!(decoded(&sin).connectivity, None);
    // El token sigue creándose; su `code` es null si no cabe (como ya ocurría con un nombre largo).
    let t = l.token("").await;
    assert_eq!(t.status, StatusCode::CREATED, "{}", t.json);
    assert_eq!(t.json["code"], Value::Null);
    let t = l.token("?conectividad=0").await;
    assert!(t.json["code"].is_string());
    l.t.finish().await;
}
