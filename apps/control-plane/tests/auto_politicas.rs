// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-031: quitar una conexión automática no toca la política manual coincidente, y la excepción es simétrica.
mod common;

use atenea_identity::{
    AgentState, DeviceConfig, LocalKeys, Selector, SigningKey, generate_seed, signing_key,
    verify_config,
};
use axum::http::StatusCode;
use common::{Device, Session, TestApp, now};
use serde_json::{Value, json};
use uuid::Uuid;

/// Dispositivo aprobado por la API; con el ajuste de la red activo, se conecta con los aprobados antes que él.
async fn aprobado(t: &TestApp, s: &Session, net: Uuid) -> Device {
    let d = Device::new();
    let token = t.token(s, net, false).await;
    assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
    let r = t.action(s, net, d.id, "approve").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    d
}

/// Pares automáticos de `d` según la API, ordenados.
async fn automaticos(t: &TestApp, s: &Session, net: Uuid, d: &Device) -> Vec<Uuid> {
    let r = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices/{}/auto-peers", d.id),
            Some(s),
            Value::Null,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let mut ids: Vec<Uuid> = r
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().parse::<Uuid>().unwrap())
        .collect();
    ids.sort();
    ids
}

/// Configuración firmada que recibe `d` ahora, verificada con sus claves.
async fn configuracion(t: &TestApp, d: &Device, net: Uuid, root: &SigningKey) -> DeviceConfig {
    let r = t.config(d, now(), *Uuid::new_v4().as_bytes()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let keys = LocalKeys {
        noise_pub: d.bundle.noise_pubkey,
        identity_pub: d.bundle.identity_pubkey,
    };
    let state = AgentState::new(
        *net.as_bytes(),
        *d.id.as_bytes(),
        root.verifying_key().to_bytes(),
    );
    verify_config(&r.body, &keys, &state, now()).unwrap().config
}

#[tokio::test]
async fn politica_manual_sobrevive_a_quitar_la_conexion_automatica_en_ambos_sentidos() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t
        .network_with(&s, org, &root, "10.44.0.0/16", "fd44::/64", true)
        .await;
    t.certificate(&s, net, &root).await;
    let a = aprobado(&t, &s, net).await;
    let c = aprobado(&t, &s, net).await;
    // Política manual C → A por TCP 443: independiente de la conexión automática entre ambos.
    let manual = t
        .json(
            "POST",
            &format!("/api/v1/networks/{net}/policies"),
            Some(&s),
            json!({"src":{"kind":"device","device_id":c.id},"dst":{"kind":"device","device_id":a.id},"proto":6,"port_min":443,"port_max":443}),
        )
        .await;
    assert_eq!(manual.status, StatusCode::CREATED, "{}", manual.json);
    assert_eq!(automaticos(&t, &s, net, &c).await, vec![a.id]);
    assert_eq!(automaticos(&t, &s, net, &a).await, vec![c.id]);
    // La excepción se pide desde C: el par deja de ser automático vista desde cualquiera de los dos.
    let quitar = t
        .json(
            "DELETE",
            &format!(
                "/api/v1/networks/{net}/devices/{}/auto-peers/{}",
                c.id, a.id
            ),
            Some(&s),
            Value::Null,
        )
        .await;
    assert_eq!(quitar.status, StatusCode::NO_CONTENT, "{}", quitar.json);
    assert!(automaticos(&t, &s, net, &c).await.is_empty());
    assert!(automaticos(&t, &s, net, &a).await.is_empty());
    // La política manual sigue: C alcanza a A por TCP 443 con su regla explícita y sin reglas automáticas.
    let cfg = configuracion(&t, &c, net, &root).await;
    assert_eq!(
        cfg.peers.iter().map(|p| p.device_id).collect::<Vec<_>>(),
        vec![*a.id.as_bytes()]
    );
    assert!(
        cfg.rules
            .iter()
            .any(|r| r.src == Selector::Device(*c.id.as_bytes())
                && r.dst == Selector::Device(*a.id.as_bytes())
                && r.proto == 6
                && r.port_min == 443
                && r.port_max == 443)
    );
    assert!(cfg.rules.iter().all(|r| r.proto != 0));
    // Repetir la excepción ya no encuentra una conexión automática que quitar.
    t.json(
        "DELETE",
        &format!(
            "/api/v1/networks/{net}/devices/{}/auto-peers/{}",
            c.id, a.id
        ),
        Some(&s),
        Value::Null,
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    t.finish().await;
}
