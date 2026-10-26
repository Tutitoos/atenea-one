// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::{AppState, api::ApiError};

fn unauthorized(code: &'static str) -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, code)
}

fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

// Rechazar cookies duplicadas evita interpretaciones distintas entre intermediarios.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for part in value.to_str().ok()?.split(';') {
            let Some((key, value)) = part.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if found.is_some() || value.is_empty() || value.len() > 512 {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}

fn set_cookie(
    state: &AppState,
    name: &str,
    value: &str,
    path: &str,
    max_age: u32,
) -> Result<HeaderValue, ApiError> {
    let secure = if state.config.dev_insecure_cookies {
        ""
    } else {
        "; Secure"
    };
    HeaderValue::from_str(&format!(
        "{name}={value}; HttpOnly; SameSite=Lax; Path={path}; Max-Age={max_age}{secure}"
    ))
    .map_err(|_| ApiError::internal())
}

fn private_response(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

async fn discover(state: &AppState) -> Result<(CoreProviderMetadata, reqwest::Client), ApiError> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| ApiError::internal())?;
    let issuer =
        IssuerUrl::new(state.config.oidc_issuer.clone()).map_err(|_| ApiError::internal())?;
    let metadata = CoreProviderMetadata::discover_async(issuer, &http)
        .await
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "oidc_unavailable"))?;
    Ok((metadata, http))
}

/// Tope de flujos OIDC vigentes; evita crecimiento ilimitado por llamadas anónimas.
const MAX_PENDING_FLOWS: i64 = 10_000;

/// Tope duro: recuento e inserción serializados en una transacción corta.
pub async fn insert_flow(
    pool: &sqlx::PgPool,
    state_hash: &[u8],
    verifier: &str,
    nonce: &str,
) -> Result<(), ApiError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(7311705)")
        .execute(&mut *tx)
        .await?;
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM oidc_flows WHERE expires_at > now()")
            .fetch_one(&mut *tx)
            .await?;
    if pending >= MAX_PENDING_FLOWS {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    sqlx::query(
        "INSERT INTO oidc_flows (state_hash, pkce_verifier, nonce, created_at, expires_at) \
         VALUES ($1, $2, $3, now(), now() + interval '10 minutes')",
    )
    .bind(state_hash)
    .bind(verifier)
    .bind(nonce)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Borra flujos OIDC y sesiones caducados (expiración absoluta, antigüedad 8 h o inactividad 1 h).
pub async fn purge(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM oidc_flows WHERE expires_at <= now()")
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM sessions WHERE expires_at <= now() \
         OR created_at <= now() - interval '8 hours' \
         OR last_seen_at <= now() - interval '1 hour'",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn login(
    State(state): State<AppState>,
    source: Option<axum::Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let ip = crate::client_ip(&state.config, source.map(|a| a.0.0.ip()), &headers);
    crate::take(&state.login_rate, ip, state.config.login_per_minute)?;
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM oidc_flows WHERE expires_at > now()")
            .fetch_one(&state.pool)
            .await?;
    if pending >= MAX_PENDING_FLOWS {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    let (metadata, _) = discover(&state).await?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(state.config.oidc_client_id.clone()),
        Some(ClientSecret::new(state.config.oidc_client_secret.clone())),
    )
    .set_redirect_uri(
        RedirectUrl::new(state.config.oidc_redirect_url.clone())
            .map_err(|_| ApiError::internal())?,
    );
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, flow_state, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            || CsrfToken::new_random_len(32),
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(challenge)
        .url();
    insert_flow(
        &state.pool,
        &hash(flow_state.secret()),
        verifier.secret(),
        nonce.secret(),
    )
    .await?;
    let mut response = private_response(Redirect::to(url.as_str()).into_response());
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie(
            &state,
            "atenea_oidc",
            flow_state.secret(),
            "/auth/callback",
            600,
        )?,
    );
    Ok(response)
}

