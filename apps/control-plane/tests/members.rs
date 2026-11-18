// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-028 fase 2b: miembros por red, invitaciones por enlace y cuotas de las organizaciones personales.
mod common;
use atenea_control_plane::{accounts, config::Config};
use atenea_identity::{generate_seed, signing_key};
use axum::http::StatusCode;
use common::{Device, Reply, Session, TestApp};
use serde_json::json;
use uuid::Uuid;

struct World {
    t: TestApp,
    owner: Session,
    other: Session,
    org: Uuid,
    net: Uuid,
}

async fn world(config: Config) -> Option<World> {
    let t = TestApp::configured(Config {
        enroll_per_minute: 10_000,
        ..config
    })
    .await?;
    let owner = t.user().await;
    let other = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.60.0.0/24", "fd60::/64")
        .await;
    Some(World {
        t,
        owner,
        other,
        org,
        net,
    })
}

async fn call(w: &World, method: &str, path: &str, s: &Session, body: serde_json::Value) -> Reply {
    w.t.json(method, path, Some(s), body).await
}

/// El propietario invita con `role`; `s` canjea el enlace.
async fn join(w: &World, s: &Session, role: &str) -> String {
    let r = call(
        w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!({"role": role}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let token = r.json["token"].as_str().unwrap().to_owned();
    let r = call(
        w,
        "POST",
        "/api/v1/invitations/accept",
        s,
        json!({"token": token}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["role"], role);
    token
}

#[tokio::test]
async fn un_viewer_solo_lee_y_un_enroller_genera_codigos() {
    let Some(w) = world(Config::default()).await else {
        return;
    };
    let net = w.net;
    // Sin invitación la red es ajena.
    call(
        &w,
        "GET",
        &format!("/api/v1/networks/{net}"),
        &w.other,
        json!(null),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");

    join(&w, &w.other, "viewer").await;
    let shared = call(&w, "GET", "/api/v1/shared-networks", &w.other, json!(null)).await;
    assert_eq!(shared.json.as_array().unwrap().len(), 1);
    assert_eq!(shared.json[0]["role"], "viewer");
    assert_eq!(shared.json[0]["id"], net.to_string());
    // Invitar a una red no da una organización.
    let orgs = call(&w, "GET", "/api/v1/orgs", &w.other, json!(null)).await;
    assert_eq!(orgs.json.as_array().unwrap().len(), 0);

    for path in [
        format!("/api/v1/networks/{net}"),
        format!("/api/v1/networks/{net}/devices"),
        format!("/api/v1/networks/{net}/policies"),
        format!("/api/v1/networks/{net}/audit"),
    ] {
        let r = call(&w, "GET", &path, &w.other, json!(null)).await;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.json);
    }
    // Lo que exige admin u owner sigue cerrado, también el código de unión y la gestión de invitados.
    call(
        &w,
        "GET",
        &format!("/api/v1/networks/{net}/join-code"),
        &w.other,
        json!(null),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{net}/policies"),
        &w.other,
        json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":0}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");
    call(
        &w,
        "GET",
        &format!("/api/v1/networks/{net}/members"),
        &w.other,
        json!(null),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{net}/enrollment-tokens"),
        &w.other,
        json!({"ttl_secs": 600}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");

    // Ascendido a enroller, genera códigos pero sigue sin crear políticas ni invitar.
    let r = call(
        &w,
        "PUT",
        &format!("/api/v1/networks/{net}/members/{}", w.other.user),
        &w.owner,
        json!({"role": "enroller"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let r = call(
        &w,
        "POST",
        &format!("/api/v1/networks/{net}/enrollment-tokens"),
        &w.other,
        json!({"ttl_secs": 600}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{net}/policies"),
        &w.other,
        json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":0}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{net}/invitations"),
        &w.other,
        json!({"role": "viewer"}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");
    w.t.finish().await;
}

#[tokio::test]
async fn el_enroller_aprueba_solo_con_la_huella_completa() {
    let Some(w) = world(Config::default()).await else {
        return;
    };
    join(&w, &w.other, "enroller").await;
    let device = Device::new();
    let token = w.t.token(&w.owner, w.net, false).await;
    assert_eq!(
        w.t.enroll(&token, &device).await.status,
        StatusCode::CREATED
    );
    let approve = format!("/api/v1/networks/{}/devices/{}/approve", w.net, device.id);

    call(&w, "POST", &approve, &w.other, json!({}))
        .await
        .error(StatusCode::BAD_REQUEST, "invalid_request");
    call(
        &w,
        "POST",
        &approve,
        &w.other,
        json!({"fingerprint": "no es la huella"}),
    )
    .await
    .error(StatusCode::CONFLICT, "fingerprint_mismatch");
    let status: String = sqlx::query_scalar("SELECT status FROM devices WHERE id=$1")
        .bind(device.id)
        .fetch_one(&w.t.pool)
        .await
        .unwrap();
    assert_eq!(status, "pending");
    let r = call(
        &w,
        "POST",
        &approve,
        &w.other,
        json!({"fingerprint": device.bundle.fingerprint()}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let by: String = sqlx::query_scalar(
        "SELECT detail->>'by' FROM audit_events WHERE action='device.approve' AND target=$1",
    )
    .bind(device.id.to_string())
    .fetch_one(&w.t.pool)
    .await
    .unwrap();
    assert_eq!(by, "enroller");

    // Un viewer no aprueba ni con la huella; un administrador no la necesita.
    let viewer = w.t.user().await;
    join(&w, &viewer, "viewer").await;
    let second = Device::new();
    let token = w.t.token(&w.owner, w.net, false).await;
    assert_eq!(
        w.t.enroll(&token, &second).await.status,
        StatusCode::CREATED
    );
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/devices/{}/approve", w.net, second.id),
        &viewer,
        json!({"fingerprint": second.bundle.fingerprint()}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");
    let r = w.t.action(&w.owner, w.net, second.id, "approve").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    w.t.finish().await;
}

#[tokio::test]
async fn la_invitacion_es_de_un_solo_uso_caduca_y_se_anula() {
    let Some(w) = world(Config {
        max_open_invitations: 2,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    let token = join(&w, &w.other, "viewer").await;
    // Ya usada: igual que si no existiera.
    let third = w.t.user().await;
    call(
        &w,
        "POST",
        "/api/v1/invitations/accept",
        &third,
        json!({"token": token}),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    call(
        &w,
        "POST",
        "/api/v1/invitations/accept",
        &third,
        json!({"token": "atinv_inventado"}),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");

    // El listado no lleva el token y solo se guarda su hash.
    let list = call(
        &w,
        "GET",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!(null),
    )
    .await;
    assert_eq!(list.json.as_array().unwrap().len(), 1);
    assert!(list.json[0].get("token").is_none());
    assert!(list.json[0]["used_at"].is_string());
    let plain: i64 =
        sqlx::query_scalar("SELECT count(*) FROM network_invitations WHERE token_hash = $1")
            .bind(token.as_bytes())
            .fetch_one(&w.t.pool)
            .await
            .unwrap();
    assert_eq!(plain, 0);

    // Ya es miembro: no se consume la invitación nueva.
    let r = call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!({"role": "viewer"}),
    )
    .await;
    let fresh = r.json["token"].as_str().unwrap().to_owned();
    let id = r.json["id"].as_str().unwrap().to_owned();
    call(
        &w,
        "POST",
        "/api/v1/invitations/accept",
        &w.other,
        json!({"token": fresh}),
    )
    .await
    .error(StatusCode::CONFLICT, "already_member");
    // El propietario tampoco se invita a su propia red.
    call(
        &w,
        "POST",
        "/api/v1/invitations/accept",
        &w.owner,
        json!({"token": fresh}),
    )
    .await
    .error(StatusCode::CONFLICT, "already_member");

    // Cupo de vigentes: la usada no cuenta, la fresca sí.
    let r = call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!({"role": "viewer"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!({"role": "viewer"}),
    )
    .await
    .error(StatusCode::CONFLICT, "invitation_limit");
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!({"role": "owner"}),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_role");

    // Anulada, ya no vale; una caducada tampoco.
    let revoke = format!("/api/v1/networks/{}/invitations/{id}", w.net);
    assert_eq!(
        call(&w, "DELETE", &revoke, &w.owner, json!(null))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    call(&w, "DELETE", &revoke, &w.owner, json!(null))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    call(
        &w,
        "POST",
        "/api/v1/invitations/accept",
        &third,
        json!({"token": fresh}),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    sqlx::query("UPDATE network_invitations SET expires_at = now() - interval '1 second'")
        .execute(&w.t.pool)
        .await
        .unwrap();
    let r = call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &w.owner,
        json!({"role": "viewer"}),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::CREATED,
        "las caducadas no cuentan en el cupo"
    );
    w.t.finish().await;
}

#[tokio::test]
async fn quitar_a_un_miembro_le_cierra_la_red_y_solo_el_propietario_invita() {
    let Some(w) = world(Config::default()).await else {
        return;
    };
    join(&w, &w.other, "enroller").await;
    let members = call(
        &w,
        "GET",
        &format!("/api/v1/networks/{}/members", w.net),
        &w.owner,
        json!(null),
    )
    .await;
    assert_eq!(members.json.as_array().unwrap().len(), 1);
    assert!(members.json[0].get("email").is_none());

    // Un administrador de la organización que no es propietario no invita.
    let admin = w.t.user().await;
    sqlx::query("INSERT INTO memberships VALUES ($1,$2,'admin')")
        .bind(w.org)
        .bind(admin.user)
        .execute(&w.t.pool)
        .await
        .unwrap();
    call(
        &w,
        "POST",
        &format!("/api/v1/networks/{}/invitations", w.net),
        &admin,
        json!({"role": "viewer"}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "forbidden");

    let remove = format!("/api/v1/networks/{}/members/{}", w.net, w.other.user);
    assert_eq!(
        call(&w, "DELETE", &remove, &w.owner, json!(null))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    call(&w, "DELETE", &remove, &w.owner, json!(null))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    call(
        &w,
        "GET",
        &format!("/api/v1/networks/{}", w.net),
        &w.other,
        json!(null),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    let shared = call(&w, "GET", "/api/v1/shared-networks", &w.other, json!(null)).await;
    assert_eq!(shared.json.as_array().unwrap().len(), 0);
    w.t.finish().await;
}

#[tokio::test]
async fn la_organizacion_personal_tiene_tope_de_redes_y_dispositivos() {
    let Some(t) = TestApp::configured(Config {
        enroll_per_minute: 10_000,
        personal_max_networks: 1,
        personal_max_devices: 1,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    let owner = t.user().await;
    let mut conn = common::connection(&t.pool).await;
    accounts::ensure_personal(&mut conn, owner.user, Some("Ana"))
        .await
        .unwrap();
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM memberships WHERE user_id=$1")
        .bind(owner.user)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.61.0.0/24", "fd61::/64")
        .await;
    let again = t
        .json(
            "POST",
            &format!("/api/v1/orgs/{org}/networks"),
            Some(&owner),
            json!({"name":"otra","ipv4_cidr":"10.62.0.0/24","ipv6_cidr":"fd62::/64","root_anchor":hex::encode(root.verifying_key().to_bytes())}),
        )
        .await;
    again.error(StatusCode::CONFLICT, "network_limit");

    let first = Device::new();
    let token = t.token(&owner, net, false).await;
    assert_eq!(t.enroll(&token, &first).await.status, StatusCode::CREATED);
    let token = t.token(&owner, net, false).await;
    t.enroll(&token, &Device::new())
        .await
        .error(StatusCode::CONFLICT, "device_limit");
    // Revocar libera el hueco.
    assert_eq!(
        t.action(&owner, net, first.id, "revoke").await.status,
        StatusCode::OK
    );
    let token = t.token(&owner, net, false).await;
    assert_eq!(
        t.enroll(&token, &Device::new()).await.status,
        StatusCode::CREATED
    );

    // Una organización normal no tiene esos topes.
    let org2 = t.org(&owner).await;
    t.network(&owner, org2, &root, "10.63.0.0/24", "fd63::/64")
        .await;
    t.network(&owner, org2, &root, "10.64.0.0/24", "fd64::/64")
        .await;
    t.finish().await;
}
