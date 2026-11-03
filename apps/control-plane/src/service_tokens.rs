// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Tokens de servicio de solo lectura (ADR-020). La autenticación del token vive en `auth.rs`.
use crate::{AppState, api::ApiError, audit, auth, orgs};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use openidconnect::CsrfToken;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct NewServiceToken {
    name: String,
    ttl_secs: u32,
}

pub async fn create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<NewServiceToken>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let u = auth::session(&s, &h, true).await?;
    orgs::role(&s, u, org, "owner").await?;
    auth::limit_session(&s, &h, "service_token", 10)?;
    orgs::name(&b.name)?;
    if !(3600..=7_776_000).contains(&b.ttl_secs) {
        return Err(ApiError::bad("invalid_ttl"));
    }
    // 256 bits del RNG del sistema; solo se guarda el SHA-256.
    let token = format!("atsvc_{}", CsrfToken::new_random_len(32).secret());
    let (id, user) = (Uuid::now_v7(), Uuid::now_v7());
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO users (id, kind, display_name) VALUES ($1, 'service', $2)")
        .bind(user)
        .bind(&b.name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships (org_id, user_id, role) VALUES ($1, $2, 'member')")
        .bind(org)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    let expires_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "INSERT INTO service_tokens (id, user_id, org_id, name, token_hash, created_by, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7)) RETURNING expires_at",
    )
    .bind(id)
    .bind(user)
    .bind(org)
    .bind(&b.name)
    .bind(Sha256::digest(token.as_bytes()).as_slice())
    .bind(u)
    .bind(f64::from(b.ttl_secs))
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        None,
        "service_token.create",
        id,
        json!({"name": b.name, "expires_at": expires_at}),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "token": token, "expires_at": expires_at})),
    ))
}

pub async fn list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, false).await?;
    orgs::role(&s, u, org, "owner").await?;
    let rows = sqlx::query(
        "SELECT id, name, created_at, expires_at, last_used_at, revoked_at \
         FROM service_tokens WHERE org_id = $1 ORDER BY created_at, id",
    )
    .bind(org)
    .fetch_all(&s.pool)
    .await?;
    let items = rows
        .iter()
        .map(|r| -> Result<Value, sqlx::Error> {
            Ok(json!({
                "id": r.try_get::<Uuid, _>("id")?,
                "name": r.try_get::<String, _>("name")?,
                "created_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
                "expires_at": r.try_get::<chrono::DateTime<chrono::Utc>, _>("expires_at")?,
                "last_used_at": r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at")?,
                "revoked_at": r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("revoked_at")?,
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Value::Array(items)))
}

pub async fn revoke(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    orgs::role(&s, u, org, "owner").await?;
    let mut tx = s.pool.begin().await?;
    let revoked = sqlx::query(
        "UPDATE service_tokens SET revoked_at = now() \
         WHERE id = $1 AND org_id = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(org)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if revoked == 0 {
        return Err(ApiError::not_found());
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        None,
        "service_token.revoke",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
