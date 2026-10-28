// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, api::ApiError, audit, networks};
use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;
pub fn name(value: &str) -> Result<&str, ApiError> {
    if value
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    {
        return Err(ApiError::bad("invalid_name"));
    }
    let value = value.trim();
    if !(1..=64).contains(&value.chars().count()) {
        return Err(ApiError::bad("invalid_name"));
    }
    Ok(value)
}
fn role(value: &Value) -> Result<&str, ApiError> {
    match value.as_str() {
        Some(role @ ("device" | "server")) => Ok(role),
        _ => Err(ApiError::bad("invalid_request")),
    }
}
fn present_role<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}
#[derive(Deserialize)]
pub struct Token {
    ttl_secs: u64,
    #[serde(default)]
    auto_approve: bool,
    #[serde(default, deserialize_with = "present_role")]
    role: Option<Value>,
}
pub async fn token(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<Token>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    let role = b.role.as_ref().map(role).transpose()?.unwrap_or("device");
    if !(60..=86400).contains(&b.ttl_secs) {
        return Err(ApiError::bad("invalid_ttl"));
    }
    let raw = URL_SAFE_NO_PAD.encode(atenea_identity::generate_seed());
    let id = Uuid::now_v7();
    let expires = chrono::Utc::now() + chrono::Duration::seconds(b.ttl_secs as i64);
    let mut tx = s.pool.begin().await?;
    let r = sqlx::query("SELECT root_anchor,name FROM networks WHERE id=$1")
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    let (root, net_name): (Vec<u8>, String) = (r.get("root_anchor"), r.get("name"));
    // Un nombre o URL que no caben en el código no impiden crear el token: `code` es null.
    let code = networks::join_code(&s, net, &net_name, &root, Some(&raw)).ok();
    let anchor: [u8; 32] = root
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::internal())?;
    let fingerprint = atenea_identity::anchor_fingerprint(&anchor);
    sqlx::query("INSERT INTO enrollment_tokens(id,network_id,token_hash,created_by,expires_at,auto_approve,role) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(net).bind(Sha256::digest(raw.as_bytes()).as_slice()).bind(u).bind(expires).bind(b.auto_approve).bind(role).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "token.create",
        id,
        json!({"auto_approve":b.auto_approve,"role":role}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        [
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        Json(
            json!({"token":raw,"expires_at":expires,"join":{"url":s.config.public_url,"token":raw,"network_id":net,"root_anchor":hex::encode(root)},"code":code,"root_anchor_fingerprint":fingerprint}),
        ),
    ))
}
#[derive(Deserialize)]
pub struct Enrollment {
    token: String,
    bundle: String,
    name: String,
    #[serde(default)]
    name_auto: bool,
}
pub async fn enroll(
    State(s): State<AppState>,
    source: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    h: HeaderMap,
    Json(b): Json<Enrollment>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ip = crate::client_ip(&s.config, source.map(|a| a.0.0.ip()), &h);
    crate::take(&s.rate, ip, s.config.enroll_per_minute)?;
    let (name, bytes, bundle) = validated(&b.name, &b.bundle)?;
    let dev = Uuid::from_bytes(bundle.device_id);
    let mut tx = s.pool.begin().await?;
    let row=sqlx::query("UPDATE enrollment_tokens SET consumed_at=now(),consumed_by_device=$2 WHERE token_hash=$1 AND consumed_at IS NULL AND expires_at>now() RETURNING network_id,auto_approve,role").bind(Sha256::digest(b.token.as_bytes()).as_slice()).bind(dev).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED,"invalid_token"))?;
    let net: Uuid = row.get("network_id");
    let auto: bool = row.get("auto_approve");
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    insert(
        &mut tx,
        net,
        dev,
        name,
        b.name_auto,
        bytes,
        &bundle,
        "token",
        &row.get::<String, _>("role"),
    )
    .await?;
    if auto {
        networks::allocate(&mut tx, net, dev).await?;
        sqlx::query("UPDATE devices SET status='approved',approved_at=now() WHERE id=$1")
            .bind(dev)
            .execute(&mut *tx)
            .await?;
    }
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM networks WHERE id=$1")
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "device",
        dev,
        Some(org),
        Some(net),
        "device.enroll",
        dev,
        json!({"auto_approve":auto}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "devices");
    s.panel.changed(net, "audit");
    Ok((
        StatusCode::CREATED,
        Json(json!({"device_id":dev,"network_id":net,"status":if auto{"approved"}else{"pending"}})),
    ))
}
/// Valida nombre y bundle (compartido por `enroll` y `request_join`).
fn validated<'a>(
    name_in: &'a str,
    bundle_hex: &str,
) -> Result<(&'a str, Vec<u8>, atenea_identity::DeviceBundle), ApiError> {
    let name = name(name_in)?;
    let bytes = hex::decode(bundle_hex).map_err(|_| ApiError::bad("invalid_bundle"))?;
    let bundle = atenea_identity::DeviceBundle::parse(&bytes)
        .map_err(|_| ApiError::bad("invalid_bundle"))?;
    bundle
        .verify()
        .map_err(|_| ApiError::bad("invalid_bundle"))?;
    Ok((name, bytes, bundle))
}
#[allow(clippy::too_many_arguments)]
async fn insert(
    tx: &mut sqlx::PgConnection,
    net: Uuid,
    dev: Uuid,
    name: &str,
    name_auto: bool,
    bytes: Vec<u8>,
    bundle: &atenea_identity::DeviceBundle,
    origin: &str,
    role: &str,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,name_source,hostname,join_origin,role) VALUES($1,$2,$3,'pending',$4,$5,$6,$7,$8,$9,$10)").bind(dev).bind(net).bind(name).bind(bundle.noise_pubkey.as_slice()).bind(bundle.identity_pubkey.as_slice()).bind(bytes).bind(if name_auto { "device" } else { "admin" }).bind(name_auto.then_some(name)).bind(origin).bind(role).execute(tx).await?;
    Ok(())
}
#[derive(Deserialize)]
pub struct JoinRequest {
    bundle: String,
    name: String,
    #[serde(default)]
    name_auto: bool,
}
/// Solicitud de unión sin token (red `con_solicitud`): crea un dispositivo `pending` que un
/// administrador aprueba comparando huellas. Red inexistente y red cerrada responden igual.
pub async fn request_join(
    State(s): State<AppState>,
    source: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<JoinRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ip = crate::client_ip(&s.config, source.map(|a| a.0.0.ip()), &h);
    crate::take(&s.rate, ip, s.config.enroll_per_minute)?;
    let (name, bytes, bundle) = validated(&b.name, &b.bundle)?;
    let dev = Uuid::from_bytes(bundle.device_id);
    let mut tx = s.pool.begin().await?;
    // La fila de la red serializa el conteo y la inserción entre solicitudes concurrentes.
    let org: Uuid = sqlx::query_scalar(
        "SELECT org_id FROM networks WHERE id=$1 AND join_policy='con_solicitud' FOR UPDATE",
    )
    .bind(net)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::not_found)?;
    sqlx::query("DELETE FROM devices WHERE network_id=$1 AND status='pending' AND join_origin='request' AND enrolled_at < now() - $2 * interval '1 second'")
        .bind(net)
        .bind(s.config.join_request_ttl_secs as i64)
        .execute(&mut *tx)
        .await?;
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM devices WHERE network_id=$1 AND status='pending' AND join_origin='request'")
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    if pending >= i64::from(s.config.max_pending_join_requests) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too_many_pending"));
    }
    insert(
        &mut tx,
        net,
        dev,
        name,
        b.name_auto,
        bytes,
        &bundle,
        "request",
        "device",
    )
    .await?;
    audit::record(
        &mut tx,
        "device",
        dev,
        Some(org),
        Some(net),
        "device.join_requested",
        dev,
        json!({"fingerprint":bundle.fingerprint(),"name_auto":b.name_auto}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "devices");
    s.panel.changed(net, "audit");
    Ok((
        StatusCode::CREATED,
        Json(json!({"device_id":dev,"network_id":net,"status":"pending"})),
    ))
}
pub async fn list(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    networks::access(&s, &h, net, false).await?;
    let rows = sqlx::query(
        "SELECT d.id,d.name,d.role,d.name_source,d.hostname,d.status,d.join_origin,d.bundle,d.endpoints,d.enrolled_at,d.approved_at,d.revoked_at,d.last_config_at,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=4) ipv4,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=6) ipv6,r.reported_at,r.nat_mapping,r.paths,r.forwarding_ok,r.dns,r.routes,GREATEST(0,floor(extract(epoch FROM now()-r.reported_at))::bigint) age_secs FROM devices d LEFT JOIN device_reports r ON r.device_id=d.id WHERE d.network_id=$1 ORDER BY d.id",
    )
    .bind(net)
    .fetch_all(&s.pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        let name_source: String = r.get("name_source");
        let hostname: Option<String> = r.get("hostname");
        let bytes: Vec<u8> = r.get("bundle");
        let b = atenea_identity::DeviceBundle::parse(&bytes).map_err(|_| ApiError::internal())?;
        let reported_at: Option<chrono::DateTime<chrono::Utc>> = r.get("reported_at");
        let age: i64 = r.get("age_secs");
        let connection = if reported_at.is_none() {
            "unknown"
        } else if age < 90 {
            "online"
        } else if age < 600 {
            "stale"
        } else {
            "offline"
        };
        let report = reported_at.map(|at|json!({"reported_at":at,"age_secs":age,"nat_mapping":r.get::<Option<String>,_>("nat_mapping"),"paths":r.get::<Value,_>("paths"),"forwarding_ok":r.get::<Option<bool>,_>("forwarding_ok"),"dns":r.get::<Option<String>,_>("dns"),"routes":r.get::<Option<Value>,_>("routes")}));
        out.push(json!({"role":r.get::<String,_>("role"),"id":r.get::<Uuid,_>("id"),"device_id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"name_source":name_source,"hostname":hostname,"status":r.get::<String,_>("status"),"join_origin":r.get::<String,_>("join_origin"),"endpoints":r.get::<Value,_>("endpoints"),"fingerprint":b.fingerprint(),"ipv4":r.get::<Option<String>,_>("ipv4"),"ipv6":r.get::<Option<String>,_>("ipv6"),"enrolled_at":r.get::<chrono::DateTime<chrono::Utc>,_>("enrolled_at"),"approved_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("approved_at"),"revoked_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("revoked_at"),"last_config_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("last_config_at"),"report":report,"connection":connection}));
    }
    Ok(Json(json!(out)))
}
pub async fn approve(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM devices WHERE id=$1 AND network_id=$2 FOR UPDATE")
            .bind(dev)
            .bind(net)
            .fetch_optional(&mut *tx)
            .await?;
    let status = status.ok_or_else(ApiError::not_found)?;
    if status != "pending" {
        return Err(ApiError::conflict("device_not_pending"));
    }
    networks::allocate(&mut tx, net, dev).await?;
    sqlx::query("UPDATE devices SET status='approved',approved_at=now() WHERE id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "device.approve",
        dev,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"device_id":dev,"status":"approved"})))
}
pub async fn revoke(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    revoke_device(&s, net, dev, Some((u, org))).await?;
    Ok(Json(json!({"device_id":dev,"status":"revoked"})))
}