#[derive(Deserialize)]
pub struct Callback {
    pub state: String,
    pub code: Option<String>,
    pub error: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<Callback>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let browser_state =
        cookie(&headers, "atenea_oidc").ok_or_else(|| unauthorized("invalid_oidc"))?;
    if hash(browser_state) != hash(&query.state) {
        return Err(unauthorized("invalid_oidc"));
    }
    // DELETE se confirma antes del intercambio: ni fallos ni carreras permiten reutilizar el flujo.
    let flow = sqlx::query(
        "DELETE FROM oidc_flows WHERE state_hash = $1 AND expires_at > now() \
         RETURNING pkce_verifier, nonce",
    )
    .bind(hash(&query.state).as_slice())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| unauthorized("invalid_oidc"))?;
    if query.error.is_some() {
        return Err(unauthorized("invalid_oidc"));
    }
    let code = query
        .code
        .filter(|code| !code.is_empty())
        .ok_or_else(|| unauthorized("invalid_oidc"))?;
    let (metadata, http) = discover(&state).await?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(state.config.oidc_client_id.clone()),
        Some(ClientSecret::new(state.config.oidc_client_secret.clone())),
    )
    .set_redirect_uri(
        RedirectUrl::new(state.config.oidc_redirect_url.clone())
            .map_err(|_| ApiError::internal())?,
    );
    let tokens = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|_| ApiError::internal())?
        .set_pkce_verifier(PkceCodeVerifier::new(flow.try_get("pkce_verifier")?))
        .request_async(&http)
        .await
        .map_err(|_| unauthorized("invalid_oidc"))?;
    let id_token = tokens
        .id_token()
        .ok_or_else(|| unauthorized("invalid_oidc"))?;
    let verifier = client.id_token_verifier();
    let nonce = Nonce::new(flow.try_get("nonce")?);
    // La biblioteca verifica firma/JWKS, issuer, audiencia, expiración y nonce.
    let claims = id_token
        .claims(&verifier, &nonce)
        .map_err(|_| unauthorized("invalid_oidc"))?;
    if let Some(expected) = claims.access_token_hash() {
        let actual = AccessTokenHash::from_token(
            tokens.access_token(),
            id_token
                .signing_alg()
                .map_err(|_| unauthorized("invalid_oidc"))?,
            id_token
                .signing_key(&verifier)
                .map_err(|_| unauthorized("invalid_oidc"))?,
        )
        .map_err(|_| unauthorized("invalid_oidc"))?;
        if actual != *expected {
            return Err(unauthorized("invalid_oidc"));
        }
    }
    let token = CsrfToken::new_random_len(32);
    let csrf = CsrfToken::new_random_len(32);
    let mut tx = state.pool.begin().await?;
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, display_name, created_at) \
         VALUES ($1, $2, $3, $4, $5, now()) \
         ON CONFLICT (oidc_issuer, oidc_subject) DO UPDATE \
         SET email = EXCLUDED.email, display_name = EXCLUDED.display_name RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(claims.issuer().as_str())
    .bind(claims.subject().as_str())
    .bind(claims.email().map(|email| email.as_str()))
    .bind(
        claims
            .name()
            .and_then(|name| name.get(None))
            .map(|name| name.as_str()),
    )
    .fetch_one(&mut *tx)
    .await?;
    // Una autenticación nueva rota también la sesión previa del navegador.
    if let Some(previous) = cookie(&headers, "atenea_session") {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(hash(previous).as_slice())
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO sessions \
         (id, user_id, csrf_hash, csrf_token, created_at, expires_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, now(), now() + interval '8 hours', now())",
    )
    .bind(hash(token.secret()).as_slice())
    .bind(user_id)
    .bind(hash(csrf.secret()).as_slice())
    .bind(csrf.secret())
    .execute(&mut *tx)
    .await?;
    let session_cookie = set_cookie(&state, "atenea_session", token.secret(), "/", 28800)?;
    let flow_cookie = set_cookie(&state, "atenea_oidc", "", "/auth/callback", 0)?;
    let mut response = private_response(Redirect::to(&state.config.public_url).into_response());
    tx.commit().await?;
    response
        .headers_mut()
        .append(header::SET_COOKIE, session_cookie);
    response
        .headers_mut()
        .append(header::SET_COOKIE, flow_cookie);
    Ok(response)
}

async fn authenticated(
    state: &AppState,
    headers: &HeaderMap,
    write: bool,
) -> Result<(Uuid, String), ApiError> {
    let token = cookie(headers, "atenea_session").ok_or_else(|| unauthorized("invalid_session"))?;
    let mut tx = state.pool.begin().await?;
    // El bloqueo de UPDATE serializa renovaciones y logout; un CSRF inválido revierte la renovación.
    let row = sqlx::query(
        "UPDATE sessions SET last_seen_at = now() \
         WHERE id = $1 AND expires_at > now() \
         AND created_at > now() - interval '8 hours' \
         AND last_seen_at > now() - interval '1 hour' \
         RETURNING user_id, csrf_hash, csrf_token",
    )
    .bind(hash(token).as_slice())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| unauthorized("invalid_session"))?;
    if write {
        let mut values = headers.get_all("x-csrf-token").iter();
        let csrf = values
            .next()
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty() && value.len() <= 512)
            .ok_or_else(|| ApiError::forbidden("csrf"))?;
        let expected: Vec<u8> = row.try_get("csrf_hash")?;
        if values.next().is_some() || hash(csrf).as_slice() != expected.as_slice() {
            return Err(ApiError::forbidden("csrf"));
        }
    }
    let result = (row.try_get("user_id")?, row.try_get("csrf_token")?);
    tx.commit().await?;
    Ok(result)
}

pub async fn session(state: &AppState, headers: &HeaderMap, write: bool) -> Result<Uuid, ApiError> {
    Ok(authenticated(state, headers, write).await?.0)
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    session(&state, &headers, true).await?;
    let token =
        cookie(&headers, "atenea_session").ok_or_else(|| unauthorized("invalid_session"))?;
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(hash(token).as_slice())
        .execute(&state.pool)
        .await?;
    let mut response = private_response(StatusCode::NO_CONTENT.into_response());
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie(&state, "atenea_session", "", "/", 0)?,
    );
    Ok(response)
}

pub async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (user_id, csrf_token) = authenticated(&state, &headers, false).await?;
    let user = sqlx::query("SELECT id, email, display_name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| unauthorized("invalid_session"))?;
    let memberships = sqlx::query(
        "SELECT org_id, role FROM memberships WHERE user_id = $1 ORDER BY org_id",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| -> Result<_, sqlx::Error> {
        Ok(json!({"org_id": row.try_get::<Uuid, _>("org_id")?, "role": row.try_get::<String, _>("role")?}))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(private_response(
        Json(json!({
            "user": {
                "id": user_id,
                "email": user.try_get::<Option<String>, _>("email")?,
                "display_name": user.try_get::<Option<String>, _>("display_name")?,
            },
            "csrf_token": csrf_token,
            "memberships": memberships,
        }))
        .into_response(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_reject_ambiguity_and_read_multiple_headers() {
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("other=value"));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("atenea_session=secret"),
        );
        assert_eq!(cookie(&headers, "atenea_session"), Some("secret"));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("atenea_session=other"),
        );
        assert_eq!(cookie(&headers, "atenea_session"), None);
    }
}
