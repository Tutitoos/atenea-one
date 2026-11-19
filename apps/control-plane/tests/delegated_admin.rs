// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-026, fase 3: un dispositivo ejerce su concesión con peticiones firmadas, dentro de su alcance.
mod common;

use atenea_identity::{generate_seed, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Reply, Session, TestApp, now};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

const ADMIN: &str = "/api/v1/device/admin";

struct Setup {
    t: TestApp,
    owner: Session,
    net: Uuid,
    /// Aprobado: el delegado.
    lead: Device,
    /// Aprobado, dado de alta por un administrador: ajeno al alcance por defecto.
    other: Device,
    /// Pendiente, con un código de un administrador.
    pending: Device,
}

async fn setup() -> Option<Setup> {
    let t = TestApp::new().await?;
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.87.0.0/24", "fd87::/64")
        .await;
    t.certificate(&owner, net, &root).await;
    let (lead, other, pending) = (Device::new(), Device::new(), Device::new());
    for (device, auto) in [(&lead, true), (&other, true), (&pending, false)] {
        let token = t.token(&owner, net, auto).await;
        let r = t.enroll(&token, device).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    }
    Some(Setup {
        t,
        owner,
        net,
        lead,
        other,
        pending,
    })
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

async fn grant(s: &Setup, device: &Device, scopes: Value, scope: Value) {
    let r =
        s.t.json(
            "PUT",
            &format!("/api/v1/networks/{}/devices/{}/grant", s.net, device.id),
            Some(&s.owner),
            json!({"scopes":scopes,"scope":scope}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
}

fn all() -> Value {
    json!([
        "read:devices",
        "read:policies",
        "write:join-codes",
        "write:approve-devices",
        "write:policies",
        "write:revoke-devices"
    ])
}

fn policy(src: Uuid, dst: Uuid) -> Value {
    json!({"src":{"kind":"device","device_id":src},"dst":{"kind":"device","device_id":dst},"proto":6,"port_min":22,"port_max":22})
}

/// El delegado da de alta un equipo con un código suyo; queda pendiente.
async fn join(s: &Setup, device: &Device) {
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/join-codes"),
        Some(json!({"ttl_secs":600})),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.headers["cache-control"], "no-store");
    let r = s.t.enroll(r.json["token"].as_str().unwrap(), device).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
}

async fn status(s: &Setup, dev: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_one(&s.t.pool)
        .await
        .unwrap()
}

/// Todas las rutas, con una petición bien formada para cada una.
fn every(target: Uuid) -> Vec<(&'static str, String, Option<Value>)> {
    vec![
        ("GET", format!("{ADMIN}/grant"), None),
        ("GET", format!("{ADMIN}/devices"), None),
        ("GET", format!("{ADMIN}/policies"), None),
        (
            "POST",
            format!("{ADMIN}/policies"),
            Some(policy(target, target)),
        ),
        ("DELETE", format!("{ADMIN}/policies/{target}"), None),
        (
            "POST",
            format!("{ADMIN}/join-codes"),
            Some(json!({"ttl_secs":600})),
        ),
        (
            "POST",
            format!("{ADMIN}/devices/{target}/approve"),
            Some(json!({"fingerprint":"x"})),
        ),
        ("POST", format!("{ADMIN}/devices/{target}/revoke"), None),
        ("GET", format!("{ADMIN}/grants"), None),
        (
            "PUT",
            format!("{ADMIN}/devices/{target}/grant"),
            Some(json!({"scopes":["read:devices"],"scope":{"kind":"network"}})),
        ),
        ("DELETE", format!("{ADMIN}/devices/{target}/grant"), None),
    ]
}

#[tokio::test]
async fn sin_concesion_o_sin_el_permiso_todo_responde_grant_missing() {
    let Some(s) = setup().await else { return };
    for (method, path, body) in every(s.other.id) {
        signed(&s.t, &s.lead, method, &path, body)
            .await
            .error(StatusCode::FORBIDDEN, "grant_missing");
    }
    // Con una concesión de solo lectura del inventario, lo demás sigue cerrado aunque el alcance sea la red.
    grant(
        &s,
        &s.lead,
        json!(["read:devices"]),
        json!({"kind":"network"}),
    )
    .await;
    for (method, path, body) in every(s.other.id) {
        let r = signed(&s.t, &s.lead, method, &path, body).await;
        if path == format!("{ADMIN}/grant") || path.ends_with("/admin/devices") {
            assert_eq!(r.status, StatusCode::OK, "{path} {}", r.json);
        } else {
            r.error(StatusCode::FORBIDDEN, "grant_missing");
        }
    }
    assert_eq!(status(&s, s.other.id).await, "approved");
    // Un dispositivo pendiente no ejerce nada, y una firma ajena tampoco.
    signed(&s.t, &s.pending, "GET", &format!("{ADMIN}/grant"), None)
        .await
        .error(StatusCode::FORBIDDEN, "device_not_approved");
    let r =
        s.t.json(
            "GET",
            &format!("{ADMIN}/devices"),
            Some(&s.owner),
            json!(null),
        )
        .await;
    r.error(StatusCode::UNAUTHORIZED, "invalid_signature");
    s.t.finish().await;
}

#[tokio::test]
async fn con_alcance_enrolled_da_de_alta_aprueba_por_huella_y_conecta_lo_suyo() {
    let Some(s) = setup().await else { return };
    grant(&s, &s.lead, all(), json!({"kind":"enrolled"})).await;
    let new = Device::new();
    join(&s, &new).await;
    assert_eq!(status(&s, new.id).await, "pending");

    // Una huella distinta se rechaza sin cambiar nada.
    let approve = format!("{ADMIN}/devices/{}/approve", new.id);
    signed(
        &s.t,
        &s.lead,
        "POST",
        &approve,
        Some(json!({"fingerprint":s.other.bundle.fingerprint()})),
    )
    .await
    .error(StatusCode::CONFLICT, "fingerprint_mismatch");
    assert_eq!(status(&s, new.id).await, "pending");
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &approve,
        Some(json!({"fingerprint":new.bundle.fingerprint()})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(status(&s, new.id).await, "approved");

    // El inventario cubre toda la red y marca el alcance y quién dio de alta a cada uno.
    let r = signed(&s.t, &s.lead, "GET", &format!("{ADMIN}/devices"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let devices = r.json.as_array().unwrap();
    assert_eq!(devices.len(), 4);
    let row = |id: Uuid| {
        devices
            .iter()
            .find(|d| d["id"] == id.to_string())
            .unwrap()
            .clone()
    };
    assert_eq!(row(new.id)["in_scope"], true);
    assert_eq!(row(new.id)["enrolled_by_device"], s.lead.id.to_string());
    assert_eq!(row(new.id)["fingerprint"], new.bundle.fingerprint());
    assert_eq!(row(s.lead.id)["in_scope"], true);
    assert_eq!(row(s.other.id)["in_scope"], false);
    assert_eq!(row(s.other.id)["enrolled_by_device"], Value::Null);

    // Política entre ambos: el equipo nuevo aparece como par del delegado.
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/policies"),
        Some(policy(s.lead.id, new.id)),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let created = common::id(&r.json);
    let r = signed(&s.t, &s.lead, "GET", "/api/v1/device/peers", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert!(
        r.body
            .windows(36)
            .any(|w| w == new.id.to_string().as_bytes()),
        "{}",
        r.json
    );
    let r = signed(&s.t, &s.lead, "GET", &format!("{ADMIN}/policies"), None).await;
    assert_eq!(r.json.as_array().unwrap().len(), 1);
    assert_eq!(r.json[0]["created_by_device"], s.lead.id.to_string());

    // Borra la suya y revoca lo que dio de alta.
    let r = signed(
        &s.t,
        &s.lead,
        "DELETE",
        &format!("{ADMIN}/policies/{created}"),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.json);
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/devices/{}/revoke", new.id),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(status(&s, new.id).await, "revoked");

    // Cada acción queda en la auditoría con el dispositivo autor.
    let audit: Vec<String> = sqlx::query(
        "SELECT action FROM audit_events WHERE network_id=$1 AND actor_kind='device' AND actor_id=$2 AND action<>'device.enroll' ORDER BY id",
    )
    .bind(s.net)
    .bind(s.lead.id.to_string())
    .fetch_all(&s.t.pool)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get("action"))
    .collect();
    assert_eq!(
        audit,
        [
            "token.create",
            "device.approve",
            "policy.create",
            "policy.delete",
            "device.revoke"
        ]
    );
    s.t.finish().await;
}

#[tokio::test]
async fn fuera_de_alcance_no_nombra_ni_aprueba_ni_revoca_ni_borra_lo_de_un_administrador() {
    let Some(s) = setup().await else { return };
    grant(&s, &s.lead, all(), json!({"kind":"enrolled"})).await;
    let policies = format!("{ADMIN}/policies");
    // Nombrar a un ajeno, en cualquiera de los dos extremos.
    for body in [
        policy(s.lead.id, s.other.id),
        policy(s.other.id, s.lead.id),
        // Ni `any` ni un prefijo, aunque el otro extremo sea propio.
        json!({"src":{"kind":"any"},"dst":{"kind":"device","device_id":s.lead.id},"proto":0}),
        json!({"src":{"kind":"device","device_id":s.lead.id},"dst":{"kind":"prefix","prefix":"10.87.0.0/24"},"proto":0}),
    ] {
        signed(&s.t, &s.lead, "POST", &policies, Some(body))
            .await
            .error(StatusCode::FORBIDDEN, "out_of_scope");
    }
    // Un pendiente de otro código, aun con su huella correcta.
    signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/devices/{}/approve", s.pending.id),
        Some(json!({"fingerprint":s.pending.bundle.fingerprint()})),
    )
    .await
    .error(StatusCode::FORBIDDEN, "out_of_scope");
    assert_eq!(status(&s, s.pending.id).await, "pending");
    signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/devices/{}/revoke", s.other.id),
        None,
    )
    .await
    .error(StatusCode::FORBIDDEN, "out_of_scope");
    assert_eq!(status(&s, s.other.id).await, "approved");
    // Un destino inexistente es 404, no una pista sobre el alcance.
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/devices/{}/revoke", Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.json);

    // Una política de un administrador no se borra ni con toda la red en el alcance.
    let r =
        s.t.json(
            "POST",
            &format!("/api/v1/networks/{}/policies", s.net),
            Some(&s.owner),
            policy(s.lead.id, s.other.id),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let admin_policy = common::id(&r.json);
    grant(&s, &s.lead, all(), json!({"kind":"network"})).await;
    signed(
        &s.t,
        &s.lead,
        "DELETE",
        &format!("{policies}/{admin_policy}"),
        None,
    )
    .await
    .error(StatusCode::FORBIDDEN, "out_of_scope");
    // Con la red en el alcance sí nombra al ajeno, pero sigue sin `any`.
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &policies,
        Some(policy(s.other.id, s.lead.id)),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let delegated = common::id(&r.json);
    signed(
        &s.t,
        &s.lead,
        "POST",
        &policies,
        Some(json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":0})),
    )
    .await
    .error(StatusCode::FORBIDDEN, "out_of_scope");

    // Con una lista explícita, la política delegada cuyo extremo queda fuera ya no es suya.
    grant(
        &s,
        &s.lead,
        all(),
        json!({"kind":"devices","devices":[s.lead.id]}),
    )
    .await;
    signed(
        &s.t,
        &s.lead,
        "DELETE",
        &format!("{policies}/{delegated}"),
        None,
    )
    .await
    .error(StatusCode::FORBIDDEN, "out_of_scope");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM access_policies WHERE network_id=$1")
        .bind(s.net)
        .fetch_one(&s.t.pool)
        .await
        .unwrap();
    assert_eq!(left, 2);
    s.t.finish().await;
}

#[tokio::test]
async fn los_codigos_delegados_caducan_en_una_hora_y_hay_ocho_vivos_como_mucho() {
    let Some(s) = setup().await else { return };
    grant(
        &s,
        &s.lead,
        json!(["write:join-codes"]),
        json!({"kind":"enrolled"}),
    )
    .await;
    let path = format!("{ADMIN}/join-codes");
    for ttl in [59, 3601] {
        signed(&s.t, &s.lead, "POST", &path, Some(json!({"ttl_secs":ttl})))
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_ttl");
    }
    // Un delegado no elige aprobación automática ni rol.
    signed(
        &s.t,
        &s.lead,
        "POST",
        &path,
        Some(json!({"ttl_secs":600,"auto_approve":true})),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_request");
    let mut first = String::new();
    for i in 0..8 {
        let r = signed(&s.t, &s.lead, "POST", &path, Some(json!({"ttl_secs":3600}))).await;
        assert_eq!(r.status, StatusCode::CREATED, "{i} {}", r.json);
        if i == 0 {
            first = r.json["token"].as_str().unwrap().to_owned();
        }
    }
    signed(&s.t, &s.lead, "POST", &path, Some(json!({"ttl_secs":600})))
        .await
        .error(StatusCode::CONFLICT, "join_code_limit");
    // Un código consumido deja sitio; el equipo entra pendiente, nunca aprobado.
    let new = Device::new();
    let r = s.t.enroll(&first, &new).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(status(&s, new.id).await, "pending");
    let r = signed(&s.t, &s.lead, "POST", &path, Some(json!({"ttl_secs":600}))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    s.t.finish().await;
}

#[tokio::test]
async fn revocar_al_delegado_retira_el_permiso_y_conserva_lo_que_creo() {
    let Some(s) = setup().await else { return };
    grant(&s, &s.lead, all(), json!({"kind":"enrolled"})).await;
    let new = Device::new();
    join(&s, &new).await;
    let mate = Device::new();
    join(&s, &mate).await;
    for device in [&new, &mate] {
        let r = signed(
            &s.t,
            &s.lead,
            "POST",
            &format!("{ADMIN}/devices/{}/approve", device.id),
            Some(json!({"fingerprint":device.bundle.fingerprint()})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/policies"),
        Some(policy(new.id, mate.id)),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    // El equipo dado de alta no hereda permisos.
    signed(&s.t, &new, "GET", &format!("{ADMIN}/grant"), None)
        .await
        .error(StatusCode::FORBIDDEN, "grant_missing");

    assert_eq!(
        s.t.action(&s.owner, s.net, s.lead.id, "revoke")
            .await
            .status,
        StatusCode::OK
    );
    signed(&s.t, &s.lead, "GET", &format!("{ADMIN}/grant"), None)
        .await
        .error(StatusCode::FORBIDDEN, "device_not_approved");
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM device_grants WHERE network_id=$1")
        .bind(s.net)
        .fetch_one(&s.t.pool)
        .await
        .unwrap();
    assert_eq!(grants, 0);
    // Lo que creó sigue ahí y lo administra un administrador.
    assert_eq!(status(&s, new.id).await, "approved");
    let r =
        s.t.json(
            "GET",
            &format!("/api/v1/networks/{}/policies", s.net),
            Some(&s.owner),
            json!(null),
        )
        .await;
    assert_eq!(r.json.as_array().unwrap().len(), 1);
    assert_eq!(r.json[0]["created_by_device"], s.lead.id.to_string());
    // El inventario del administrador dice quién dio de alta a cada equipo; los del administrador, nadie.
    let devices =
        s.t.json(
            "GET",
            &format!("/api/v1/networks/{}/devices", s.net),
            Some(&s.owner),
            json!(null),
        )
        .await;
    let by = |id: Uuid| {
        devices
            .json
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == id.to_string())
            .unwrap()["enrolled_by_device"]
            .clone()
    };
    assert_eq!(by(new.id), s.lead.id.to_string());
    assert_eq!(by(s.other.id), Value::Null);
    let r =
        s.t.json(
            "DELETE",
            &format!(
                "/api/v1/networks/{}/policies/{}",
                s.net,
                common::id(&r.json[0])
            ),
            Some(&s.owner),
            json!(null),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.json);
    s.t.finish().await;
}

/// Un delegado con `write:grants` y alcance sobre `other` y un tercero aprobado.
async fn with_grants(s: &Setup) -> Device {
    let third = Device::new();
    let token = s.t.token(&s.owner, s.net, true).await;
    let r = s.t.enroll(&token, &third).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    grant(
        s,
        &s.lead,
        json!(["read:devices", "read:policies", "write:grants"]),
        json!({"kind":"devices","devices":[s.other.id,third.id]}),
    )
    .await;
    third
}

fn give(scopes: Value, scope: Value) -> Option<Value> {
    Some(json!({"scopes":scopes,"scope":scope}))
}

async fn grants_of(s: &Setup) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT device_id FROM device_grants WHERE network_id=$1 AND granted_by_device IS NOT NULL ORDER BY device_id")
        .bind(s.net)
        .fetch_all(&s.t.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn conceder_permisos_no_sube_de_nivel_ni_toca_lo_ajeno() {
    let Some(s) = setup().await else { return };
    let third = with_grants(&s).await;
    let to = |d: Uuid| format!("{ADMIN}/devices/{d}/grant");

    // Lo propio y dentro del alcance: se concede y queda constancia de quién lo dio, sin el usuario.
    let r = signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(s.other.id),
        give(
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["granted_by_device"], s.lead.id.to_string());
    assert!(r.json.get("granted_by").is_none());
    assert_eq!(grants_of(&s).await, vec![s.other.id]);

    // D2 y D3: ni permisos que no tiene, ni `write:grants`, ni un alcance mayor, ni salir del suyo.
    for (scopes, scope) in [
        (
            json!(["write:policies"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
        (
            json!(["write:grants"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
        (json!(["read:devices"]), json!({"kind":"network"})),
        (json!(["read:devices"]), json!({"kind":"enrolled"})),
        (
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[s.lead.id]}),
        ),
        (
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[s.pending.id]}),
        ),
    ] {
        signed(
            &s.t,
            &s.lead,
            "PUT",
            &to(third.id),
            give(scopes.clone(), scope.clone()),
        )
        .await
        .error(StatusCode::FORBIDDEN, "grant_exceeds_own");
    }
    // Ni la suya ni un dispositivo fuera del alcance; un destino de otra red o inexistente es 404.
    signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(s.lead.id),
        give(
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
    )
    .await
    .error(StatusCode::FORBIDDEN, "grant_self");
    signed(&s.t, &s.lead, "DELETE", &to(s.lead.id), None)
        .await
        .error(StatusCode::FORBIDDEN, "grant_self");
    signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(s.pending.id),
        give(
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
    )
    .await
    .error(StatusCode::FORBIDDEN, "out_of_scope");
    signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(Uuid::new_v4()),
        give(
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    // Un cuerpo mal formado se rechaza.
    signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(third.id),
        give(json!([]), json!({"kind":"devices","devices":[third.id]})),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_grant");

    // D4: la concesión de un administrador no se sustituye ni se retira.
    grant(
        &s,
        &third,
        json!(["read:devices"]),
        json!({"kind":"enrolled"}),
    )
    .await;
    signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(third.id),
        give(
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[third.id]}),
        ),
    )
    .await
    .error(StatusCode::FORBIDDEN, "grant_not_yours");
    signed(&s.t, &s.lead, "DELETE", &to(third.id), None)
        .await
        .error(StatusCode::FORBIDDEN, "grant_not_yours");

    // D6: la lista cubre solo el alcance y no lleva al usuario; el administrador lo ve todo.
    let r = signed(&s.t, &s.lead, "GET", &format!("{ADMIN}/grants"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let ids: Vec<&str> = r
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["device_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(
        ids.contains(&s.other.id.to_string().as_str())
            && ids.contains(&third.id.to_string().as_str())
    );
    assert!(
        r.json
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g.get("granted_by").is_none())
    );

    // Retirar lo que dio: 204, y una segunda vez no hay nada (404). El de un administrador sigue.
    assert_eq!(
        signed(&s.t, &s.lead, "DELETE", &to(s.other.id), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    signed(&s.t, &s.lead, "DELETE", &to(s.other.id), None)
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    assert!(grants_of(&s).await.is_empty());
    let audit: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_events WHERE network_id=$1 AND actor_kind='device' AND actor_id=$2 AND action LIKE 'device.grant%' ORDER BY id")
        .bind(s.net)
        .bind(s.lead.id.to_string())
        .fetch_all(&s.t.pool)
        .await
        .unwrap();
    assert_eq!(audit, ["device.grant", "device.grant_revoke"]);
    s.t.finish().await;
}

async fn give_both(s: &Setup, third: Uuid) {
    for d in [s.other.id, third] {
        let r = signed(
            &s.t,
            &s.lead,
            "PUT",
            &format!("{ADMIN}/devices/{d}/grant"),
            give(
                json!(["read:devices"]),
                json!({"kind":"devices","devices":[s.other.id]}),
            ),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }
}

async fn reasons(s: &Setup) -> Vec<String> {
    sqlx::query_scalar("SELECT detail->>'reason' FROM audit_events WHERE network_id=$1 AND action='device.grant_revoke' ORDER BY id")
        .bind(s.net)
        .fetch_all(&s.t.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn cambiar_retirar_o_revocar_a_quien_concede_retira_lo_que_dio() {
    let Some(s) = setup().await else { return };
    let third = with_grants(&s).await;

    // Un administrador sustituye la concesión del que concede: se va lo que dio.
    give_both(&s, third.id).await;
    assert_eq!(grants_of(&s).await.len(), 2);
    grant(
        &s,
        &s.lead,
        json!(["read:devices", "write:grants"]),
        json!({"kind":"devices","devices":[s.other.id,third.id]}),
    )
    .await;
    assert!(grants_of(&s).await.is_empty());
    // Retirársela del todo, también.
    give_both(&s, third.id).await;
    let r =
        s.t.json(
            "DELETE",
            &format!("/api/v1/networks/{}/devices/{}/grant", s.net, s.lead.id),
            Some(&s.owner),
            json!(null),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.json);
    assert!(grants_of(&s).await.is_empty());
    // Y revocarlo.
    grant(
        &s,
        &s.lead,
        json!(["read:devices", "write:grants"]),
        json!({"kind":"devices","devices":[s.other.id,third.id]}),
    )
    .await;
    give_both(&s, third.id).await;
    let r =
        s.t.json(
            "POST",
            &format!("/api/v1/networks/{}/devices/{}/revoke", s.net, s.lead.id),
            Some(&s.owner),
            json!(null),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert!(grants_of(&s).await.is_empty());
    let reasons = reasons(&s).await;
    assert_eq!(
        reasons.iter().filter(|r| *r == "grantor_changed").count(),
        6
    );
    s.t.finish().await;
}

#[tokio::test]
async fn el_alcance_de_lo_concedido_cabe_en_el_de_quien_concede() {
    let Some(s) = setup().await else { return };
    let to = |d: Uuid| format!("{ADMIN}/devices/{d}/grant");
    // Con alcance `network` puede dar cualquier forma de alcance.
    grant(
        &s,
        &s.lead,
        json!(["read:devices", "write:grants"]),
        json!({"kind":"network"}),
    )
    .await;
    for scope in [
        json!({"kind":"network"}),
        json!({"kind":"enrolled"}),
        json!({"kind":"devices","devices":[s.pending.id]}),
    ] {
        let r = signed(
            &s.t,
            &s.lead,
            "PUT",
            &to(s.other.id),
            give(json!(["read:devices"]), scope),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }
    // Con `enrolled`, solo una lista dentro de lo que él dio de alta (y él mismo).
    grant(
        &s,
        &s.lead,
        json!([
            "read:devices",
            "write:join-codes",
            "write:approve-devices",
            "write:grants"
        ]),
        json!({"kind":"enrolled"}),
    )
    .await;
    let new = Device::new();
    join(&s, &new).await;
    let r = signed(
        &s.t,
        &s.lead,
        "POST",
        &format!("{ADMIN}/devices/{}/approve", new.id),
        Some(json!({"fingerprint":new.bundle.fingerprint()})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let r = signed(
        &s.t,
        &s.lead,
        "PUT",
        &to(new.id),
        give(
            json!(["read:devices"]),
            json!({"kind":"devices","devices":[new.id,s.lead.id]}),
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    for scope in [
        json!({"kind":"devices","devices":[s.other.id]}),
        json!({"kind":"enrolled"}),
        json!({"kind":"network"}),
    ] {
        signed(
            &s.t,
            &s.lead,
            "PUT",
            &to(new.id),
            give(json!(["read:devices"]), scope),
        )
        .await
        .error(StatusCode::FORBIDDEN, "grant_exceeds_own");
    }
    s.t.finish().await;
}
