// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-026: concesiones de administración delegada. Aquí solo las gestiona un administrador; ejercerlas es de otra fase.
use crate::{AppState, api::ApiError, audit, networks};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

/// Lista cerrada, en el orden en que se guarda y se devuelve.
pub const SCOPES: [&str; 7] = [
    "read:devices",
    "read:policies",
    "write:join-codes",
    "write:approve-devices",
    "write:policies",
    "write:revoke-devices",
    "write:grants",
];
const MAX_SCOPE_DEVICES: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) devices: Vec<Uuid>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub(crate) scopes: Vec<String>,
    pub(crate) scope: Scope,
}

/// Permisos en orden canónico, o `None` si la lista está vacía, repite alguno o nombra uno desconocido.
pub(crate) fn canonical_scopes(requested: &[String]) -> Option<Vec<&'static str>> {
    let known = requested.iter().all(|s| SCOPES.contains(&s.as_str()));
    let unique = requested
        .iter()
        .enumerate()
        .all(|(i, s)| !requested[..i].contains(s));
    (known && unique && !requested.is_empty()).then(|| {
        SCOPES
            .into_iter()
            .filter(|s| requested.iter().any(|r| r == s))
            .collect()
    })
}

/// La lista de dispositivos solo existe con `devices`: de 1 a 64, sin repetir.
pub(crate) fn valid_scope(scope: &Scope) -> bool {
    let unique = scope
        .devices
        .iter()
        .enumerate()
        .all(|(i, d)| !scope.devices[..i].contains(d));
    match scope.kind.as_str() {
        "enrolled" | "network" => scope.devices.is_empty(),
        "devices" => unique && (1..=MAX_SCOPE_DEVICES).contains(&scope.devices.len()),
        _ => false,
    }
}

pub(crate) fn view(r: &PgRow) -> Value {
    json!({"device_id":r.get::<Uuid,_>("device_id"),"scopes":r.get::<Value,_>("scopes"),"scope":{"kind":r.get::<String,_>("scope_kind"),"devices":r.get::<Value,_>("scope_devices")},"granted_by":r.get::<Uuid,_>("granted_by"),"granted_by_device":r.get::<Option<Uuid>,_>("granted_by_device"),"granted_at":r.get::<chrono::DateTime<chrono::Utc>,_>("granted_at")})
}

pub async fn list(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    networks::access(&s, &h, net, false).await?;
    let rows = sqlx::query("SELECT device_id,scopes,scope_kind,scope_devices,granted_by,granted_by_device,granted_at FROM device_grants WHERE network_id=$1 ORDER BY granted_at,device_id").bind(net).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(view).collect::<Vec<_>>())))
}

