// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-018 § 3.4: lectura **interna** de las raíces de las redes para el coordinator. Se sirve en un listener
//! distinto del público (no publicado por Caddy/HAProxy ni por Compose) y con un secreto Bearer compartido por
//! fichero; el router público no contiene esta ruta. Sin configuración nada de esto existe.
use crate::{AppState, api::ApiError, config::Config};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::os::unix::fs::PermissionsExt;

/// Dirección del listener interno y secreto ya validados.
#[derive(Debug)]
pub struct Access {
    pub listen: String,
    pub secret: String,
}

impl Access {
    /// `internal_listen` e `internal_secret_file` van juntos o no va ninguno: una mitad impide arrancar.
    pub fn load(config: &Config) -> Result<Option<Self>, &'static str> {
        let (listen, file) = match (&config.internal_listen, &config.internal_secret_file) {
            (None, None) => return Ok(None),
            (Some(l), Some(f)) => (l, f),
            _ => return Err("internal_listen e internal_secret_file deben configurarse juntos"),
        };
        listen
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "internal_listen inválido")?;
        let f = std::fs::File::open(file).map_err(|_| "no se puede abrir internal_secret_file")?;
        let meta = f
            .metadata()
            .map_err(|_| "no se puede inspeccionar internal_secret_file")?;
        if !meta.is_file() || !matches!(meta.permissions().mode() & 0o777, 0o400 | 0o600) {
            return Err("internal_secret_file requiere fichero regular 0400 o 0600");
        }
        let text =
            std::io::read_to_string(f).map_err(|_| "no se puede leer internal_secret_file")?;
        let secret = text.trim();
        // Mismo mínimo que exige el coordinator a su lado del secreto.
        if secret.len() < 16 || !secret.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("internal_secret_file: secreto de al menos 16 caracteres imprimibles");
        }
        Ok(Some(Self {
            listen: listen.clone(),
            secret: secret.to_string(),
        }))
    }
}

#[derive(Clone)]
struct Guard {
    state: AppState,
    /// Se compara el hash, no el secreto: el tiempo de la comparación no revela prefijos.
    secret_hash: [u8; 32],
}

pub fn router(state: AppState, secret: &str) -> Router {
    Router::new()
        .route("/internal/v1/root-anchors", get(root_anchors))
        .with_state(Guard {
            state,
            secret_hash: Sha256::digest(secret.as_bytes()).into(),
        })
}

async fn root_anchors(
    State(g): State<Guard>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let given: [u8; 32] = Sha256::digest(given.as_bytes()).into();
    if given != g.secret_hash {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    // Cada raíz va con su red: el coordinator solo la acepta para esa red (ADR-018 § 4).
    let rows = sqlx::query("SELECT id, root_anchor FROM networks ORDER BY id")
        .fetch_all(&g.state.pool)
        .await?;
    let anchors: Vec<_> = rows
        .iter()
        .map(|r| {
            json!({
                "network_id": hex::encode(r.get::<uuid::Uuid, _>("id").as_bytes()),
                "root_anchor": hex::encode(r.get::<&[u8], _>("root_anchor")),
            })
        })
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "anchors": anchors })),
    ))
}
