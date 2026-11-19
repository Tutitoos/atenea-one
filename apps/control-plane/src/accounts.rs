// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Cuentas propias (ADR-028 D2 a D5): registro con correo y contraseña, inicio de sesión, recuperación con
//! códigos de un solo uso y organización personal. Una cuenta local es un `users` con `oidc_issuer = 'local'` y
//! `oidc_subject` = el correo en minúsculas; su correo no está verificado y nunca llega a `users.email`.
use crate::{AppState, api::ApiError, audit, auth};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use openidconnect::CsrfToken;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::OnceLock};
use uuid::Uuid;

/// Emisor de las identidades locales en `users.oidc_issuer`. No es una URL: ningún proveedor OIDC puede colisionar.
const LOCAL: &str = "local";
/// Suelo del mínimo configurable (`Config::password_min_chars`, 12 por defecto): un valor menor se sube a 8.
pub const FLOOR_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_BYTES: usize = 128;
const RECOVERY_CODES: usize = 10;
/// Fallos seguidos tras los que la cuenta se bloquea; el bloqueo se duplica por fallo hasta `MAX_LOCK_SECS`.
const LOCK_AFTER: i32 = 5;
const MAX_LOCK_SECS: f64 = 900.;

fn invalid_credentials() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials")
}

fn limited() -> ApiError {
    ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited")
}

