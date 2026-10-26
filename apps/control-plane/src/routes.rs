// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Declaraciones del dispositivo y aprobación administrativa bajo bloqueo de red.
use crate::{AppState, api::ApiError};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

pub fn prefix(value: &str) -> Result<atenea_identity::Prefix, ApiError> {
    let n: ipnet::IpNet = value.parse().map_err(|_| ApiError::bad("invalid_route"))?;
    if n.addr() != n.network() {
        return Err(ApiError::bad("invalid_route"));
    }
    let p = atenea_identity::Prefix {
        addr: n.addr(),
        len: n.prefix_len(),
    };
    if !p.valid_route() {
        return Err(ApiError::bad("invalid_route"));
    }
    Ok(p)
}

pub async fn authenticate(
    s: &AppState,
    h: &HeaderMap,
    uri: &Uri,
    body: &[u8],
) -> Result<(Uuid, Uuid), ApiError> {
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
        body,
    )
    .map_err(|_| invalid())?;
    crate::take(&s.device_rate, dev, s.config.device_per_minute)?;
    crate::claim_nonce(&s.nonces, dev, signed.nonce, now)?;
    Ok((dev, row.get("network_id")))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    routes: Vec<String>,
}
pub async fn declare(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let (dev, net) = authenticate(&s, &h, &uri, &body).await?;
    let b: Declaration =
        serde_json::from_slice(&body).map_err(|_| ApiError::bad("invalid_route"))?;
    if b.routes.len() > 8 {
        return Err(ApiError::bad("invalid_route"));
    }
    let mut routes = Vec::new();
    for value in &b.routes {
        let p = prefix(value)?;
        let n = ipnet::IpNet::new(p.addr, p.len).map_err(|_| ApiError::bad("invalid_route"))?;
        if routes.iter().any(|other: &ipnet::IpNet| {
            other.contains(&n.network()) || n.contains(&other.network())
        }) {
            return Err(ApiError::bad("route_overlap"));
        }
        routes.push(n);
    }
    let mut tx = s.pool.begin().await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    let row = sqlx::query("SELECT status,routes_declared_at > now()-interval '5 seconds' recent FROM devices WHERE id=$1 FOR UPDATE").bind(dev).fetch_one(&mut *tx).await?;
    if row.get::<String, _>("status") != "approved" {
        return Err(ApiError::forbidden("device_not_approved"));
    }
    if row.get::<Option<bool>, _>("recent").unwrap_or(false) {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    let values: Vec<String> = routes.iter().map(ToString::to_string).collect();
    sqlx::query("DELETE FROM device_routes WHERE device_id=$1 AND NOT (prefix::text=ANY($2))")
        .bind(dev)
        .bind(&values)
        .execute(&mut *tx)
        .await?;
    for value in &values {
        sqlx::query("INSERT INTO device_routes(id,network_id,device_id,prefix) VALUES($1,$2,$3,$4::cidr) ON CONFLICT(device_id,prefix) DO NOTHING").bind(Uuid::now_v7()).bind(net).bind(dev).bind(value).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE devices SET routes_declared_at=now() WHERE id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    crate::audit::record(
        &mut tx,
        "device",
        dev,
        Some(org),
        Some(net),
        "route.declare",
        dev,
        json!({"routes":values}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "routes");
    s.panel.changed(net, "audit");
    Ok(StatusCode::NO_CONTENT)
}
pub async fn list(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    crate::networks::access(&s, &h, net, false).await?;
    let rows = sqlx::query("SELECT id,device_id,prefix::text,status,snat,declared_at,decided_at FROM device_routes WHERE network_id=$1 ORDER BY declared_at,id").bind(net).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|r| json!({"id":r.get::<Uuid,_>("id"),"device_id":r.get::<Uuid,_>("device_id"),"prefix":r.get::<String,_>("prefix"),"status":r.get::<String,_>("status"),"snat":r.get::<Option<bool>,_>("snat"),"declared_at":r.get::<chrono::DateTime<chrono::Utc>,_>("declared_at"),"decided_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("decided_at")})).collect::<Vec<_>>())))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    snat: bool,
}
pub async fn approve(
    State(s): State<AppState>,
    Path((net, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(b): Json<Approval>,
) -> Result<StatusCode, ApiError> {
    decide(s, net, id, h, Some(b.snat)).await
}
pub async fn reject(
    State(s): State<AppState>,
    Path((net, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    decide(s, net, id, h, None).await
}
async fn decide(
    s: AppState,
    net: Uuid,
    id: Uuid,
    h: HeaderMap,
    snat: Option<bool>,
) -> Result<StatusCode, ApiError> {
    let (user, org) = crate::networks::access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let value: String = sqlx::query_scalar("SELECT r.prefix::text FROM device_routes r JOIN devices d ON d.id=r.device_id WHERE r.id=$1 AND r.network_id=$2 AND d.status='approved'").bind(id).bind(net).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::not_found)?;
    if snat.is_some() {
        let overlap: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM networks WHERE id=$1 AND (ipv4_cidr && $3::cidr OR ipv6_cidr && $3::cidr)) OR EXISTS(SELECT 1 FROM device_routes WHERE network_id=$1 AND id<>$2 AND status='approved' AND prefix && $3::cidr)").bind(net).bind(id).bind(&value).fetch_one(&mut *tx).await?;
        if overlap {
            return Err(ApiError::conflict("route_overlap"));
        }
    }
    let status = if snat.is_some() {
        "approved"
    } else {
        "rejected"
    };
    sqlx::query("UPDATE device_routes SET status=$3,snat=$4,decided_at=now(),decided_by=$5 WHERE network_id=$1 AND id=$2").bind(net).bind(id).bind(status).bind(snat).bind(user).execute(&mut *tx).await?;
    crate::audit::record(
        &mut tx,
        "user",
        user,
        Some(org),
        Some(net),
        if snat.is_some() {
            "route.approve"
        } else {
            "route.reject"
        },
        id,
        json!({"snat":snat}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "routes");
    s.panel.changed(net, "audit");
    Ok(StatusCode::NO_CONTENT)
}
