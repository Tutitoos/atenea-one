//! DNS dividido: servidores alcanzables y dominios de enrutamiento explícitos.
use crate::{AppState, api::ApiError};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dns {
    pub servers: Vec<std::net::IpAddr>,
    pub domains: Vec<String>,
}
pub async fn load(c: &mut PgConnection, net: Uuid) -> Result<Dns, ApiError> {
    let row = sqlx::query("SELECT dns_servers,dns_domains FROM networks WHERE id=$1")
        .bind(net)
        .fetch_one(c)
        .await?;
    Ok(Dns {
        servers: serde_json::from_value(row.get("dns_servers"))
            .map_err(|_| ApiError::internal())?,
        domains: serde_json::from_value(row.get("dns_domains"))
            .map_err(|_| ApiError::internal())?,
    })
}
pub async fn get(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Dns>, ApiError> {
    crate::networks::access(&s, &h, net, false).await?;
    let mut c = s.pool.acquire().await?;
    Ok(Json(load(&mut c, net).await?))
}
pub async fn set(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<Dns>,
) -> Result<StatusCode, ApiError> {
    let (user, org) = crate::networks::access(&s, &h, net, true).await?;
    if b.servers.len() > 3
        || b.domains.len() > 8
        || b.domains.iter().any(|d| !atenea_identity::valid_domain(d))
        || b.servers
            .iter()
            .enumerate()
            .any(|(i, s)| b.servers[..i].contains(s))
        || b.domains
            .iter()
            .enumerate()
            .any(|(i, d)| b.domains[..i].contains(d))
    {
        return Err(ApiError::bad("invalid_dns"));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    for server in &b.servers {
        let valid: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM networks WHERE id=$1 AND ($2::inet <<= ipv4_cidr OR $2::inet <<= ipv6_cidr)) OR EXISTS(SELECT 1 FROM device_routes r JOIN devices d ON d.id=r.device_id WHERE r.network_id=$1 AND r.status='approved' AND d.status='approved' AND $2::inet <<= r.prefix)").bind(net).bind(server.to_string()).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(ApiError::bad("invalid_dns"));
        }
    }
    sqlx::query("UPDATE networks SET dns_servers=$2,dns_domains=$3 WHERE id=$1")
        .bind(net)
        .bind(serde_json::json!(b.servers))
        .bind(serde_json::json!(b.domains))
        .execute(&mut *tx)
        .await?;
    crate::audit::record(
        &mut tx,
        "user",
        user,
        Some(org),
        Some(net),
        "dns.set",
        net,
        serde_json::json!(b),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "dns");
    s.panel.changed(net, "audit");
    Ok(StatusCode::NO_CONTENT)
}
