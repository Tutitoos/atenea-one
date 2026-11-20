// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-031, adenda (migración 0020): una aprobación escrita por un binario anterior, que solo hace
//! `UPDATE devices SET status='approved'`, recibe igualmente el orden de aprobación y el ajuste vigente.
mod common;

use atenea_identity::{generate_seed, signing_key};
use axum::http::StatusCode;
use common::{Device, Session, TestApp};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

async fn network(t: &TestApp, s: &Session, auto: bool) -> Uuid {
    let org = t.org(s).await;
    let root = signing_key(generate_seed());
    t.network_with(s, org, &root, "10.91.0.0/24", "fd91::/64", auto)
        .await
}

/// Dispositivo inscrito y pendiente.
async fn pending(t: &TestApp, s: &Session, net: Uuid) -> Device {
    let d = Device::new();
    let token = t.token(s, net, false).await;
    assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
    d
}

async fn approve(t: &TestApp, s: &Session, net: Uuid, d: &Device) {
    let r = t.action(s, net, d.id, "approve").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
}

async fn set_auto(t: &TestApp, s: &Session, net: Uuid, on: bool) {
    let r = t
        .json(
            "PATCH",
            &format!("/api/v1/networks/{net}"),
            Some(s),
            json!({"auto_connect": on}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
}

/// Exactamente lo que hace el control plane de la 0.1.35 al aprobar: sin orden ni ajuste.
async fn approve_like_the_previous_binary(t: &TestApp, d: &Device) {
    sqlx::query("UPDATE devices SET status='approved',approved_at=now() WHERE id=$1")
        .bind(d.id)
        .execute(&t.pool)
        .await
        .unwrap();
}

async fn order(t: &TestApp, d: &Device) -> (i64, bool) {
    let row =
        sqlx::query("SELECT approval_order,auto_connect_on_approval FROM devices WHERE id=$1")
            .bind(d.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    (
        row.get("approval_order"),
        row.get("auto_connect_on_approval"),
    )
}

async fn auto_peers(t: &TestApp, s: &Session, net: Uuid, d: &Device) -> Vec<Uuid> {
    let r = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices/{}/auto-peers", d.id),
            Some(s),
            Value::Null,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let mut peers: Vec<Uuid> = r
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().parse().unwrap())
        .collect();
    peers.sort();
    peers
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

#[tokio::test]
async fn la_aprobacion_del_binario_anterior_recibe_orden_y_el_ajuste_vigente() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true).await;
    let (a, b) = (pending(&t, &s, net).await, pending(&t, &s, net).await);
    approve(&t, &s, net, &a).await;
    approve(&t, &s, net, &b).await;
    assert_eq!(
        (order(&t, &a).await, order(&t, &b).await),
        ((1, true), (2, true))
    );

    // Servidor anterior con el ajuste apagado: d queda aprobado, pero sin conexiones automáticas.
    set_auto(&t, &s, net, false).await;
    let d = pending(&t, &s, net).await;
    approve_like_the_previous_binary(&t, &d).await;
    assert_eq!(
        order(&t, &d).await,
        (3, false),
        "orden siguiente y ajuste apagado"
    );
    assert_eq!(auto_peers(&t, &s, net, &d).await, Vec::<Uuid>::new());
    assert_eq!(auto_peers(&t, &s, net, &a).await, vec![b.id]);

    // Con el ajuste activo, e se conecta con todos los aprobados antes, y solo con ellos.
    set_auto(&t, &s, net, true).await;
    let e = pending(&t, &s, net).await;
    approve_like_the_previous_binary(&t, &e).await;
    assert_eq!(order(&t, &e).await, (4, true));
    assert_eq!(
        auto_peers(&t, &s, net, &e).await,
        sorted(vec![a.id, b.id, d.id])
    );
    assert_eq!(
        auto_peers(&t, &s, net, &d).await,
        vec![e.id],
        "d, aprobado con el ajuste apagado, solo se conecta con quien se aprobó después con él activo"
    );
    t.finish().await;
}

#[tokio::test]
async fn el_disparador_no_pisa_un_orden_ya_fijado_ni_otros_cambios() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true).await;
    let a = pending(&t, &s, net).await;
    let b = pending(&t, &s, net).await;
    // El código actual fija el orden él mismo: dos aprobaciones por la API siguen numerándose 1 y 2.
    approve(&t, &s, net, &a).await;
    approve(&t, &s, net, &b).await;
    assert_eq!((order(&t, &a).await.0, order(&t, &b).await.0), (1, 2));

    // Un orden ya asignado no se reasigna al volver a aprobar (revocado y aprobado de nuevo).
    sqlx::query("UPDATE devices SET status='revoked' WHERE id=$1")
        .bind(a.id)
        .execute(&t.pool)
        .await
        .unwrap();
    approve_like_the_previous_binary(&t, &a).await;
    assert_eq!(order(&t, &a).await, (1, true));

    // Un aprobado anterior a la migración (orden 0, insertado ya aprobado) conserva su orden al cambiar otros campos.
    let legacy = Device::new();
    sqlx::query("INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,approved_at) VALUES($1,$2,'antiguo','approved',$3,$4,$5,now())")
        .bind(legacy.id)
        .bind(net)
        .bind(legacy.bundle.noise_pubkey.as_slice())
        .bind(legacy.bundle.identity_pubkey.as_slice())
        .bind(legacy.bundle.encode().to_vec())
        .execute(&t.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE devices SET name='renombrado' WHERE id=$1")
        .bind(legacy.id)
        .execute(&t.pool)
        .await
        .unwrap();
    assert_eq!(order(&t, &legacy).await, (0, false));
    t.finish().await;
}

#[tokio::test]
async fn aprobaciones_concurrentes_del_binario_anterior_se_numeran_sin_repetir() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true).await;
    let mut devices = Vec::new();
    for _ in 0..12 {
        devices.push(pending(&t, &s, net).await);
    }
    let t_ref = &t;
    futures_util::future::join_all(devices.iter().map(|d| async move {
        // Cada aprobación en su propia transacción y conexión, como las peticiones concurrentes de un servidor.
        let mut tx = t_ref.pool.begin().await.unwrap();
        sqlx::query("UPDATE devices SET status='approved',approved_at=now() WHERE id=$1")
            .bind(d.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }))
    .await;
    let mut orders: Vec<i64> = sqlx::query_scalar(
        "SELECT approval_order FROM devices WHERE network_id=$1 ORDER BY approval_order",
    )
    .bind(net)
    .fetch_all(&t.pool)
    .await
    .unwrap();
    orders.sort_unstable();
    assert_eq!(orders, (1..=12).collect::<Vec<i64>>());
    t.finish().await;
}
