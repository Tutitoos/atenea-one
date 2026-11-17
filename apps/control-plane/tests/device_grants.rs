// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-026, fase 2: concesiones de administración delegada gestionadas por un administrador.
mod common;

use atenea_identity::{generate_seed, signing_key};
use axum::http::StatusCode;
use common::{Device, Session, TestApp};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

struct Setup {
    t: TestApp,
    owner: Session,
    org: Uuid,
    net: Uuid,
    /// Aprobado: puede recibir una concesión.
    lead: Device,
    /// Aprobado: otro dispositivo de la misma red.
    other: Device,
    /// Pendiente de aprobación.
    pending: Device,
}

async fn setup() -> Option<Setup> {
    let t = TestApp::new().await?;
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.86.0.0/24", "fd86::/64")
        .await;
    let (lead, other, pending) = (Device::new(), Device::new(), Device::new());
    for (device, auto) in [(&lead, true), (&other, true), (&pending, false)] {
        let token = t.token(&owner, net, auto).await;
        let r = t.enroll(&token, device).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    }
    Some(Setup {
        t,
        owner,
        org,
        net,
        lead,
        other,
        pending,
    })
}

fn path(net: Uuid, dev: Uuid) -> String {
    format!("/api/v1/networks/{net}/devices/{dev}/grant")
}

async fn grants(t: &TestApp, session: &Session, net: Uuid) -> Value {
    let r = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/grants"),
            Some(session),
            json!(null),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    r.json
}

async fn audit_actions(t: &TestApp, net: Uuid) -> Vec<(String, String, Value)> {
    sqlx::query("SELECT action,actor_kind,detail FROM audit_events WHERE network_id=$1 AND action LIKE 'device.grant%' ORDER BY id")
        .bind(net)
        .fetch_all(&t.pool)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get("action"), r.get("actor_kind"), r.get("detail")))
        .collect()
}

