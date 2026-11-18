// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-029: redes gestionadas por el servidor (raíz cifrada, certificado automático, CIDR automáticos).
mod common;
use atenea_control_plane::{AppState, config::Config, managed};
use atenea_identity::{IssuerCert, generate_seed, signing_key};
use axum::http::StatusCode;
use common::{Session, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const KEY: [u8; 32] = [42; 32];

async fn create(t: &TestApp, s: &Session, org: Uuid, body: Value) -> common::Reply {
    t.json(
        "POST",
        &format!("/api/v1/orgs/{org}/networks"),
        Some(s),
        body,
    )
    .await
}

async fn cert(pool: &PgPool, net: Uuid) -> (IssuerCert, [u8; 32]) {
    let (bytes, anchor): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT issuer_cert, root_anchor FROM networks WHERE id=$1")
            .bind(net)
            .fetch_one(pool)
            .await
            .unwrap();
    (
        IssuerCert::parse(&bytes).unwrap(),
        anchor.try_into().unwrap(),
    )
}

#[tokio::test]
async fn una_red_con_solo_nombre_nace_gestionada_con_certificado_valido_y_rangos_propios() {
    let Some(t) = TestApp::with_wrap(Config::default(), KEY).await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let r = create(&t, &owner, org, json!({"name": "casa"})).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["root_mode"], "managed");
    assert_eq!(r.json["has_issuer_cert"], true);
    assert_eq!(r.json["ipv4_cidr"], "10.64.0.0/24");
    assert!(r.json["ipv6_cidr"].as_str().unwrap().starts_with("fd"));
    assert!(r.json["ipv6_cidr"].as_str().unwrap().ends_with("/64"));
    let net = common::id(&r.json);

    // El certificado lo firma la raíz de la red y es de la clave de emisor del servidor, con la validez acordada.
    let (c, anchor) = cert(&t.pool, net).await;
    c.verify(&anchor, *net.as_bytes(), common::now()).unwrap();
    assert_eq!(c.issuer_pubkey, t.issuer.verifying_key().to_bytes());
    assert_eq!(c.issuer_serial, 1);
    assert_eq!(c.not_after - c.not_before, 30 * 86_400 + 60);
    assert_eq!(hex::encode(anchor), r.json["root_anchor"].as_str().unwrap());

    // La raíz no está en claro: 24 de nonce + 32 de semilla + 16 de etiqueta.
    let wrapped: Vec<u8> = sqlx::query_scalar("SELECT root_wrapped FROM networks WHERE id=$1")
        .bind(net)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(wrapped.len(), 72);
    let audit: Value = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE action='network.issuer_cert' AND network_id=$1",
    )
    .bind(net)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(audit, json!({"serial": 1, "managed": true}));
    let kind: String = sqlx::query_scalar(
        "SELECT actor_kind FROM audit_events WHERE action='network.issuer_cert' AND network_id=$1",
    )
    .bind(net)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(kind, "system");

    // El servidor firma la configuración de un equipo de esa red sin que nadie haya subido nada.
    let device = common::Device::new();
    let token = t.token(&owner, net, true).await;
    assert_eq!(t.enroll(&token, &device).await.status, StatusCode::CREATED);
    let r = t.config(&device, common::now(), [1; 16]).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    t.finish().await;
}

#[tokio::test]
async fn los_rangos_automaticos_no_se_solapan_con_ninguna_red_existente() {
    let Some(t) = TestApp::with_wrap(Config::default(), KEY).await else {
        return;
    };
    let (a, b) = (t.user().await, t.user().await);
    let (org_a, org_b) = (t.org(&a).await, t.org(&b).await);
    // Otra cuenta ocupa 10.64.0.0/16 con raíz propia: el primer libre es 10.65.0.0/24.
    let own = create(
        &t,
        &b,
        org_b,
        json!({"name": "ajena", "ipv4_cidr": "10.64.0.0/16", "ipv6_cidr": "fd64::/64",
               "root_anchor": hex::encode(signing_key(generate_seed()).verifying_key().to_bytes())}),
    )
    .await;
    assert_eq!(own.status, StatusCode::CREATED, "{}", own.json);
    assert_eq!(own.json["root_mode"], "own");
    let first = create(&t, &a, org_a, json!({"name": "uno"})).await;
    let second = create(&t, &a, org_a, json!({"name": "dos"})).await;
    assert_eq!(first.json["ipv4_cidr"], "10.65.0.0/24", "{}", first.json);
    assert_eq!(second.json["ipv4_cidr"], "10.65.1.0/24", "{}", second.json);
    assert_ne!(first.json["ipv6_cidr"], second.json["ipv6_cidr"]);
    t.finish().await;
}

