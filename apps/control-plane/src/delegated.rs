// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-026: administración delegada. Un dispositivo aprobado ejerce con su firma la concesión que le dio un
//! administrador. Las lecturas cubren la red; el alcance limita las escrituras.
use crate::{AppState, api::ApiError, audit, devices, grants, policies};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Postgres, Row, Transaction};
use uuid::Uuid;

/// Tasa propia de estas rutas, aparte de la del resto de peticiones del dispositivo.
const PER_MINUTE: u32 = 60;
const MAX_LIVE_CODES: i64 = 8;
const MAX_POLICIES: i64 = 256;
const MAX_CODE_TTL_SECS: u64 = 3600;
const MAX_GIVEN: i64 = 64;

fn missing() -> ApiError {
    ApiError::forbidden("grant_missing")
}
fn out_of_scope() -> ApiError {
    ApiError::forbidden("out_of_scope")
}

/// El delegado autenticado y su concesión, leída dentro de la transacción.
struct Delegate {
    me: Uuid,
    net: Uuid,
    org: Uuid,
    scopes: Value,
    kind: String,
    devices: Vec<Uuid>,
    granted_at: chrono::DateTime<chrono::Utc>,
}

/// Autentica y carga la concesión. Una escritura bloquea la red: revocar al delegado borra su concesión bajo
/// ese mismo bloqueo, así que no se ejerce una concesión retirada.
async fn begin(
    s: &AppState,
    h: &HeaderMap,
    uri: &Uri,
    body: &[u8],
    method: &str,
    permission: Option<&str>,
) -> Result<(Transaction<'static, Postgres>, Delegate), ApiError> {
    let (me, net) = crate::routes::authenticate_as(s, h, uri, body, method).await?;
    crate::take(&s.admin_rate, me, PER_MINUTE)?;
    let mut tx = s.pool.begin().await?;
    let org: Uuid = sqlx::query_scalar(if method == "GET" {
        "SELECT org_id FROM networks WHERE id=$1"
    } else {
        "SELECT org_id FROM networks WHERE id=$1 FOR UPDATE"
    })
    .bind(net)
    .fetch_one(&mut *tx)
    .await?;
    let row = sqlx::query("SELECT g.scopes,g.scope_kind,g.scope_devices,g.granted_at FROM device_grants g JOIN devices d ON d.id=g.device_id WHERE g.device_id=$1 AND g.network_id=$2 AND d.status='approved'")
        .bind(me)
        .bind(net)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    let scopes: Value = row.get("scopes");
    if let Some(p) = permission
        && !scopes.as_array().is_some_and(|a| a.iter().any(|v| v == p))
    {
        return Err(missing());
    }
    let devices =
        serde_json::from_value(row.get("scope_devices")).map_err(|_| ApiError::internal())?;
    Ok((
        tx,
        Delegate {
            me,
            net,
            org,
            scopes,
            kind: row.get("scope_kind"),
            devices,
            granted_at: row.get("granted_at"),
        },
    ))
}

impl Delegate {
    /// Solo el alcance; que el dispositivo exista en la red lo comprueba quien llama.
    async fn covers(&self, c: &mut PgConnection, dev: Uuid) -> Result<bool, ApiError> {
        Ok(match self.kind.as_str() {
            "network" => true,
            "devices" => self.devices.contains(&dev),
            _ => {
                dev == self.me
                    || sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM devices WHERE id=$1 AND network_id=$2 AND enrolled_by_device=$3)")
                        .bind(dev)
                        .bind(self.net)
                        .bind(self.me)
                        .fetch_one(c)
                        .await?
            }
        })
    }
    async fn audit(
        &self,
        c: &mut PgConnection,
        action: &str,
        target: Uuid,
        detail: Value,
    ) -> Result<(), ApiError> {
        audit::record(
            c,
            "device",
            self.me,
            Some(self.org),
            Some(self.net),
            action,
            target,
            detail,
        )
        .await
    }
}

/// Un destino inexistente o de otra red responde `404` antes de mirar el alcance.
async fn target(
    c: &mut PgConnection,
    d: &Delegate,
    dev: Uuid,
) -> Result<(String, Vec<u8>), ApiError> {
    let row = sqlx::query("SELECT status,bundle FROM devices WHERE id=$1 AND network_id=$2")
        .bind(dev)
        .bind(d.net)
        .fetch_optional(&mut *c)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !d.covers(c, dev).await? {
        return Err(out_of_scope());
    }
    Ok((row.get("status"), row.get("bundle")))
}

fn changed(s: &AppState, net: Uuid, scope: &str) {
    s.panel.changed(net, scope);
    s.panel.changed(net, "audit");
}

