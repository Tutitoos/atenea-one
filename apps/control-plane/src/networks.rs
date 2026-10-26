// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, api::ApiError, audit, auth, orgs};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use ipnet::IpNet;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;
pub async fn access(
    s: &AppState,
    h: &HeaderMap,
    net: Uuid,
    write: bool,
) -> Result<(Uuid, Uuid), ApiError> {
    let u = auth::session(s, h, write).await?;
    let org: Option<Uuid> = sqlx::query_scalar("SELECT org_id FROM networks WHERE id=$1")
        .bind(net)
        .fetch_optional(&s.pool)
        .await?;
    let org = org.ok_or_else(ApiError::not_found)?;
    orgs::role(s, u, org, if write { "admin" } else { "member" }).await?;
    Ok((u, org))
}
#[derive(Deserialize)]
pub struct NewNetwork {
    pub name: String,
    pub ipv4_cidr: String,
    pub ipv6_cidr: String,
    pub root_anchor: String,
}
fn value(r: &PgRow) -> Value {
    json!({"id":r.get::<Uuid,_>("id"),"org_id":r.get::<Uuid,_>("org_id"),"name":r.get::<String,_>("name"),"ipv4_cidr":r.get::<String,_>("v4"),"ipv6_cidr":r.get::<String,_>("v6"),"root_anchor":hex::encode(r.get::<Vec<u8>,_>("root_anchor")),"has_issuer_cert":r.get::<bool,_>("has_cert")})
}
const SELECT: &str = "SELECT id,org_id,name,ipv4_cidr::text v4,ipv6_cidr::text v6,root_anchor,(issuer_cert IS NOT NULL) has_cert FROM networks";
pub async fn create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<NewNetwork>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let u = auth::session(&s, &h, true).await?;
    orgs::role(&s, u, org, "admin").await?;
    orgs::name(&b.name)?;
    let v4: IpNet = b
        .ipv4_cidr
        .parse()
        .map_err(|_| ApiError::bad("invalid_cidr"))?;
    let v6: IpNet = b
        .ipv6_cidr
        .parse()
        .map_err(|_| ApiError::bad("invalid_cidr"))?;
    let prefix = |n: &IpNet| atenea_identity::Prefix {
        addr: n.addr(),
        len: n.prefix_len(),
    };
    if !v4.addr().is_ipv4()
        || !v6.addr().is_ipv6()
        || v4.addr() != v4.network()
        || v6.addr() != v6.network()
        || !prefix(&v4).valid_route()
        || !prefix(&v6).valid_route()
    {
        return Err(ApiError::bad("invalid_cidr"));
    }
    let root: [u8; 32] = hex::decode(&b.root_anchor)
        .ok()
        .and_then(|x| x.try_into().ok())
        .ok_or_else(|| ApiError::bad("invalid_root"))?;
    if root == [0; 32]
        || atenea_identity::VerifyingKey::from_bytes(&root).map_or(true, |k| k.is_weak())
    {
        return Err(ApiError::bad("invalid_root"));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    let overlap:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM networks WHERE org_id=$1 AND (ipv4_cidr && $2::cidr OR ipv6_cidr && $3::cidr))").bind(org).bind(v4.to_string()).bind(v6.to_string()).fetch_one(&mut *tx).await?;
    if overlap {
        return Err(ApiError::conflict("overlapping_network"));
    }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO networks(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor) VALUES($1,$2,$3,$4::cidr,$5::cidr,$6)").bind(id).bind(org).bind(&b.name).bind(v4.to_string()).bind(v6.to_string()).bind(root.as_slice()).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(id),
        "network.create",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(id, "network");
    s.panel.changed(id, "audit");
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"id":id,"org_id":org,"name":b.name,"ipv4_cidr":v4,"ipv6_cidr":v6,"root_anchor":b.root_anchor,"has_issuer_cert":false}),
        ),
    ))
}
pub async fn list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, false).await?;
    orgs::role(&s, u, org, "member").await?;
    let rows = sqlx::query(&format!("{SELECT} WHERE org_id=$1 ORDER BY id"))
        .bind(org)
        .fetch_all(&s.pool)
        .await?;
    Ok(Json(json!(rows.iter().map(value).collect::<Vec<_>>())))
}
pub async fn get(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    access(&s, &h, net, false).await?;
    let r = sqlx::query(&format!("{SELECT} WHERE id=$1"))
        .bind(net)
        .fetch_one(&s.pool)
        .await?;
    Ok(Json(value(&r)))
}
pub async fn remove(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (u, org) = access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network.delete",
        net,
        json!({}),
    )
    .await?;
    sqlx::query("DELETE FROM networks WHERE id=$1")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn certificate(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    b: Bytes,
) -> Result<StatusCode, ApiError> {
    let (u, org) = access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    let root: Vec<u8> =
        sqlx::query_scalar("SELECT root_anchor FROM networks WHERE id=$1 FOR UPDATE")
            .bind(net)
            .fetch_one(&mut *tx)
            .await?;
    let root: [u8; 32] = root.try_into().map_err(|_| ApiError::internal())?;
    let cert =
        atenea_identity::IssuerCert::parse(&b).map_err(|_| ApiError::bad("invalid_certificate"))?;
    cert.verify(&root, *net.as_bytes(), crate::api::now() as u64)
        .map_err(|_| ApiError::bad("invalid_certificate"))?;
    if cert.issuer_pubkey != s.issuer.verifying_key().to_bytes() {
        return Err(ApiError::bad("wrong_issuer"));
    }
    sqlx::query("UPDATE networks SET issuer_cert=$1 WHERE id=$2")
        .bind(b.as_ref())
        .bind(net)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network.issuer_cert",
        net,
        json!({"serial":cert.issuer_serial}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn allocate(c: &mut PgConnection, net: Uuid, dev: Uuid) -> Result<(), ApiError> {
    for attempt in 0..2 {
        sqlx::query("SAVEPOINT ipam_attempt")
            .execute(&mut *c)
            .await?;
        match allocate_once(c, net, dev).await {
            Ok(()) => {
                sqlx::query("RELEASE SAVEPOINT ipam_attempt")
                    .execute(&mut *c)
                    .await?;
                return Ok(());
            }
            Err(error) => {
                sqlx::query("ROLLBACK TO SAVEPOINT ipam_attempt")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("RELEASE SAVEPOINT ipam_attempt")
                    .execute(&mut *c)
                    .await?;
                if attempt == 1 || error.1 != "conflict" {
                    return Err(error);
                }
            }
        }
    }
    Err(ApiError::conflict("ipam_conflict"))
}
async fn allocate_once(c: &mut PgConnection, net: Uuid, dev: Uuid) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT ipv4_cidr::text v4,ipv6_cidr::text v6 FROM networks WHERE id=$1 FOR UPDATE",
    )
    .bind(net)
    .fetch_one(&mut *c)
    .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM devices WHERE network_id=$1 AND status='approved'",
    )
    .bind(net)
    .fetch_one(&mut *c)
    .await?;
    if count >= 256 {
        return Err(ApiError::conflict("network_full"));
    }
    for (family, key) in [(4_i16, "v4"), (6, "v6")] {
        let prefix: IpNet = row
            .get::<String, _>(key)
            .parse()
            .map_err(|_| ApiError::internal())?;
        let used: Vec<String> = sqlx::query_scalar(
            "SELECT host(addr) FROM ip_allocations WHERE network_id=$1 AND family=$2",
        )
        .bind(net)
        .bind(family)
        .fetch_all(&mut *c)
        .await?;
        let used: std::collections::HashSet<std::net::IpAddr> =
            used.iter().filter_map(|s| s.parse().ok()).collect();
        let start = prefix.network();
        let mut candidate = match start {
            std::net::IpAddr::V4(a) => u128::from(u32::from(a)),
            std::net::IpAddr::V6(a) => u128::from(a),
        };
        let address = loop {
            candidate = candidate
                .checked_add(1)
                .ok_or_else(|| ApiError::conflict("network_full"))?;
            let a = if family == 4 {
                if candidate > u128::from(u32::MAX) {
                    return Err(ApiError::conflict("network_full"));
                }
                std::net::IpAddr::V4((candidate as u32).into())
            } else {
                std::net::IpAddr::V6(candidate.into())
            };
            if !prefix.contains(&a) || family == 4 && a == prefix.broadcast() {
                return Err(ApiError::conflict("network_full"));
            }
            if !used.contains(&a) {
                break a;
            }
        };
        sqlx::query("INSERT INTO ip_allocations(network_id,device_id,family,addr) VALUES($1,$2,$3,$4::inet)").bind(net).bind(dev).bind(family).bind(address.to_string()).execute(&mut *c).await?;
    }
    Ok(())
}
