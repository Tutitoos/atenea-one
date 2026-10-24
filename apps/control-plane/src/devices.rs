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
use std::{
    net::{IpAddr, SocketAddr},
    time::Instant,
};
use uuid::Uuid;
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
) -> Result<(StatusCode, Json<Value>), ApiError> {
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
}
pub async fn enroll(
    State(s): State<AppState>,
    source: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Json(b): Json<Enrollment>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ip = source.map_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), |a| a.0.0.ip());
    {
        let mut rates = s.rate.lock().map_err(|_| ApiError::internal())?;
        let limit = f64::from(s.config.enroll_per_minute);
        let now = Instant::now();
        rates.retain(|_, (_, at)| now.duration_since(*at).as_secs() < 120);
        let entry = rates.entry(ip).or_insert((limit, now));
        entry.0 = (entry.0 + now.duration_since(entry.1).as_secs_f64() * limit / 60.).min(limit);
        entry.1 = now;
        if entry.0 < 1. {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
        }
        entry.0 -= 1.;
    }
    crate::orgs::name(&b.name)?;
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
    sqlx::query("INSERT INTO devices(id,network_id,name,status,noise_pubkey,identity_pubkey,bundle) VALUES($1,$2,$3,'pending',$4,$5,$6)").bind(dev).bind(net).bind(&b.name).bind(bundle.noise_pubkey.as_slice()).bind(bundle.identity_pubkey.as_slice()).bind(bytes).execute(&mut *tx).await?;
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
        "SELECT d.id,d.name,d.status,d.bundle,d.endpoints,d.enrolled_at,d.approved_at,d.revoked_at,d.last_config_at,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=4) ipv4,(SELECT host(addr) FROM ip_allocations WHERE device_id=d.id AND family=6) ipv6,r.reported_at,r.nat_mapping,r.paths,GREATEST(0,floor(extract(epoch FROM now()-r.reported_at))::bigint) age_secs FROM devices d LEFT JOIN device_reports r ON r.device_id=d.id WHERE d.network_id=$1 ORDER BY d.id",
    )
    .bind(net)
    .fetch_all(&s.pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
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
        let report = reported_at.map(|at|json!({"reported_at":at,"age_secs":age,"nat_mapping":r.get::<Option<String>,_>("nat_mapping"),"paths":r.get::<Value,_>("paths")}));
        out.push(json!({"id":r.get::<Uuid,_>("id"),"device_id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"status":r.get::<String,_>("status"),"endpoints":r.get::<Value,_>("endpoints"),"fingerprint":b.fingerprint(),"ipv4":r.get::<Option<String>,_>("ipv4"),"ipv6":r.get::<Option<String>,_>("ipv6"),"enrolled_at":r.get::<chrono::DateTime<chrono::Utc>,_>("enrolled_at"),"approved_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("approved_at"),"revoked_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("revoked_at"),"last_config_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("last_config_at"),"report":report,"connection":connection}));
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
        "UPDATE devices SET status='revoked',revoked_at=now() WHERE id=$1 AND network_id=$2",
    )
    .bind(dev)
    .bind(net)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    sqlx::query("DELETE FROM ip_allocations WHERE device_id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
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
    endpoints: Option<Vec<SocketAddr>>,
}
pub async fn update(
    State(s): State<AppState>,
    Path((net, dev)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(b): Json<Update>,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    if let Some(n) = &b.name {
        crate::orgs::name(n)?;
    }
    if b.endpoints
        .as_ref()
        .is_some_and(|e| e.len() > 4 || e.iter().any(|e| e.port() == 0))
    {
        return Err(ApiError::bad("invalid_endpoints"));
    }
    let endpoints = b.endpoints.map(|mut e| {
        e.sort();
        e.dedup();
        json!(e)
    });
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let result=sqlx::query("UPDATE devices SET name=COALESCE($3,name),endpoints=COALESCE($4,endpoints) WHERE id=$1 AND network_id=$2").bind(dev).bind(net).bind(b.name).bind(endpoints).execute(&mut *tx).await?;
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
