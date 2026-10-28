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
    let can_see_email: bool = sqlx::query_scalar(
        "SELECT role IN ('owner','admin') FROM memberships WHERE org_id=$1 AND user_id=$2",
    )
    .bind(org)
    .bind(u)
    .fetch_one(&s.pool)
    .await?;
    let rows = sqlx::query("SELECT m.user_id,m.role,u.display_name,u.email FROM memberships m JOIN users u ON u.id=m.user_id WHERE m.org_id=$1 ORDER BY m.user_id")
        .bind(org)
        .fetch_all(&s.pool)
        .await?;
    Ok(Json(json!(
        rows.iter()
            .map(|r| member_value(r, can_see_email))
            .collect::<Vec<_>>()
    )))
}

fn member_value(r: &sqlx::postgres::PgRow, can_see_email: bool) -> Value {
    json!({
        "user_id":r.get::<Uuid,_>("user_id"),
        "role":r.get::<String,_>("role"),
        "display_name":r.get::<Option<String>,_>("display_name"),
        "email":if can_see_email {r.get::<Option<String>,_>("email")} else {None},
    })
}

#[derive(Deserialize)]
pub struct AddMember {
    email: String,
    role: String,
}

pub async fn add_member(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(body): Json<AddMember>,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    role(&s, u, org, "owner").await?;
    auth::limit_session(&s, &h, "add_member", 10)?;
    validate_role(&body.role)?;
    let users: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE lower(email)=lower($1) LIMIT 2")
            .bind(&body.email)
            .fetch_all(&s.pool)
            .await?;
    let user = match users.as_slice() {
        [user] => *user,
        [] => return Err(ApiError::not_found()),
        _ => return Err(ApiError::conflict("conflict")),
    };
    write_member(&s, u, org, user, &body.role).await
}

fn validate_role(role: &str) -> Result<(), ApiError> {
    if !["owner", "admin", "member"].contains(&role) {
        return Err(ApiError::bad("invalid_role"));
    }
    Ok(())
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
    validate_role(&body.role)?;
    write_member(&s, u, org, user, &body.role).await
}

async fn write_member(
    s: &AppState,
    actor: Uuid,
    org: Uuid,
    user: Uuid,
    role: &str,
) -> Result<Json<Value>, ApiError> {
    let mut tx = s.pool.begin().await?;
    if role != "owner" {
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
    let previous_role: Option<String> =
        sqlx::query_scalar("SELECT role FROM memberships WHERE org_id=$1 AND user_id=$2")
            .bind(org)
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?;
    sqlx::query("INSERT INTO memberships VALUES($1,$2,$3) ON CONFLICT(org_id,user_id) DO UPDATE SET role=excluded.role").bind(org).bind(user).bind(role).execute(&mut *tx).await?;
    audit::record(
        &mut tx,
        "user",
        actor,
        Some(org),
        None,
        "member.set",
        user,
        json!({"previous_role":previous_role,"role":role}),
    )
    .await?;
    let row = sqlx::query("SELECT m.user_id,m.role,u.display_name,u.email FROM memberships m JOIN users u ON u.id=m.user_id WHERE m.org_id=$1 AND m.user_id=$2")
        .bind(org).bind(user).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(member_value(&row, true)))
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

pub async fn rename(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(body): Json<Name>,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    role(&s, u, org, "owner").await?;
    name(&body.name)?;
    let mut tx = s.pool.begin().await?;
    let previous_name: String =
        sqlx::query_scalar("SELECT name FROM organizations WHERE id=$1 FOR UPDATE")
            .bind(org)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("UPDATE organizations SET name=$2 WHERE id=$1")
        .bind(org)
        .bind(&body.name)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        None,
        "org.rename",
        org,
        json!({"previous_name":previous_name,"name":body.name}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":org,"name":body.name})))
}