async fn revoke_device(
    s: &AppState,
    net: Uuid,
    dev: Uuid,
    admin: Option<(Uuid, Uuid)>,
) -> Result<(), ApiError> {
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query(
        "UPDATE devices SET status='revoked',revoked_at=now() WHERE id=$1 AND network_id=$2 AND status<>'revoked'",
    )
    .bind(dev)
    .bind(net)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        // Idempotente: ya revocado no reescribe revoked_at ni duplica auditoría.
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM devices WHERE id=$1 AND network_id=$2)",
        )
        .bind(dev)
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
        return if exists {
            Ok(())
        } else {
            Err(ApiError::not_found())
        };
    }
    sqlx::query("DELETE FROM ip_allocations WHERE device_id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    let (actor_kind, actor, org, action) = match admin {
        Some((user, org)) => ("user", user, org, "device.revoke"),
        None => {
            let org = sqlx::query_scalar("SELECT org_id FROM networks WHERE id=$1")
                .bind(net)
                .fetch_one(&mut *tx)
                .await?;
            ("device", dev, org, "device.leave")
        }
    };
    sqlx::query("UPDATE device_routes SET status='rejected',snat=NULL,decided_at=now(),decided_by=$2 WHERE device_id=$1").bind(dev).bind(admin.map(|(user, _)| user)).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        actor_kind,
        actor,
        Some(org),
        Some(net),
        action,
        dev,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    if admin.is_none() {
        s.panel.changed(net, "devices");
        s.panel.changed(net, "audit");
    }
    Ok(())
}

