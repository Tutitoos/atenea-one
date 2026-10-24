use crate::{AppState, api::ApiError, audit, networks};
use atenea_identity::{Prefix, Rule, Selector};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize)]
pub struct Selection {
    pub kind: String,
    pub device_id: Option<Uuid>,
    pub prefix: Option<String>,
}
impl Selection {
    pub fn selector(&self) -> Result<Selector, ApiError> {
        match self.kind.as_str() {
            "any" if self.device_id.is_none() && self.prefix.is_none() => Ok(Selector::Any),
            "device" if self.prefix.is_none() => Ok(Selector::Device(
                *self
                    .device_id
                    .ok_or_else(|| ApiError::bad("invalid_selector"))?
                    .as_bytes(),
            )),
            "prefix" if self.device_id.is_none() => {
                let n: ipnet::IpNet = self
                    .prefix
                    .as_ref()
                    .ok_or_else(|| ApiError::bad("invalid_selector"))?
                    .parse()
                    .map_err(|_| ApiError::bad("invalid_selector"))?;
                if n.addr() != n.network() {
                    return Err(ApiError::bad("invalid_selector"));
                }
                Ok(Selector::Prefix(Prefix {
                    addr: n.addr(),
                    len: n.prefix_len(),
                }))
            }
            _ => Err(ApiError::bad("invalid_selector")),
        }
    }
}
#[derive(Deserialize)]
pub struct Policy {
    pub src: Selection,
    pub dst: Selection,
    pub proto: u8,
    #[serde(default)]
    pub port_min: u16,
    #[serde(default)]
    pub port_max: u16,
}
pub async fn rules(c: &mut PgConnection, net: Uuid) -> Result<Vec<Rule>, ApiError> {
    let rows=sqlx::query("SELECT src,dst,proto,port_min,port_max FROM access_policies WHERE network_id=$1 ORDER BY id").bind(net).fetch_all(c).await?;
    rows.iter()
        .map(|r| {
            let src: Selection =
                serde_json::from_value(r.get("src")).map_err(|_| ApiError::internal())?;
            let dst: Selection =
                serde_json::from_value(r.get("dst")).map_err(|_| ApiError::internal())?;
            Ok(Rule {
                src: src.selector()?,
                dst: dst.selector()?,
                proto: r.get::<i16, _>("proto") as u8,
                port_min: r.get::<i32, _>("port_min") as u16,
                port_max: r.get::<i32, _>("port_max") as u16,
            })
        })
        .collect()
}
pub async fn list(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    networks::access(&s, &h, net, false).await?;
    let rows=sqlx::query("SELECT id,src,dst,proto,port_min,port_max FROM access_policies WHERE network_id=$1 ORDER BY id").bind(net).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"src":r.get::<Value,_>("src"),"dst":r.get::<Value,_>("dst"),"proto":r.get::<i16,_>("proto"),"port_min":r.get::<i32,_>("port_min"),"port_max":r.get::<i32,_>("port_max")})).collect::<Vec<_>>())))
}
pub async fn create(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<Policy>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    if ![0, 1, 6, 17, 58].contains(&b.proto)
        || b.port_min > b.port_max
        || !matches!(b.proto, 6 | 17) && (b.port_min != 0 || b.port_max != 0)
    {
        return Err(ApiError::bad("invalid_ports"));
    }
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT ipv4_cidr::text v4,ipv6_cidr::text v6 FROM networks WHERE id=$1 FOR UPDATE",
    )
    .bind(net)
    .fetch_one(&mut *tx)
    .await?;
    for selector in [b.src.selector()?, b.dst.selector()?] {
        match selector {
            Selector::Device(id) => {
                let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM devices WHERE id=$1 AND network_id=$2 AND status!='revoked')").bind(Uuid::from_bytes(id)).bind(net).fetch_one(&mut *tx).await?;
                if !exists {
                    return Err(ApiError::bad("invalid_selector"));
                }
            }
            Selector::Prefix(p) => {
                let n: ipnet::IpNet = row
                    .get::<String, _>(if p.addr.is_ipv4() { "v4" } else { "v6" })
                    .parse()
                    .map_err(|_| ApiError::internal())?;
                if n.prefix_len() > p.len || !n.contains(&p.addr) {
                    return Err(ApiError::bad("invalid_selector"));
                }
            }
            _ => {}
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM access_policies WHERE network_id=$1")
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    if count >= 256 {
        return Err(ApiError::conflict("policy_overflow"));
    }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO access_policies(id,network_id,src,dst,proto,port_min,port_max,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(net).bind(json!(b.src)).bind(json!(b.dst)).bind(i16::from(b.proto)).bind(i32::from(b.port_min)).bind(i32::from(b.port_max)).bind(u).execute(&mut *tx).await?;
    let devices = crate::issue::load_devices(&mut tx, net).await?;
    let rules = rules(&mut tx, net).await?;
    for d in &devices {
        crate::issue::compile(d, &devices, &rules)?;
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "policy.create",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
pub async fn remove(
    State(s): State<AppState>,
    Path((net, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (u, org) = networks::access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("DELETE FROM access_policies WHERE id=$1 AND network_id=$2")
        .bind(id)
        .bind(net)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "policy.delete",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
