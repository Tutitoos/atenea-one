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
#[derive(Deserialize)]
pub struct Token {
    ttl_secs: u64,
    #[serde(default)]
    auto_approve: bool,
}
pub async fn token(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<Token>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    if !(60..=86400).contains(&b.ttl_secs) {
        return Err(ApiError::bad("invalid_ttl"));
    }
    let raw = URL_SAFE_NO_PAD.encode(atenea_identity::generate_seed());
    let id = Uuid::now_v7();
    let expires = chrono::Utc::now() + chrono::Duration::seconds(b.ttl_secs as i64);
    let mut tx = s.pool.begin().await?;
    let root: Vec<u8> = sqlx::query_scalar("SELECT root_anchor FROM networks WHERE id=$1")
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO enrollment_tokens(id,network_id,token_hash,created_by,expires_at,auto_approve) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(net).bind(Sha256::digest(raw.as_bytes()).as_slice()).bind(u).bind(expires).bind(b.auto_approve).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "token.create",
        id,
        json!({"auto_approve":b.auto_approve}),
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
            json!({"token":raw,"expires_at":expires,"join":{"url":s.config.public_url,"token":raw,"network_id":net,"root_anchor":hex::encode(root)}}),
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
    let name = name(&b.name)?;
    let bytes = hex::decode(&b.bundle).map_err(|_| ApiError::bad("invalid_bundle"))?;
    let bundle = atenea_identity::DeviceBundle::parse(&bytes)
        .map_err(|_| ApiError::bad("invalid_bundle"))?;
    bundle
        .verify()
        .map_err(|_| ApiError::bad("invalid_bundle"))?;
    let dev = Uuid::from_bytes(bundle.device_id);
    let mut tx = s.pool.begin().await?;
    let row=sqlx::query("UPDATE enrollment_tokens SET consumed_at=now(),consumed_by_device=$2 WHERE token_hash=$1 AND consumed_at IS NULL AND expires_at>now() RETURNING network_id,auto_approve").bind(Sha256::digest(b.token.as_bytes()).as_slice()).bind(dev).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED,"invalid_token"))?;
    let net: Uuid = row.get("network_id");
    let auto: bool = row.get("auto_approve");
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,name_source,hostname) VALUES($1,$2,$3,'pending',$4,$5,$6,$7,$8)").bind(dev).bind(net).bind(name).bind(bundle.noise_pubkey.as_slice()).bind(bundle.identity_pubkey.as_slice()).bind(bytes).bind(if b.name_auto { "device" } else { "admin" }).bind(b.name_auto.then_some(name)).execute(&mut *tx).await?;
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
pub async fn list(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    networks::access(&s, &h, net, false).await?;
    let rows = sqlx::query(
        "SELECT d.id,d.name,d.name_source,d.hostname,d.status,d.bundle,d.endpoints,d.enrolled_at,d.approved_at,d.revoked_at,d.last_config_at,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=4) ipv4,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=6) ipv6,r.reported_at,r.nat_mapping,r.paths,r.forwarding_ok,r.dns,r.routes,GREATEST(0,floor(extract(epoch FROM now()-r.reported_at))::bigint) age_secs FROM devices d LEFT JOIN device_reports r ON r.device_id=d.id WHERE d.network_id=$1 ORDER BY d.id",
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
        out.push(json!({"id":r.get::<Uuid,_>("id"),"device_id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"name_source":name_source,"hostname":hostname,"status":r.get::<String,_>("status"),"endpoints":r.get::<Value,_>("endpoints"),"fingerprint":b.fingerprint(),"ipv4":r.get::<Option<String>,_>("ipv4"),"ipv6":r.get::<Option<String>,_>("ipv6"),"enrolled_at":r.get::<chrono::DateTime<chrono::Utc>,_>("enrolled_at"),"approved_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("approved_at"),"revoked_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("revoked_at"),"last_config_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("last_config_at"),"report":report,"connection":connection}));
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
            Ok(Json(json!({"device_id":dev,"status":"revoked"})))
        } else {
            Err(ApiError::not_found())
        };
    }
    sqlx::query("DELETE FROM ip_allocations WHERE device_id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE device_routes SET status='rejected',snat=NULL,decided_at=now(),decided_by=$2 WHERE device_id=$1").bind(dev).bind(u).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "device.revoke",
        dev,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"device_id":dev,"status":"revoked"})))
}
#[derive(Deserialize)]
pub struct Update {
    name: Option<String>,
    #[serde(default)]
    use_system_name: bool,
    endpoints: Option<Vec<SocketAddr>>,
}
pub async fn update(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(b): Json<Update>,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
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
    let result=sqlx::query("UPDATE devices SET name=CASE WHEN $5 THEN hostname ELSE COALESCE($3,name) END,name_source=CASE WHEN $5 THEN 'device' WHEN $3 IS NOT NULL THEN 'admin' ELSE name_source END,endpoints=COALESCE($4,endpoints) WHERE id=$1 AND network_id=$2").bind(dev).bind(net).bind(name).bind(endpoints).bind(b.use_system_name).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
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