#[tokio::test]
async fn sin_clave_de_envoltura_no_se_crean_redes_gestionadas_pero_las_propias_siguen() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    create(&t, &owner, org, json!({"name": "casa"}))
        .await
        .error(StatusCode::CONFLICT, "managed_unavailable");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM networks")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "una creación rechazada no deja nada a medias");
    // Con ancla propia y sin CIDR, los rangos también se asignan solos.
    let r = create(
        &t,
        &owner,
        org,
        json!({"name": "propia", "root_anchor": hex::encode(signing_key(generate_seed()).verifying_key().to_bytes())}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["root_mode"], "own");
    assert_eq!(r.json["has_issuer_cert"], false);
    assert_eq!(r.json["ipv4_cidr"], "10.64.0.0/24");
    t.finish().await;
}

#[tokio::test]
async fn el_certificado_de_una_red_gestionada_no_se_sube_a_mano() {
    let Some(t) = TestApp::with_wrap(Config::default(), KEY).await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let net = common::id(&create(&t, &owner, org, json!({"name": "casa"})).await.json);
    let (c, _) = cert(&t.pool, net).await;
    let request = axum::http::Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/networks/{net}/issuer-cert"))
        .header("cookie", format!("atenea_session={}", owner.token))
        .header("x-csrf-token", &owner.csrf)
        .header("content-type", "application/octet-stream")
        .body(axum::body::Body::from(c.encode().to_vec()))
        .unwrap();
    t.send(request)
        .await
        .error(StatusCode::CONFLICT, "managed_network");
    t.finish().await;
}

#[tokio::test]
async fn la_renovacion_sube_el_serial_cuando_queda_un_tercio_y_no_toca_lo_vigente() {
    let Some(t) = TestApp::with_wrap(Config::default(), KEY).await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let net = common::id(&create(&t, &owner, org, json!({"name": "casa"})).await.json);
    let state = |key: [u8; 32]| {
        AppState::new(t.pool.clone(), Config::default(), t.issuer.clone())
            .with_wrap(Some(managed::Wrap::new(&key)))
    };

    // Recién firmado: nada que renovar.
    assert_eq!(managed::renew_due(&state(KEY)).await.unwrap(), 0);

    // Se deja el certificado instalado en su último tercio (serial 5, vigencia ya casi agotada). Solo se tocan
    // los campos de tiempo y de serial: la renovación lee el serial y las fechas del almacenado.
    let (current, anchor) = cert(&t.pool, net).await;
    let mut bytes = current.encode();
    bytes[26..34].copy_from_slice(&5u64.to_be_bytes());
    let now = common::now();
    bytes[34..42].copy_from_slice(&(now - 100).to_be_bytes());
    bytes[42..50].copy_from_slice(&(now + 10).to_be_bytes());
    sqlx::query("UPDATE networks SET issuer_cert=$1 WHERE id=$2")
        .bind(bytes.as_slice())
        .bind(net)
        .execute(&t.pool)
        .await
        .unwrap();

    // Con otra clave de envoltura no descifra: no renueva y no borra nada.
    assert_eq!(managed::renew_due(&state([9; 32])).await.unwrap(), 0);
    let (unchanged, _) = cert(&t.pool, net).await;
    assert_eq!(unchanged.issuer_serial, 5);

    assert_eq!(managed::renew_due(&state(KEY)).await.unwrap(), 1);
    let (renewed, _) = cert(&t.pool, net).await;
    assert_eq!(renewed.issuer_serial, 6);
    renewed
        .verify(&anchor, *net.as_bytes(), common::now())
        .unwrap();
    assert!(renewed.not_after > now + 29 * 86_400);
    assert_eq!(managed::renew_due(&state(KEY)).await.unwrap(), 0);
    t.finish().await;
}

#[tokio::test]
async fn una_red_gestionada_sin_certificado_lo_recibe_en_el_siguiente_barrido() {
    let Some(t) = TestApp::with_wrap(Config::default(), KEY).await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let net = common::id(&create(&t, &owner, org, json!({"name": "casa"})).await.json);
    sqlx::query("UPDATE networks SET issuer_cert=NULL WHERE id=$1")
        .bind(net)
        .execute(&t.pool)
        .await
        .unwrap();
    let state = AppState::new(t.pool.clone(), Config::default(), t.issuer.clone())
        .with_wrap(Some(managed::Wrap::new(&KEY)));
    assert_eq!(managed::renew_due(&state).await.unwrap(), 1);
    let (c, anchor) = cert(&t.pool, net).await;
    assert_eq!(c.issuer_serial, 1);
    c.verify(&anchor, *net.as_bytes(), common::now()).unwrap();
    t.finish().await;
}
