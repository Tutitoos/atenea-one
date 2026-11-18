// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Pertenencia por red e invitaciones por enlace (ADR-028 D6 y D7). Invitar a una red no da acceso a la
//! organización: `viewer` solo lee y `enroller` además genera códigos y aprueba (con la huella completa).
use crate::{AppState, api::ApiError, audit, auth, networks};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

/// Validez de una invitación (D7).
const INVITATION_DAYS: i64 = 7;

fn valid_role(role: &str) -> Result<(), ApiError> {
    if matches!(role, "viewer" | "enroller") {
        Ok(())
    } else {
        Err(ApiError::bad("invalid_role"))
    }
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[derive(Deserialize)]
pub struct NewInvitation {
    role: String,
}

/// Crea un enlace de un solo uso. Solo el propietario de la red; el token se devuelve una vez y se guarda su hash.
pub async fn create_invitation(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<NewInvitation>,
) -> Result<(StatusCode, [(&'static str, &'static str); 2], Json<Value>), ApiError> {
    let (u, org) = networks::access_as(&s, &h, net, true, "owner").await?;
    auth::limit_session(&s, &h, "invitation", 20)?;
    valid_role(&b.role)?;
    let token = format!(
        "atinv_{}",
        URL_SAFE_NO_PAD.encode(atenea_identity::generate_seed())
    );
    let (id, expires) = (
        Uuid::now_v7(),
        chrono::Utc::now() + chrono::Duration::days(INVITATION_DAYS),
    );
    let mut tx = s.pool.begin().await?;
    // La fila de la red serializa el conteo y la inserción entre invitaciones simultáneas.
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM network_invitations \
         WHERE network_id=$1 AND used_at IS NULL AND revoked_at IS NULL AND expires_at>now()",
    )
    .bind(net)
    .fetch_one(&mut *tx)
    .await?;
    if open >= i64::from(s.config.max_open_invitations) {
        return Err(ApiError::conflict("invitation_limit"));
    }
    sqlx::query(
        "INSERT INTO network_invitations (id, network_id, token_hash, role, created_by, expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(net)
    .bind(token_hash(&token).as_slice())
    .bind(&b.role)
    .bind(u)
    .bind(expires)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "invitation.create",
        id,
        json!({"role": b.role}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "audit");
    Ok((
        StatusCode::CREATED,
        crate::devices::TOKEN_HEADERS,
        Json(json!({"id": id, "token": token, "role": b.role, "expires_at": expires})),
    ))
}

/// Invitaciones recientes de la red, sin el token (solo existe su hash).
pub async fn list_invitations(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    networks::access_as(&s, &h, net, false, "owner").await?;
    let rows = sqlx::query(
        "SELECT id, role, created_at, expires_at, used_at, revoked_at FROM network_invitations \
         WHERE network_id=$1 ORDER BY created_at DESC, id DESC LIMIT 100",
    )
    .bind(net)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!(
        rows.iter()
            .map(|r| {
                let at = |name: &str| r.get::<Option<chrono::DateTime<chrono::Utc>>, _>(name);
                json!({
                    "id": r.get::<Uuid, _>("id"),
                    "role": r.get::<String, _>("role"),
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                    "expires_at": r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at"),
                    "used_at": at("used_at"),
                    "revoked_at": at("revoked_at"),
                })
            })
            .collect::<Vec<_>>()
    )))
}

