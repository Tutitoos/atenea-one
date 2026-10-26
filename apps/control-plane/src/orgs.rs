// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, api::ApiError, audit, auth};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
pub async fn role(s: &AppState, user: Uuid, org: Uuid, required: &str) -> Result<(), ApiError> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM memberships WHERE org_id=$1 AND user_id=$2")
            .bind(org)
            .bind(user)
            .fetch_optional(&s.pool)
            .await?;
    let role = role.ok_or_else(ApiError::not_found)?;
    if required == "owner" && role != "owner" || required == "admin" && role == "member" {
        return Err(ApiError::forbidden("forbidden"));
    }
    Ok(())
}
#[derive(Deserialize)]
pub struct Name {
    pub name: String,
}
pub fn name(n: &str) -> Result<(), ApiError> {
    if n.trim().is_empty() || n.len() > 255 {
        Err(ApiError::bad("invalid_name"))
    } else {
        Ok(())
    }
}
pub async fn create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(body): Json<Name>,
) -> Result<(axum::http::StatusCode, Json<Value>), ApiError> {
    let u = auth::session(&s, &h, true).await?;
    name(&body.name)?;
    let id = Uuid::now_v7();
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO organizations(id,name) VALUES($1,$2)")
        .bind(id)
        .bind(&body.name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships VALUES($1,$2,'owner')")
        .bind(id)
        .bind(u)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(id),
        None,
        "org.create",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"id":id,"name":body.name})),
    ))
}
pub async fn list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, false).await?;
    let rows=sqlx::query("SELECT o.id,o.name,m.role FROM organizations o JOIN memberships m ON m.org_id=o.id WHERE m.user_id=$1 ORDER BY o.id").bind(u).fetch_all(&s.pool).await?;
    Ok(Json(Value::Array(rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"role":r.get::<String,_>("role")})).collect())))
}
pub async fn members(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, false).await?;
    role(&s, u, org, "member").await?;
    let rows = sqlx::query("SELECT user_id,role FROM memberships WHERE org_id=$1 ORDER BY user_id")
        .bind(org)
        .fetch_all(&s.pool)
        .await?;
    Ok(Json(json!(
        rows.iter()
            .map(|r| json!({"user_id":r.get::<Uuid,_>("user_id"),"role":r.get::<String,_>("role")}))
            .collect::<Vec<_>>()
    )))
}
#[derive(Deserialize)]
pub struct Role {
    role: String,
}
async fn protect_owner(c: &mut PgConnection, org: Uuid, user: Uuid) -> Result<(), ApiError> {
    sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
        .bind(org)
        .execute(&mut *c)
        .await?;
    let owners: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memberships WHERE org_id=$1 AND role='owner'")
            .bind(org)
            .fetch_one(&mut *c)
            .await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT role FROM memberships WHERE org_id=$1 AND user_id=$2")
            .bind(org)
            .bind(user)
            .fetch_optional(&mut *c)
            .await?;
    if owners <= 1 && old.as_deref() == Some("owner") {
        return Err(ApiError::conflict("last_owner"));
    }
    Ok(())
}
pub async fn set_member(
    State(s): State<AppState>,
    Path((org, user)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(body): Json<Role>,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    role(&s, u, org, "owner").await?;
    if !["owner", "admin", "member"].contains(&body.role.as_str()) {
        return Err(ApiError::bad("invalid_role"));
    }
    let mut tx = s.pool.begin().await?;
    if body.role != "owner" {
        protect_owner(&mut tx, org, user).await?;
    } else {
        sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
            .bind(org)
            .execute(&mut *tx)
            .await?;
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(ApiError::not_found());
    }
    sqlx::query("INSERT INTO memberships VALUES($1,$2,$3) ON CONFLICT(org_id,user_id) DO UPDATE SET role=excluded.role").bind(org).bind(user).bind(&body.role).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        None,
        "member.set",
        user,
        json!({"role":body.role}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"user_id":user,"role":body.role})))
}
pub async fn remove_member(
    State(s): State<AppState>,
    Path((org, user)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    role(&s, u, org, "owner").await?;
    let mut tx = s.pool.begin().await?;
    protect_owner(&mut tx, org, user).await?;
    sqlx::query("DELETE FROM memberships WHERE org_id=$1 AND user_id=$2")
        .bind(org)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        None,
        "member.set",
        user,
        json!({"removed":true}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({})))
}
pub async fn remove(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM memberships WHERE org_id=$1 AND user_id=$2")
            .bind(org)
            .bind(u)
            .fetch_optional(&mut *tx)
            .await?;
    match role.as_deref() {
        None => return Err(ApiError::not_found()),
        Some("owner") => {}
        _ => return Err(ApiError::forbidden("forbidden")),
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        None,
        "org.delete",
        org,
        json!({}),
    )
    .await?;
    sqlx::query("DELETE FROM organizations WHERE id=$1")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
