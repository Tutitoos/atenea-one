// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, api::ApiError};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
#[allow(clippy::too_many_arguments)]
pub async fn record(
    c: &mut PgConnection,
    kind: &str,
    actor: Uuid,
    org: Option<Uuid>,
    net: Option<Uuid>,
    action: &str,
    target: Uuid,
    detail: Value,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO audit_events(actor_kind,actor_id,org_id,network_id,action,target,detail) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(kind).bind(actor.to_string()).bind(org).bind(net).bind(action).bind(target.to_string()).bind(detail).execute(c).await?;
    Ok(())
}
#[derive(Deserialize)]
pub struct Page {
    limit: Option<i64>,
    before: Option<i64>,
}
pub async fn list(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Query(p): Query<Page>,
) -> Result<Json<Value>, ApiError> {
    crate::networks::access(&s, &h, net, false).await?;
    let rows=sqlx::query("SELECT id,at,actor_kind,actor_id,action,target,detail FROM audit_events WHERE network_id=$1 AND id<$2 ORDER BY id DESC LIMIT $3").bind(net).bind(p.before.unwrap_or(i64::MAX)).bind(p.limit.unwrap_or(100).clamp(1,500)).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<i64,_>("id"),"at":r.get::<chrono::DateTime<chrono::Utc>,_>("at"),"actor_kind":r.get::<String,_>("actor_kind"),"actor_id":r.get::<String,_>("actor_id"),"action":r.get::<String,_>("action"),"target":r.get::<String,_>("target"),"detail":r.get::<Value,_>("detail")})).collect::<Vec<_>>())))
}