pub async fn grant(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (_, d) = begin(&s, &h, &uri, &body, "GET", None).await?;
    Ok(Json(
        json!({"device_id":d.me,"network_id":d.net,"scopes":d.scopes,"scope":{"kind":d.kind,"devices":d.devices},"granted_at":d.granted_at}),
    ))
}

pub async fn list_devices(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "GET", Some("read:devices")).await?;
    let rows = sqlx::query("SELECT d.id,d.name,d.role,d.status,d.bundle,d.enrolled_by_device,d.enrolled_at,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=4) ipv4,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=6) ipv6 FROM devices d WHERE d.network_id=$1 ORDER BY d.id")
        .bind(d.net)
        .fetch_all(&mut *tx)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let id: Uuid = r.get("id");
        let enrolled_by: Option<Uuid> = r.get("enrolled_by_device");
        let bundle = atenea_identity::DeviceBundle::parse(&r.get::<Vec<u8>, _>("bundle"))
            .map_err(|_| ApiError::internal())?;
        let in_scope = match d.kind.as_str() {
            "network" => true,
            "devices" => d.devices.contains(&id),
            _ => id == d.me || enrolled_by == Some(d.me),
        };
        out.push(json!({"id":id,"name":r.get::<String,_>("name"),"role":r.get::<String,_>("role"),"status":r.get::<String,_>("status"),"fingerprint":bundle.fingerprint(),"ipv4":r.get::<Option<String>,_>("ipv4"),"ipv6":r.get::<Option<String>,_>("ipv6"),"enrolled_at":r.get::<chrono::DateTime<chrono::Utc>,_>("enrolled_at"),"enrolled_by_device":enrolled_by,"in_scope":in_scope}));
    }
    Ok(Json(json!(out)))
}

pub async fn list_policies(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "GET", Some("read:policies")).await?;
    let rows = sqlx::query("SELECT id,src,dst,proto,port_min,port_max,created_by_device FROM access_policies WHERE network_id=$1 ORDER BY id")
        .bind(d.net)
        .fetch_all(&mut *tx)
        .await?;
    Ok(Json(json!(rows.iter().map(|r| json!({"id":r.get::<Uuid,_>("id"),"src":r.get::<Value,_>("src"),"dst":r.get::<Value,_>("dst"),"proto":r.get::<i16,_>("proto"),"port_min":r.get::<i32,_>("port_min"),"port_max":r.get::<i32,_>("port_max"),"created_by_device":r.get::<Option<Uuid>,_>("created_by_device")})).collect::<Vec<_>>())))
}

/// Extremo de una política delegada: un dispositivo, nunca `any` ni un prefijo.
fn end(selection: &Value) -> Option<Uuid> {
    (selection["kind"] == "device")
        .then(|| selection["device_id"].as_str()?.parse().ok())
        .flatten()
}

pub async fn create_policy(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "POST", Some("write:policies")).await?;
    let b: policies::Policy =
        serde_json::from_slice(&body).map_err(|_| ApiError::bad("invalid_request"))?;
    for selection in [&b.src, &b.dst] {
        if selection.kind != "device" {
            return Err(out_of_scope());
        }
        let dev = selection
            .device_id
            .ok_or_else(|| ApiError::bad("invalid_selector"))?;
        if !d.covers(&mut tx, dev).await? {
            return Err(out_of_scope());
        }
    }
    let mine: i64 =
        sqlx::query_scalar("SELECT count(*) FROM access_policies WHERE created_by_device=$1")
            .bind(d.me)
            .fetch_one(&mut *tx)
            .await?;
    if mine >= MAX_POLICIES {
        return Err(ApiError::conflict("policy_overflow"));
    }
    // Valida igual que la ruta del administrador: selector existente y no revocado, puertos y compilación.
    let id = policies::insert(&mut tx, d.net, &b, None, Some(d.me)).await?;
    d.audit(
        &mut tx,
        "policy.create",
        id,
        json!({"src":b.src,"dst":b.dst,"proto":b.proto,"port_min":b.port_min,"port_max":b.port_max}),
    )
    .await?;
    tx.commit().await?;
    changed(&s, d.net, "policies");
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}