#[tokio::test]
async fn un_administrador_concede_sustituye_y_retira_y_queda_auditado() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        owner,
        net,
        lead,
        other,
        ..
    } = &s;
    assert_eq!(grants(t, owner, *net).await, json!([]));

    // Alcance por defecto del ADR; los permisos vuelven en orden canónico, no en el recibido.
    let r = t
        .json(
            "PUT",
            &path(*net, lead.id),
            Some(owner),
            json!({"scopes":["write:policies","write:join-codes","read:devices"],"scope":{"kind":"enrolled"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["device_id"], lead.id.to_string());
    assert_eq!(
        r.json["scopes"],
        json!(["read:devices", "write:join-codes", "write:policies"])
    );
    assert_eq!(r.json["scope"], json!({"kind":"enrolled","devices":[]}));
    assert_eq!(r.json["granted_by"], owner.user.to_string());
    assert!(r.json["granted_at"].is_string());
    assert_eq!(grants(t, owner, *net).await, json!([r.json]));

    // Concederla de nuevo la sustituye: sigue habiendo una sola.
    let r = t
        .json(
            "PUT",
            &path(*net, lead.id),
            Some(owner),
            json!({"scopes":["read:policies"],"scope":{"kind":"devices","devices":[other.id, lead.id]}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let listed = grants(t, owner, *net).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["scopes"], json!(["read:policies"]));
    assert_eq!(
        listed[0]["scope"],
        json!({"kind":"devices","devices":[other.id, lead.id]})
    );

    let r = t
        .json("DELETE", &path(*net, lead.id), Some(owner), json!(null))
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.json);
    assert_eq!(grants(t, owner, *net).await, json!([]));
    t.json("DELETE", &path(*net, lead.id), Some(owner), json!(null))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");

    let audit = audit_actions(t, *net).await;
    let actions: Vec<&str> = audit.iter().map(|(a, _, _)| a.as_str()).collect();
    assert_eq!(
        actions,
        ["device.grant", "device.grant", "device.grant_revoke"]
    );
    assert!(audit.iter().all(|(_, kind, _)| kind == "user"));
    assert_eq!(
        audit[0].2,
        json!({"scopes":["read:devices","write:join-codes","write:policies"],"scope":{"kind":"enrolled","devices":[]}})
    );
    assert_eq!(audit[2].2["reason"], "admin");
    s.t.finish().await;
}

#[tokio::test]
async fn solo_se_concede_a_un_aprobado_de_la_red_con_permisos_y_alcance_validos() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        owner,
        org,
        net,
        lead,
        other,
        pending,
    } = &s;
    let valid = json!({"scopes":["read:devices"],"scope":{"kind":"enrolled"}});

    t.json("PUT", &path(*net, pending.id), Some(owner), valid.clone())
        .await
        .error(StatusCode::CONFLICT, "device_not_approved");
    t.json(
        "PUT",
        &path(*net, Uuid::new_v4()),
        Some(owner),
        valid.clone(),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");

    // Un dispositivo de otra red de la misma organización no vale ni como destinatario ni dentro del alcance.
    let root = signing_key(generate_seed());
    let elsewhere = t
        .network(owner, *org, &root, "10.87.0.0/24", "fd87::/64")
        .await;
    let foreign = Device::new();
    let token = t.token(owner, elsewhere, true).await;
    assert_eq!(t.enroll(&token, &foreign).await.status, StatusCode::CREATED);
    t.json("PUT", &path(*net, foreign.id), Some(owner), valid.clone())
        .await
        .error(StatusCode::NOT_FOUND, "not_found");

    // Un revocado tampoco puede figurar en el alcance.
    let gone = Device::new();
    let token = t.token(owner, *net, true).await;
    assert_eq!(t.enroll(&token, &gone).await.status, StatusCode::CREATED);
    assert_eq!(
        t.action(owner, *net, gone.id, "revoke").await.status,
        StatusCode::OK
    );

    for invalid in [
        json!({"scopes":[],"scope":{"kind":"enrolled"}}),
        json!({"scopes":["read:devices","read:devices"],"scope":{"kind":"enrolled"}}),
        json!({"scopes":["write:everything"],"scope":{"kind":"enrolled"}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"all"}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"enrolled","devices":[other.id]}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"network","devices":[other.id]}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"devices"}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"devices","devices":[other.id, other.id]}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"devices","devices":[foreign.id]}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"devices","devices":[gone.id]}}),
        json!({"scopes":["read:devices"],"scope":{"kind":"devices","devices":[Uuid::new_v4()]}}),
    ] {
        let r = t
            .json("PUT", &path(*net, lead.id), Some(owner), invalid.clone())
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{invalid}: {}", r.json);
        assert_eq!(r.json["error"]["code"], "invalid_grant", "{invalid}");
    }
    // Un campo desconocido o un tipo erróneo se rechaza antes de llegar a la validación.
    for malformed in [
        json!({"scopes":["read:devices"],"scope":{"kind":"enrolled"},"extra":1}),
        json!({"scopes":["read:devices"],"scope":{"kind":"enrolled","extra":1}}),
        json!({"scopes":"read:devices","scope":{"kind":"enrolled"}}),
        json!({"scopes":["read:devices"]}),
    ] {
        let r = t
            .json("PUT", &path(*net, lead.id), Some(owner), malformed.clone())
            .await;
        assert!(r.status.is_client_error(), "{malformed}: {}", r.status);
    }
    // Ningún rechazo dejó una concesión ni un evento.
    assert_eq!(grants(t, owner, *net).await, json!([]));
    assert!(audit_actions(t, *net).await.is_empty());

    // Los seis permisos y el alcance de toda la red son válidos.
    let r = t
        .json(
            "PUT",
            &path(*net, lead.id),
            Some(owner),
            json!({"scopes":["read:devices","read:policies","write:join-codes","write:approve-devices","write:policies","write:revoke-devices"],"scope":{"kind":"network"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["scopes"].as_array().unwrap().len(), 6);
    s.t.finish().await;
}

#[tokio::test]
async fn conceder_es_de_admin_u_owner_y_un_miembro_solo_lista() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        owner,
        org,
        net,
        lead,
        other,
        ..
    } = &s;
    let (admin, member, stranger) = (t.user().await, t.user().await, t.user().await);
    for (user, role) in [(&admin, "admin"), (&member, "member")] {
        let r = t
            .json(
                "PUT",
                &format!("/api/v1/orgs/{org}/members/{}", user.user),
                Some(owner),
                json!({"role":role}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }
    let body = json!({"scopes":["read:devices"],"scope":{"kind":"enrolled"}});

    let r = t
        .json("PUT", &path(*net, lead.id), Some(&admin), body.clone())
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["granted_by"], admin.user.to_string());

    // Miembro: lee, no escribe.
    assert_eq!(grants(t, &member, *net).await.as_array().unwrap().len(), 1);
    let r = t
        .json("PUT", &path(*net, other.id), Some(&member), body.clone())
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.json);
    let r = t
        .json("DELETE", &path(*net, lead.id), Some(&member), json!(null))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.json);

    // Sin sesión y ajeno a la organización: nada, y la red ajena no revela su existencia.
    let r = t
        .json("PUT", &path(*net, other.id), None, body.clone())
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.json);
    let r = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/grants"),
            None,
            json!(null),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.json);
    t.json("PUT", &path(*net, other.id), Some(&stranger), body.clone())
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    t.json(
        "GET",
        &format!("/api/v1/networks/{net}/grants"),
        Some(&stranger),
        json!(null),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");

    assert_eq!(grants(t, owner, *net).await.as_array().unwrap().len(), 1);
    s.t.finish().await;
}

#[tokio::test]
async fn revocar_el_dispositivo_borra_su_concesion_y_lo_audita() {
    let Some(s) = setup().await else { return };
    let Setup {
        t,
        owner,
        net,
        lead,
        other,
        ..
    } = &s;
    for device in [lead, other] {
        let r = t
            .json(
                "PUT",
                &path(*net, device.id),
                Some(owner),
                json!({"scopes":["write:policies"],"scope":{"kind":"network"}}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }
    assert_eq!(
        t.action(owner, *net, lead.id, "revoke").await.status,
        StatusCode::OK
    );
    // Solo desaparece la del revocado.
    let left = grants(t, owner, *net).await;
    assert_eq!(left.as_array().unwrap().len(), 1);
    assert_eq!(left[0]["device_id"], other.id.to_string());
    let audit = audit_actions(t, *net).await;
    let last = audit.last().unwrap();
    assert_eq!(last.0, "device.grant_revoke");
    assert_eq!(last.2["reason"], "device_revoked");
    // Revocar de nuevo es idempotente y no añade eventos.
    assert_eq!(
        t.action(owner, *net, lead.id, "revoke").await.status,
        StatusCode::OK
    );
    assert_eq!(audit_actions(t, *net).await.len(), audit.len());
    // Un revocado ya no puede recibir otra.
    t.json(
        "PUT",
        &path(*net, lead.id),
        Some(owner),
        json!({"scopes":["read:devices"],"scope":{"kind":"enrolled"}}),
    )
    .await
    .error(StatusCode::CONFLICT, "device_not_approved");
    s.t.finish().await;
}