/// Correo en minúsculas, sin espacios ni controles, con un único `@` y un dominio con punto.
fn normalize_login(raw: &str) -> Result<String, ApiError> {
    let login = raw.to_lowercase();
    let valid = (3..=254).contains(&login.len())
        && !login.chars().any(|c| c.is_whitespace() || c.is_control())
        && login.split('@').count() == 2
        && login.split_once('@').is_some_and(|(user, domain)| {
            !user.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if valid {
        Ok(login)
    } else {
        Err(ApiError::bad("invalid_login"))
    }
}

fn check_password(password: &str, min_chars: usize) -> Result<(), ApiError> {
    if password.chars().count() < min_chars.max(FLOOR_PASSWORD_CHARS)
        || password.len() > MAX_PASSWORD_BYTES
    {
        Err(ApiError::bad("invalid_password"))
    } else {
        Ok(())
    }
}

/// Hash argon2id con los parámetros por defecto de la biblioteca (19 MiB, 2 pasadas, 1 hilo). Trabaja en un hilo
/// de bloqueo y con un máximo de hashes simultáneos: sin tope, una inundación de registros agotaría CPU y memoria.
async fn hash_password(state: &AppState, password: String) -> Result<String, ApiError> {
    let _permit = state.hash_gate.try_acquire().map_err(|_| limited())?;
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(|_| ApiError::internal())
}

/// Con `None` verifica contra un hash falso: el tiempo no distingue un correo desconocido de una contraseña mala.
async fn verify_password(
    state: &AppState,
    stored: Option<String>,
    password: String,
) -> Result<bool, ApiError> {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let _permit = state.hash_gate.try_acquire().map_err(|_| limited())?;
    tokio::task::spawn_blocking(move || {
        let known = stored.is_some();
        let hash = stored.unwrap_or_else(|| {
            DUMMY
                .get_or_init(|| {
                    Argon2::default()
                        .hash_password(b"sin cuenta", &SaltString::generate(&mut OsRng))
                        .map(|hash| hash.to_string())
                        .unwrap_or_default()
                })
                .clone()
        });
        let Ok(parsed) = PasswordHash::new(&hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
            && known
    })
    .await
    .map_err(|_| ApiError::internal())
}

fn code_hash(code: &str) -> [u8; 32] {
    Sha256::digest(code.as_bytes()).into()
}

/// IP cliente agrupada y cupo por IP más cupo global (ADR-028 D4).
fn limit_registration(
    state: &AppState,
    source: Option<ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let ip = crate::client_ip(&state.config, source.map(|a| a.0.ip()), headers);
    crate::take(
        &state.register_rate,
        crate::ip_group(ip),
        state.config.register_per_minute,
    )?;
    crate::take(
        &state.register_global_rate,
        (),
        state.config.register_global_per_minute,
    )
}

fn limit_login(
    state: &AppState,
    source: Option<ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let ip = crate::client_ip(&state.config, source.map(|a| a.0.ip()), headers);
    crate::take(
        &state.login_rate,
        crate::ip_group(ip),
        state.config.login_per_minute,
    )
}

/// Crea la organización personal de la cuenta una sola vez (ADR-028 D5). `personal_org_done` evita recrearla si
/// el usuario la borra después, y la actualización condicional serializa dos inicios de sesión simultáneos.
/// Los usuarios internos de los tokens de servicio no tienen organización propia.
pub async fn ensure_personal(
    tx: &mut sqlx::PgConnection,
    user: Uuid,
    display_name: Option<&str>,
) -> Result<(), ApiError> {
    let pending: Option<bool> = sqlx::query_scalar(
        "UPDATE users SET personal_org_done = true \
         WHERE id = $1 AND kind = 'oidc' AND personal_org_done = false RETURNING true",
    )
    .bind(user)
    .fetch_optional(&mut *tx)
    .await?;
    if pending.is_none() {
        return Ok(());
    }
    let name = display_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(
            || "Mi organización".to_owned(),
            |n| {
                format!(
                    "Organización de {}",
                    n.chars().take(200).collect::<String>()
                )
            },
        );
    let org = Uuid::now_v7();
    sqlx::query("INSERT INTO organizations (id, name, personal) VALUES ($1, $2, true)")
        .bind(org)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships VALUES ($1, $2, 'owner')")
        .bind(org)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    audit::record(
        tx,
        "user",
        user,
        Some(org),
        None,
        "org.create",
        org,
        json!({"personal": true}),
    )
    .await
}

#[derive(Deserialize)]
pub struct Register {
    login: String,
    password: String,
    display_name: Option<String>,
}

pub async fn register(
    State(state): State<AppState>,
    source: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<Register>,
) -> Result<Response, ApiError> {
    if !state.config.registration_open {
        return Err(ApiError::forbidden("registration_closed"));
    }
    limit_registration(&state, source.map(|e| e.0), &headers)?;
    let login = normalize_login(&body.login)?;
    check_password(&body.password, state.config.password_min_chars)?;
    let display = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if let Some(name) = display {
        crate::orgs::name(name)?;
    }
    let password_hash = hash_password(&state, body.password).await?;
    let user = Uuid::new_v4();
    let codes: Vec<String> = (0..RECOVERY_CODES)
        .map(|_| CsrfToken::new_random_len(16).secret().clone())
        .collect();
    let mut tx = state.pool.begin().await?;
    let created: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, display_name, created_at) \
         VALUES ($1, $2, $3, NULL, $4, now()) ON CONFLICT (oidc_issuer, oidc_subject) DO NOTHING RETURNING id",
    )
    .bind(user)
    .bind(LOCAL)
    .bind(&login)
    .bind(display)
    .fetch_optional(&mut *tx)
    .await?;
    if created.is_none() {
        return Err(ApiError::conflict("conflict"));
    }
    sqlx::query(
        "INSERT INTO local_credentials (user_id, login, password_hash) VALUES ($1, $2, $3)",
    )
    .bind(user)
    .bind(&login)
    .bind(&password_hash)
    .execute(&mut *tx)
    .await?;
    for code in &codes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user)
            .bind(code_hash(code).as_slice())
            .execute(&mut *tx)
            .await?;
    }
    ensure_personal(&mut tx, user, display).await?;
    let cookie = auth::start_session(&state, &mut tx, &headers, user).await?;
    tx.commit().await?;
    let mut response = auth::private_response(
        (
            StatusCode::CREATED,
            Json(json!({"user_id": user, "recovery_codes": codes})),
        )
            .into_response(),
    );
    response.headers_mut().append(header::SET_COOKIE, cookie);
    Ok(response)
}

