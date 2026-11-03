// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Lectura de las raíces de las redes desde el control plane (ADR-018 § 3.4).
use crate::config::{AnchorsSource, parse_anchor};
use atenea_identity::PublicKey;
use serde::Deserialize;
use std::time::Duration;

const MAX_BODY: u64 = 512 * 1024;
const MAX_ANCHORS: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    anchors: Vec<String>,
}

/// Consulta bloqueante. Cualquier fallo (red, estado, JSON o una sola ancla inválida) descarta la respuesta entera:
/// quien llama conserva la última lista buena.
pub fn fetch(source: &AnchorsSource) -> Result<Vec<PublicKey>, &'static str> {
    let secret = source.secret()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&source.url)
        .header("Authorization", &format!("Bearer {secret}"))
        .call()
        .map_err(|_| "el control plane no responde")?;
    if response.status().as_u16() != 200 {
        return Err("el control plane rechazó la consulta de anclas");
    }
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|_| "respuesta de anclas ilegible o demasiado grande")?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<PublicKey>, &'static str> {
    let reply: Reply = serde_json::from_str(text).map_err(|_| "respuesta de anclas inválida")?;
    if reply.anchors.len() > MAX_ANCHORS {
        return Err("demasiadas anclas");
    }
    reply.anchors.iter().map(|s| parse_anchor(s)).collect()
}
