// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, auth, devices, issue, networks, orgs, policies, service_tokens};
use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use std::time::Duration;
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str);
impl ApiError {
    pub fn bad(c: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, c)
    }
    pub fn forbidden(c: &'static str) -> Self {
        Self(StatusCode::FORBIDDEN, c)
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found")
    }
    pub fn internal() -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, "internal")
    }
    pub fn conflict(c: &'static str) -> Self {
        Self(StatusCode::CONFLICT, c)
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        atenea_observability::event(atenea_observability::Event::DatabaseError, 1);
        if e.as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            Self::conflict("conflict")
        } else {
            tracing::error!("Fallo de base de datos");
            Self::internal()
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(serde_json::json!({"error":{"code":self.1,"message":self.1}})),
        )
            .into_response()
    }
}
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
pub fn router(s: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", post(auth::logout))
        .route("/api/v1/session", get(auth::get_session))
        .route("/api/v1/orgs", get(orgs::list).post(orgs::create))
        .route(
            "/api/v1/orgs/{org}",
            delete(orgs::remove).patch(orgs::rename),
        )
        .route(
            "/api/v1/orgs/{org}/members",
            get(orgs::members).post(orgs::add_member),
        )
        .route(
            "/api/v1/orgs/{org}/members/{user}",
            put(orgs::set_member).delete(orgs::remove_member),
        )
        .route(
            "/api/v1/orgs/{org}/service-tokens",
            get(service_tokens::list).post(service_tokens::create),
        )
        .route(
            "/api/v1/orgs/{org}/service-tokens/{id}",
            delete(service_tokens::revoke),
        )
        .route(
            "/api/v1/orgs/{org}/networks",
            get(networks::list).post(networks::create),
        )
        .route(
            "/api/v1/networks/{net}",
            get(networks::get)
                .delete(networks::remove)
                .patch(networks::rename),
        )
        .route(
            "/api/v1/networks/{net}/issuer-cert",
            put(networks::certificate),
        )
        .route(
            "/api/v1/networks/{net}/enrollment-tokens",
            post(devices::token),
        )
        .route("/api/v1/networks/{net}/devices", get(devices::list))
        .route(
            "/api/v1/networks/{net}/join-policy",
            put(networks::set_join_policy),
        )
        .route(
            "/api/v1/networks/{net}/join-code",
            get(networks::join_code_get),
        )
        .route(
            "/api/v1/networks/{net}/devices/{dev}/approve",
            post(devices::approve),
        )
        .route(
            "/api/v1/networks/{net}/devices/{dev}/revoke",
            post(devices::revoke),
        )
        .route(
            "/api/v1/networks/{net}/devices/{dev}",
            patch(devices::update),
        )
        .route(
            "/api/v1/networks/{net}/policies",
            get(policies::list).post(policies::create),
        )
        .route(
            "/api/v1/networks/{net}/policies/{id}",
            delete(policies::remove),
        )
        .route("/api/v1/networks/{net}/audit", get(crate::audit::list))
        .route("/api/v1/enroll", post(devices::enroll))
        .route(
            "/api/v1/networks/{net}/join-requests",
            post(devices::request_join),
        )
        .route("/api/v1/device/config", get(issue::get))
        .route("/api/v1/device/peers", get(issue::peers))
        .route("/api/v1/server/info", get(issue::server_info))
        .route("/api/v1/server", get(server))
        .route("/api/v1/relay-ca", get(crate::connectivity::relay_ca))
        .route(
            "/api/v1/device/leave",
            post(devices::leave).layer(axum::extract::DefaultBodyLimit::max(1024)),
        )
        .route("/api/v1/device/routes", post(crate::routes::declare))
        .route("/api/v1/networks/{net}/routes", get(crate::routes::list))
        .route(
            "/api/v1/networks/{net}/routes/{id}/approve",
            post(crate::routes::approve),
        )
        .route(
            "/api/v1/networks/{net}/routes/{id}/reject",
            post(crate::routes::reject),
        )
        .route(
            "/api/v1/networks/{net}/dns",
            get(crate::dns::get).put(crate::dns::set),
        )
        .route(
            "/api/v1/device/status",
            post(crate::panel::status).layer(axum::extract::DefaultBodyLimit::max(65536)),
        )
        .route("/api/v1/networks/{net}/events", get(crate::panel::events))
        .route("/api/v1/networks/{net}/metrics", get(crate::panel::metrics))
        .route("/api/openapi.json", get(|| async { Json(openapi()) }))
        .fallback(crate::panel::static_content)
        .layer(axum::middleware::from_fn_with_state(
            s.clone(),
            crate::panel::changed,
        ))
        .layer(axum::middleware::from_fn(read_only_token))
        .layer(axum::middleware::from_fn(uniform_errors))
        .layer(axum::middleware::from_fn_with_state(
            REQUEST_TIMEOUT,
            request_timeout,
        ))
        .layer(axum::middleware::from_fn_with_state(
            s.metrics.clone(),
            atenea_observability::record,
        ))
        .with_state(s)
}
async fn health(
    axum::extract::State(s): axum::extract::State<AppState>,
) -> Result<StatusCode, ApiError> {
    sqlx::query("SELECT 1").execute(&s.pool).await?;
    Ok(StatusCode::OK)
}