/// Anula una invitación que aún no se ha usado.
pub async fn revoke_invitation(
    State(s): State<AppState>,
    Path((net, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (u, org) = networks::access_as(&s, &h, net, true, "owner").await?;
    let mut tx = s.pool.begin().await?;
    let revoked = sqlx::query(
        "UPDATE network_invitations SET revoked_at=now() \
         WHERE id=$1 AND network_id=$2 AND used_at IS NULL AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(net)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if revoked != 1 {
        return Err(ApiError::not_found());
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "invitation.revoke",
        id,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "audit");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Accept {
    token: String,
}

/// Canjea una invitación con la cuenta de la sesión. Inexistente, caducada, anulada o ya usada responden igual
/// (`404`). Si la cuenta ya tiene acceso a la red no cambia nada y la invitación sigue vigente (`409`).
pub async fn accept(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(b): Json<Accept>,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, true).await?;
    auth::limit_session(&s, &h, "invitation_accept", 10)?;
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT i.id, i.network_id, i.role, n.org_id FROM network_invitations i \
         JOIN networks n ON n.id = i.network_id \
         WHERE i.token_hash=$1 AND i.used_at IS NULL AND i.revoked_at IS NULL AND i.expires_at>now() \
         FOR UPDATE OF i",
    )
    .bind(token_hash(&b.token).as_slice())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(ApiError::not_found)?;
    let (id, net, role, org): (Uuid, Uuid, String, Uuid) = (
        row.get("id"),
        row.get("network_id"),
        row.get("role"),
        row.get("org_id"),
    );
    let has_access: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE org_id=$1 AND user_id=$2) \
         OR EXISTS(SELECT 1 FROM network_members WHERE network_id=$3 AND user_id=$2)",
    )
    .bind(org)
    .bind(u)
    .bind(net)
    .fetch_one(&mut *tx)
    .await?;
    if has_access {
        return Err(ApiError::conflict("already_member"));
    }
    let inviter: Option<Uuid> =
        sqlx::query_scalar("SELECT created_by FROM network_invitations WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query(
        "INSERT INTO network_members (network_id, user_id, role, invited_by) VALUES ($1,$2,$3,$4)",
    )
    .bind(net)
    .bind(u)
    .bind(&role)
    .bind(inviter)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE network_invitations SET used_at=now(), used_by=$2 WHERE id=$1")
        .bind(id)
        .bind(u)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network_member.join",
        u,
        json!({"role": role, "invitation": id}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "audit");
    Ok(Json(json!({"network_id": net, "role": role})))
}

/// Miembros por invitación de la red (no los de la organización). Sin correos: solo el nombre visible.
pub async fn list_members(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    networks::access_as(&s, &h, net, false, "owner").await?;
    let rows = sqlx::query(
        "SELECT m.user_id, m.role, m.created_at, u.display_name FROM network_members m \
         JOIN users u ON u.id = m.user_id WHERE m.network_id=$1 ORDER BY m.created_at, m.user_id",
    )
    .bind(net)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!(
        rows.iter()
            .map(|r| json!({
                "user_id": r.get::<Uuid, _>("user_id"),
                "role": r.get::<String, _>("role"),
                "display_name": r.get::<Option<String>, _>("display_name"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            }))
            .collect::<Vec<_>>()
    )))
}

#[derive(Deserialize)]
pub struct Role {
    role: String,
}

pub async fn set_member(
    State(s): State<AppState>,
    Path((net, user)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(b): Json<Role>,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = networks::access_as(&s, &h, net, true, "owner").await?;
    valid_role(&b.role)?;
    let mut tx = s.pool.begin().await?;
    let previous: Option<String> = sqlx::query_scalar(
        "UPDATE network_members SET role=$3 WHERE network_id=$1 AND user_id=$2 \
         RETURNING (SELECT role FROM network_members WHERE network_id=$1 AND user_id=$2)",
    )
    .bind(net)
    .bind(user)
    .bind(&b.role)
    .fetch_optional(&mut *tx)
    .await?;
    let previous = previous.ok_or_else(ApiError::not_found)?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network_member.set",
        user,
        json!({"previous_role": previous, "role": b.role}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "audit");
    Ok(Json(json!({"user_id": user, "role": b.role})))
}

/// Quita a un miembro. No revoca los dispositivos que aprobó: la auditoría dice quién aprobó cada uno y el
/// propietario decide.
pub async fn remove_member(
    State(s): State<AppState>,
    Path((net, user)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (u, org) = networks::access_as(&s, &h, net, true, "owner").await?;
    let mut tx = s.pool.begin().await?;
    let removed = sqlx::query("DELETE FROM network_members WHERE network_id=$1 AND user_id=$2")
        .bind(net)
        .bind(user)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if removed != 1 {
        return Err(ApiError::not_found());
    }
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network_member.remove",
        user,
        json!({}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "audit");
    Ok(StatusCode::NO_CONTENT)
}

/// Redes a las que la cuenta pertenece por invitación, con su rol. No incluye las de sus organizaciones.
pub async fn shared_networks(
    State(s): State<AppState>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, false).await?;
    let rows = sqlx::query(&format!(
        "{} WHERE id IN (SELECT network_id FROM network_members WHERE user_id=$1) ORDER BY id",
        networks::SELECT
    ))
    .bind(u)
    .fetch_all(&s.pool)
    .await?;
    let roles: std::collections::HashMap<Uuid, String> =
        sqlx::query("SELECT network_id, role FROM network_members WHERE user_id=$1")
            .bind(u)
            .fetch_all(&s.pool)
            .await?
            .iter()
            .map(|r| (r.get("network_id"), r.get("role")))
            .collect();
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut network = networks::value(row)?;
        let id: Uuid = row.get("id");
        network["role"] = json!(roles.get(&id));
        out.push(network);
    }
    Ok(Json(Value::Array(out)))
}