pub async fn leave(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: axum::http::Uri,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    let invalid = || ApiError(StatusCode::UNAUTHORIZED, "invalid_signature");
    let signed = atenea_identity::api_auth::parse_authorization(
        h.get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    let now = crate::api::now() as u64;
    if now.abs_diff(signed.ts) > 60 {
        return Err(invalid());
    }
    let dev = Uuid::from_bytes(signed.device_id);
    let row = sqlx::query("SELECT network_id,identity_pubkey FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(invalid)?;
    let key: [u8; 32] = row
        .get::<Vec<u8>, _>("identity_pubkey")
        .try_into()
        .map_err(|_| invalid())?;
    atenea_identity::api_auth::verify_request(
        &signed,
        &key,
        "POST",
        uri.path_and_query().map_or(uri.path(), |p| p.as_str()),
        &body,
    )
    .map_err(|_| invalid())?;
    crate::take(&s.device_rate, dev, s.config.device_per_minute)?;
    crate::claim_nonce(&s.nonces, dev, signed.nonce, now)?;
    if !body.is_empty() {
        return Err(ApiError::bad("invalid_request"));
    }
    revoke_device(&s, row.get("network_id"), dev, None).await?;
    Ok(Json(json!({"device_id":dev,"status":"revoked"})))
}
#[derive(Deserialize)]
pub struct Update {
    name: Option<String>,
    #[serde(default)]
    use_system_name: bool,
    endpoints: Option<Vec<SocketAddr>>,
    #[serde(default, deserialize_with = "present_role")]
    role: Option<Value>,
}
pub async fn update(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(b): Json<Update>,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    let role = b.role.as_ref().map(role).transpose()?;
    if b.name.is_some() && b.use_system_name {
        return Err(ApiError::bad("invalid_request"));
    }
    let name = b.name.as_deref().map(name).transpose()?;
    let endpoints = b.endpoints.map(|mut e| {
        e.sort();
        e.dedup();
        e
    });
    if endpoints.as_ref().is_some_and(|e| {
        e.len() > 4
            || atenea_nat_traversal::sanitize_candidates(e.iter().copied(), 4).len() != e.len()
    }) {
        return Err(ApiError::bad("invalid_endpoints"));
    }
    let endpoints = endpoints.map(|e| json!(e));
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let previous: String =
        sqlx::query_scalar("SELECT role FROM devices WHERE id=$1 AND network_id=$2 FOR UPDATE")
            .bind(dev)
            .bind(net)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::not_found)?;
    if b.use_system_name {
        let hostname: Option<Option<String>> = sqlx::query_scalar(
            "SELECT hostname FROM devices WHERE id=$1 AND network_id=$2 FOR UPDATE",
        )
        .bind(dev)
        .bind(net)
        .fetch_optional(&mut *tx)
        .await?;
        if hostname.ok_or_else(ApiError::not_found)?.is_none() {
            return Err(ApiError::conflict("no_system_name"));
        }
    }
    let result=sqlx::query("UPDATE devices SET name=CASE WHEN $5 THEN hostname ELSE COALESCE($3,name) END,name_source=CASE WHEN $5 THEN 'device' WHEN $3 IS NOT NULL THEN 'admin' ELSE name_source END,endpoints=COALESCE($4,endpoints),role=COALESCE($6,role) WHERE id=$1 AND network_id=$2").bind(dev).bind(net).bind(name).bind(endpoints).bind(b.use_system_name).bind(role).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    if let Some(role) = role.filter(|role| *role != previous) {
        audit::record(
            &mut tx,
            "user",
            u,
            Some(org),
            Some(net),
            "device.role",
            dev,
            json!({"previous":previous,"new":role}),
        )
        .await?;
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "device.update",
        dev,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"device_id":dev})))
}