async fn server(
    axum::extract::State(s): axum::extract::State<AppState>,
    h: axum::http::HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let user = auth::session(&s, &h, false).await?;
    let member: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memberships WHERE user_id=$1)")
            .bind(user)
            .fetch_one(&s.pool)
            .await?;
    if !member {
        return Err(ApiError::forbidden("forbidden"));
    }
    auth::limit_session(&s, &h, "server", 60)?;
    Ok((
        [("cache-control", "no-store")],
        Json(serde_json::json!({
            "control_plane": {
                "version": env!("CARGO_PKG_VERSION"),
                "commit": option_env!("ATENEA_BUILD_COMMIT").unwrap_or("unknown"),
            },
            "server_time": chrono::Utc::now().to_rfc3339(),
        })),
    ))
}
pub fn openapi() -> utoipa::openapi::OpenApi {
    crate::openapi::document()
}
/// ADR-020: un token de servicio solo lee. Cualquier método de escritura, también `/device/*` y
/// `/enroll`, se rechaza antes de tocar la base.
async fn read_only_token(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if !req.method().is_safe() && auth::bearer_service(req.headers()) {
        return ApiError::forbidden("service_token_read_only").into_response();
    }
    next.run(req).await
}
async fn uniform_errors(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let report = req.uri().path() == "/api/v1/device/status";
    let leave = req.uri().path() == "/api/v1/device/leave";
    let response = next.run(req).await;
    if report && response.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::bad("invalid_report").into_response();
    }
    if leave && response.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::bad("invalid_request").into_response();
    }
    if (response.status().is_client_error() || response.status().is_server_error())
        && response
            .headers()
            .get("content-type")
            .is_none_or(|v| v != "application/json")
    {
        return ApiError(response.status(), "invalid_request").into_response();
    }
    response
}
/// Plazo de una petición, cuerpo incluido: sin él, un cliente lento retiene la conexión y su
/// permiso de base de datos. El WebSocket solo lo agota hasta responder el `101`: la sesión
/// vive en otra tarea.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
async fn request_timeout(
    axum::extract::State(limit): axum::extract::State<Duration>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(response) => response,
        Err(_) => ApiError(StatusCode::REQUEST_TIMEOUT, "request_timeout").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn peticion_lenta_se_corta_con_408_json() {
        let app = Router::new()
            .route("/lenta", get(std::future::pending::<&'static str>))
            .layer(axum::middleware::from_fn_with_state(
                Duration::from_millis(50),
                request_timeout,
            ));
        let request = axum::http::Request::builder()
            .uri("/lenta")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["code"], "request_timeout");
    }
}