pub async fn remove_policy(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "DELETE", Some("write:policies")).await?;
    let p = sqlx::query("SELECT src,dst,proto,port_min,port_max,created_by_device FROM access_policies WHERE id=$1 AND network_id=$2")
        .bind(id)
        .bind(d.net)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let (src, dst): (Value, Value) = (p.get("src"), p.get("dst"));
    // Nunca las de un administrador, y solo si ambos extremos están en el alcance.
    if p.get::<Option<Uuid>, _>("created_by_device").is_none() {
        return Err(out_of_scope());
    }
    for selection in [&src, &dst] {
        let dev = end(selection).ok_or_else(out_of_scope)?;
        if !d.covers(&mut tx, dev).await? {
            return Err(out_of_scope());
        }
    }
    sqlx::query("DELETE FROM access_policies WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    d.audit(
        &mut tx,
        "policy.delete",
        id,
        json!({"src":src,"dst":dst,"proto":p.get::<i16,_>("proto"),"port_min":p.get::<i32,_>("port_min"),"port_max":p.get::<i32,_>("port_max")}),
    )
    .await?;
    tx.commit().await?;
    changed(&s, d.net, "policies");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinCode {
    ttl_secs: u64,
}

pub async fn join_code(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "POST", Some("write:join-codes")).await?;
    let b: JoinCode =
        serde_json::from_slice(&body).map_err(|_| ApiError::bad("invalid_request"))?;
    if !(60..=MAX_CODE_TTL_SECS).contains(&b.ttl_secs) {
        return Err(ApiError::bad("invalid_ttl"));
    }
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM enrollment_tokens WHERE created_by_device=$1 AND consumed_at IS NULL AND expires_at>now()")
        .bind(d.me)
        .fetch_one(&mut *tx)
        .await?;
    if live >= MAX_LIVE_CODES {
        return Err(ApiError::conflict("join_code_limit"));
    }
    // Sin aprobación automática (D1): el equipo entra pendiente y se aprueba por su huella.
    let (id, reply) = devices::issue_token(
        &s,
        &mut tx,
        d.net,
        b.ttl_secs,
        false,
        "device",
        true,
        None,
        Some(d.me),
    )
    .await?;
    d.audit(
        &mut tx,
        "token.create",
        id,
        json!({"auto_approve":false,"role":"device"}),
    )
    .await?;
    tx.commit().await?;
    changed(&s, d.net, "devices");
    Ok((StatusCode::CREATED, devices::TOKEN_HEADERS, Json(reply)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    fingerprint: String,
}

pub async fn approve(
    State(s): State<AppState>,
    Path(dev): Path<Uuid>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "POST", Some("write:approve-devices")).await?;
    let b: Approval =
        serde_json::from_slice(&body).map_err(|_| ApiError::bad("invalid_request"))?;
    let (_, bundle) = target(&mut tx, &d, dev).await?;
    let bundle = atenea_identity::DeviceBundle::parse(&bundle).map_err(|_| ApiError::internal())?;
    if bundle.fingerprint() != b.fingerprint {
        return Err(ApiError::conflict("fingerprint_mismatch"));
    }
    let addresses = devices::approve_pending(&mut tx, d.net, dev).await?;
    d.audit(
        &mut tx,
        "device.approve",
        dev,
        json!({"addresses":addresses}),
    )
    .await?;
    tx.commit().await?;
    changed(&s, d.net, "devices");
    Ok(Json(json!({"device_id":dev,"status":"approved"})))
}

pub async fn revoke(
    State(s): State<AppState>,
    Path(dev): Path<Uuid>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "POST", Some("write:revoke-devices")).await?;
    if !body.is_empty() {
        return Err(ApiError::bad("invalid_request"));
    }
    target(&mut tx, &d, dev).await?;
    devices::revoke_in(&mut tx, d.net, dev, "device", d.me, d.org, "device.revoke").await?;
    tx.commit().await?;
    changed(&s, d.net, "devices");
    Ok(Json(json!({"device_id":dev,"status":"revoked"})))
}

fn exceeds() -> ApiError {
    ApiError::forbidden("grant_exceeds_own")
}

/// Una concesión delegada no expone al usuario que dio `write:grants`.
fn entry(row: &sqlx::postgres::PgRow) -> Value {
    let mut v = grants::view(row);
    if let Some(o) = v.as_object_mut() {
        o.remove("granted_by");
    }
    v
}

/// ADR-027, D6: las concesiones de los dispositivos del alcance.
pub async fn list_grants(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "GET", Some("write:grants")).await?;
    let rows = sqlx::query("SELECT device_id,scopes,scope_kind,scope_devices,granted_by,granted_by_device,granted_at FROM device_grants WHERE network_id=$1 ORDER BY granted_at,device_id")
        .bind(d.net)
        .fetch_all(&mut *tx)
        .await?;
    let mut out = Vec::new();
    for r in &rows {
        if d.covers(&mut tx, r.get("device_id")).await? {
            out.push(entry(r));
        }
    }
    Ok(Json(json!(out)))
}