pub async fn set(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(b): Json<Grant>,
) -> Result<Json<Value>, ApiError> {
    let (user, org) = networks::access(&s, &h, net, true).await?;
    let scopes = canonical_scopes(&b.scopes).ok_or_else(|| ApiError::bad("invalid_grant"))?;
    if !valid_scope(&b.scope) {
        return Err(ApiError::bad("invalid_grant"));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM devices WHERE id=$1 AND network_id=$2")
            .bind(dev)
            .bind(net)
            .fetch_optional(&mut *tx)
            .await?;
    // Solo un dispositivo aprobado puede firmar peticiones útiles: conceder a otro sería una concesión dormida.
    match status.as_deref() {
        None => return Err(ApiError::not_found()),
        Some("approved") => {}
        Some(_) => return Err(ApiError::conflict("device_not_approved")),
    }
    if !members_ok(&mut tx, net, &b.scope.devices).await? {
        return Err(ApiError::bad("invalid_grant"));
    }
    // Cambiar la concesión de quien concede retira lo que él dio (ADR-027, D5).
    cascade(&mut tx, net, dev, "user", user, org).await?;
    let row = store(&mut tx, net, dev, &scopes, &b.scope, user, None).await?;
    audit::record(
        &mut tx,
        "user",
        user,
        Some(org),
        Some(net),
        "device.grant",
        dev,
        json!({"scopes":scopes,"scope":{"kind":b.scope.kind,"devices":b.scope.devices}}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "devices");
    s.panel.changed(net, "audit");
    Ok(Json(view(&row)))
}

pub async fn remove(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (user, org) = networks::access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    if !withdraw(&mut tx, net, dev, "user", user, org, "admin").await? {
        return Err(ApiError::not_found());
    }
    tx.commit().await?;
    s.panel.changed(net, "devices");
    s.panel.changed(net, "audit");
    Ok(StatusCode::NO_CONTENT)
}

/// Borra la concesión del dispositivo, si la tiene, y lo audita. También la usa la revocación del dispositivo.
pub async fn withdraw(
    c: &mut PgConnection,
    net: Uuid,
    dev: Uuid,
    actor_kind: &str,
    actor: Uuid,
    org: Uuid,
    reason: &str,
) -> Result<bool, ApiError> {
    let gone = sqlx::query(
        "DELETE FROM device_grants WHERE device_id=$1 AND network_id=$2 RETURNING scopes,scope_kind",
    )
    .bind(dev)
    .bind(net)
    .fetch_optional(&mut *c)
    .await?;
    let Some(row) = gone else { return Ok(false) };
    audit::record(
        c,
        actor_kind,
        actor,
        Some(org),
        Some(net),
        "device.grant_revoke",
        dev,
        json!({"reason":reason,"scopes":row.get::<Value,_>("scopes"),"scope":{"kind":row.get::<String,_>("scope_kind")}}),
    )
    .await?;
    cascade(c, net, dev, actor_kind, actor, org).await?;
    Ok(true)
}

/// D5 (ADR-027): lo que concedió un dispositivo no sobrevive a un cambio ni a la retirada de su propia concesión.
pub async fn cascade(
    c: &mut PgConnection,
    net: Uuid,
    grantor: Uuid,
    actor_kind: &str,
    actor: Uuid,
    org: Uuid,
) -> Result<(), ApiError> {
    let gone = sqlx::query("DELETE FROM device_grants WHERE granted_by_device=$1 AND network_id=$2 RETURNING device_id,scopes,scope_kind")
        .bind(grantor)
        .bind(net)
        .fetch_all(&mut *c)
        .await?;
    for row in gone {
        audit::record(
            c,
            actor_kind,
            actor,
            Some(org),
            Some(net),
            "device.grant_revoke",
            row.get::<Uuid, _>("device_id"),
            json!({"reason":"grantor_changed","grantor":grantor,"scopes":row.get::<Value,_>("scopes"),"scope":{"kind":row.get::<String,_>("scope_kind")}}),
        )
        .await?;
    }
    Ok(())
}

/// Los dispositivos de un alcance `devices` son de la red y no están revocados.
pub(crate) async fn members_ok(
    c: &mut PgConnection,
    net: Uuid,
    devices: &[Uuid],
) -> Result<bool, ApiError> {
    let members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM devices WHERE network_id=$1 AND status<>'revoked' AND id=ANY($2)",
    )
    .bind(net)
    .bind(devices)
    .fetch_one(c)
    .await?;
    Ok(members == devices.len() as i64)
}

/// Concede o sustituye. `granted_by` es siempre un usuario; `by_device` solo en una concesión delegada.
pub(crate) async fn store(
    c: &mut PgConnection,
    net: Uuid,
    dev: Uuid,
    scopes: &[&str],
    scope: &Scope,
    granted_by: Uuid,
    by_device: Option<Uuid>,
) -> Result<PgRow, ApiError> {
    Ok(sqlx::query("INSERT INTO device_grants(device_id,network_id,scopes,scope_kind,scope_devices,granted_by,granted_by_device) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (device_id) DO UPDATE SET scopes=EXCLUDED.scopes,scope_kind=EXCLUDED.scope_kind,scope_devices=EXCLUDED.scope_devices,granted_by=EXCLUDED.granted_by,granted_by_device=EXCLUDED.granted_by_device,granted_at=now() RETURNING device_id,scopes,scope_kind,scope_devices,granted_by,granted_by_device,granted_at")
        .bind(dev)
        .bind(net)
        .bind(json!(scopes))
        .bind(&scope.kind)
        .bind(json!(scope.devices))
        .bind(granted_by)
        .bind(by_device)
        .fetch_one(c)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn scopes_are_a_closed_list_stored_in_canonical_order() {
        assert_eq!(
            canonical_scopes(&names(&["write:policies", "read:devices"])),
            Some(vec!["read:devices", "write:policies"])
        );
        assert_eq!(canonical_scopes(&names(&SCOPES)), Some(SCOPES.to_vec()));
        for bad in [
            &[][..],
            &["read:devices", "read:devices"],
            &["write:everything"],
            &["READ:DEVICES"],
            &["read:devices", ""],
        ] {
            assert_eq!(canonical_scopes(&names(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn the_device_list_only_exists_for_the_devices_scope() {
        let scope = |kind: &str, devices: Vec<Uuid>| Scope {
            kind: kind.into(),
            devices,
        };
        let one = Uuid::from_u128(1);
        assert!(valid_scope(&scope("enrolled", vec![])));
        assert!(valid_scope(&scope("network", vec![])));
        assert!(valid_scope(&scope("devices", vec![one])));
        assert!(valid_scope(&scope(
            "devices",
            (0..64).map(Uuid::from_u128).collect()
        )));
        assert!(!valid_scope(&scope("enrolled", vec![one])));
        assert!(!valid_scope(&scope("network", vec![one])));
        assert!(!valid_scope(&scope("devices", vec![])));
        assert!(!valid_scope(&scope("devices", vec![one, one])));
        assert!(!valid_scope(&scope(
            "devices",
            (0..65).map(Uuid::from_u128).collect()
        )));
        assert!(!valid_scope(&scope("all", vec![])));
    }
}