#[derive(Deserialize)]
pub struct LocalLogin {
    login: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    source: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<LocalLogin>,
) -> Result<Response, ApiError> {
    limit_login(&state, source.map(|e| e.0), &headers)?;
    // Un correo mal formado no puede existir; se responde igual que una contraseña errónea.
    let login = normalize_login(&body.login).map_err(|_| invalid_credentials())?;
    let row: Option<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT user_id, password_hash, coalesce(locked_until > now(), false) \
         FROM local_credentials WHERE login = $1",
    )
    .bind(&login)
    .fetch_optional(&state.pool)
    .await?;
    if row.as_ref().is_some_and(|r| r.2) {
        return Err(limited());
    }
    let user = row.as_ref().map(|r| r.0);
    let ok = verify_password(&state, row.map(|r| r.1), body.password).await?;
    let Some(user) = user.filter(|_| ok) else {
        // Atómico: dos intentos simultáneos no se pisan el contador.
        sqlx::query(
            "UPDATE local_credentials SET failed_attempts = failed_attempts + 1, \
             locked_until = CASE WHEN failed_attempts + 1 >= $2 THEN now() + \
             make_interval(secs => least($3, 30 * power(2, failed_attempts + 1 - $2))) \
             ELSE locked_until END WHERE login = $1",
        )
        .bind(&login)
        .bind(LOCK_AFTER)
        .bind(MAX_LOCK_SECS)
        .execute(&state.pool)
        .await?;
        return Err(invalid_credentials());
    };
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE local_credentials SET failed_attempts = 0, locked_until = NULL WHERE user_id = $1",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    let cookie = auth::start_session(&state, &mut tx, &headers, user).await?;
    tx.commit().await?;
    let mut response = auth::private_response(StatusCode::NO_CONTENT.into_response());
    response.headers_mut().append(header::SET_COOKIE, cookie);
    Ok(response)
}

#[derive(Deserialize)]
pub struct Recover {
    login: String,
    recovery_code: String,
    new_password: String,
}

/// Cambia la contraseña con un código de recuperación (un solo uso) y cierra todas las sesiones de la cuenta.
/// No inicia sesión: el usuario entra con la contraseña nueva.
pub async fn recover(
    State(state): State<AppState>,
    source: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<Recover>,
) -> Result<StatusCode, ApiError> {
    limit_login(&state, source.map(|e| e.0), &headers)?;
    let login = normalize_login(&body.login).map_err(|_| invalid_credentials())?;
    check_password(&body.new_password, state.config.password_min_chars)?;
    // Se hace el hash antes de buscar la cuenta: el tiempo no revela si existe.
    let password_hash = hash_password(&state, body.new_password).await?;
    let mut tx = state.pool.begin().await?;
    let user: Option<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM local_credentials WHERE login = $1 FOR UPDATE")
            .bind(&login)
            .fetch_optional(&mut *tx)
            .await?;
    let user = user.ok_or_else(invalid_credentials)?;
    let used = sqlx::query(
        "UPDATE recovery_codes SET used_at = now() \
         WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
    )
    .bind(user)
    .bind(code_hash(body.recovery_code.trim()).as_slice())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if used != 1 {
        return Err(invalid_credentials());
    }
    sqlx::query(
        "UPDATE local_credentials SET password_hash = $2, failed_attempts = 0, locked_until = NULL \
         WHERE user_id = $1",
    )
    .bind(user)
    .bind(&password_hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lo único que la pantalla de acceso necesita saber antes de iniciar sesión: si ofrece «Crear una cuenta».
pub async fn options(State(state): State<AppState>) -> impl IntoResponse {
    (
        [("cache-control", "no-store")],
        Json(json!({"registration_open": state.config.registration_open})),
    )
}

/// Clave pública del emisor del servidor, para que el usuario firme con su raíz el certificado de su red
/// (ADR-028 D1). Es pública (también va en el certificado); se exige sesión para no regalarla al anonimato.
pub async fn issuer(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    auth::session(&state, &headers, false).await?;
    auth::limit_session(&state, &headers, "issuer", 60)?;
    Ok((
        [("cache-control", "no-store")],
        Json(json!({"issuer_pubkey": hex::encode(state.issuer.verifying_key().to_bytes())})),
    ))
}