/// ADR-027, D2 a D4: concede o sustituye los permisos de un dispositivo del alcance, sin dar más de lo que se tiene.
pub async fn set_grant(
    State(s): State<AppState>,
    Path(dev): Path<Uuid>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "PUT", Some("write:grants")).await?;
    let b: grants::Grant =
        serde_json::from_slice(&body).map_err(|_| ApiError::bad("invalid_request"))?;
    if dev == d.me {
        return Err(ApiError::forbidden("grant_self"));
    }
    let (status, _) = target(&mut tx, &d, dev).await?;
    let scopes =
        grants::canonical_scopes(&b.scopes).ok_or_else(|| ApiError::bad("invalid_grant"))?;
    if !grants::valid_scope(&b.scope) {
        return Err(ApiError::bad("invalid_grant"));
    }
    if status != "approved" {
        return Err(ApiError::conflict("device_not_approved"));
    }
    if !grants::members_ok(&mut tx, d.net, &b.scope.devices).await? {
        return Err(ApiError::bad("invalid_grant"));
    }
    // D2: ni más permisos de los propios ni `write:grants`.
    let own = d.scopes.as_array().ok_or_else(ApiError::internal)?;
    if scopes
        .iter()
        .any(|p| *p == "write:grants" || !own.iter().any(|o| o == p))
    {
        return Err(exceeds());
    }
    // D3: salvo con alcance `network`, solo una lista de dispositivos contenida en el alcance propio.
    if d.kind != "network" {
        if b.scope.kind != "devices" {
            return Err(exceeds());
        }
        for x in &b.scope.devices {
            if !d.covers(&mut tx, *x).await? {
                return Err(exceeds());
            }
        }
    }
    // D4: solo sustituye lo que él dio.
    let current: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT granted_by_device FROM device_grants WHERE device_id=$1")
            .bind(dev)
            .fetch_optional(&mut *tx)
            .await?;
    if current.is_some_and(|by| by != Some(d.me)) {
        return Err(ApiError::forbidden("grant_not_yours"));
    }
    if current.is_none() {
        let given: i64 =
            sqlx::query_scalar("SELECT count(*) FROM device_grants WHERE granted_by_device=$1")
                .bind(d.me)
                .fetch_one(&mut *tx)
                .await?;
        if given >= MAX_GIVEN {
            return Err(ApiError::conflict("grant_limit"));
        }
    }
    // `granted_by` conserva al administrador que dio `write:grants` a quien concede.
    let root: Uuid = sqlx::query_scalar("SELECT granted_by FROM device_grants WHERE device_id=$1")
        .bind(d.me)
        .fetch_one(&mut *tx)
        .await?;
    let row = grants::store(&mut tx, d.net, dev, &scopes, &b.scope, root, Some(d.me)).await?;
    d.audit(
        &mut tx,
        "device.grant",
        dev,
        json!({"scopes":scopes,"scope":{"kind":b.scope.kind,"devices":b.scope.devices}}),
    )
    .await?;
    tx.commit().await?;
    changed(&s, d.net, "devices");
    Ok(Json(entry(&row)))
}

pub async fn remove_grant(
    State(s): State<AppState>,
    Path(dev): Path<Uuid>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let (mut tx, d) = begin(&s, &h, &uri, &body, "DELETE", Some("write:grants")).await?;
    if !body.is_empty() {
        return Err(ApiError::bad("invalid_request"));
    }
    if dev == d.me {
        return Err(ApiError::forbidden("grant_self"));
    }
    target(&mut tx, &d, dev).await?;
    let current: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT granted_by_device FROM device_grants WHERE device_id=$1")
            .bind(dev)
            .fetch_optional(&mut *tx)
            .await?;
    match current {
        None => return Err(ApiError::not_found()),
        Some(by) if by != Some(d.me) => return Err(ApiError::forbidden("grant_not_yours")),
        Some(_) => {}
    }
    grants::withdraw(&mut tx, d.net, dev, "device", d.me, d.org, "delegate").await?;
    tx.commit().await?;
    changed(&s, d.net, "devices");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_un_dispositivo_es_extremo_de_una_politica_delegada() {
        let id = Uuid::from_u128(7);
        assert_eq!(
            end(&json!({"kind":"device","device_id":id,"prefix":null})),
            Some(id)
        );
        assert_eq!(end(&json!({"kind":"any"})), None);
        assert_eq!(
            end(&json!({"kind":"prefix","prefix":"10.86.0.0/24","device_id":id})),
            None
        );
    }
}
