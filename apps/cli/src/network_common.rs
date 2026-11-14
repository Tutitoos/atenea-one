// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Lo que `atenea network` comparte entre Unix y Windows: metadatos públicos de una red, resolución por
//! prefijo o nombre, confirmación interactiva y el examen de un código de unión (sin disco ni red).
// En Windows aún no se usan `confirm`, `now` ni `resolve`: los consumen `join`, `up`, `down` y `leave`
// (ADR-017 fase 2, siguientes entregas). Se retira este permiso cuando lleguen.
#![cfg_attr(windows, allow(dead_code))]
use crate::network_service::Service;
use atenea_crypto::hex;
use atenea_identity::anchor_fingerprint;
use atenea_identity::join_code::JoinCode;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// Metadatos NO secretos de una red unida (`network.json`). Nunca contienen el token.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Meta {
    pub network_id: String,
    pub name: String,
    pub server: String,
    pub url: String,
    pub root_anchor_fingerprint: String,
    pub device_id: String,
    pub device_fingerprint: String,
    pub interface: String,
    pub listen_port: u16,
    /// Segundos Unix.
    pub created: u64,
    /// `pendiente_de_aprobacion` o `aprobada` (lo que el CLI supo al unirse).
    pub join_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_backend: Option<Service>,
}

/// Id corto de una red (últimos 8 hex del UUID): los UUIDv7 comparten el prefijo temporal.
pub fn short(id: &str) -> &str {
    &id[28..]
}

pub fn uuid(b: &[u8; 16]) -> String {
    let h = hex::encode(b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

pub fn is_uuid_name(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// Id completo, prefijo único (≥ 4 caracteres) o nombre único.
pub fn resolve<'a>(metas: &'a [Meta], query: &str) -> Result<&'a Meta, String> {
    let q = query.trim().to_lowercase();
    if let Some(m) = metas.iter().find(|m| m.network_id == q) {
        return Ok(m);
    }
    let found: Vec<&Meta> = metas
        .iter()
        .filter(|m| {
            (q.len() >= 4 && (m.network_id.starts_with(&q) || m.network_id.ends_with(&q)))
                || m.name.to_lowercase() == q
        })
        .collect();
    match found.as_slice() {
        [m] => Ok(m),
        [] => Err(format!(
            "ninguna red coincide con «{query}» (usa `atenea network list`)"
        )),
        many => Err(format!(
            "«{query}» es ambiguo; candidatas:\n{}",
            many.iter()
                .map(|m| format!("  {} {}", m.network_id, m.name))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

pub fn confirm(
    prompt: &str,
    yes: bool,
    interactive: bool,
    input: &mut dyn BufRead,
) -> Result<(), String> {
    if yes {
        return Ok(());
    }
    if !interactive {
        return Err("hace falta confirmación y no hay terminal interactiva: revisa los datos y repite con --yes".into());
    }
    print!("{prompt} [escribe «sí» para continuar]: ");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    if matches!(
        line.trim().to_lowercase().as_str(),
        "sí" | "si" | "s" | "yes" | "y"
    ) {
        Ok(())
    } else {
        Err("cancelado: no se ha cambiado nada".into())
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn server_of(url: &str) -> String {
    url.strip_prefix("https://")
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default()
        .into()
}

#[derive(Serialize)]
pub struct ConnectivityInfo {
    pub stun_host: String,
    pub relay_ca_sha256: Option<String>,
}

#[derive(Serialize)]
pub struct CodeInfo<'a> {
    pub name: &'a str,
    pub server: String,
    pub network_id: String,
    pub root_fingerprint: String,
    pub has_token: bool,
    /// Lo que el código anuncia para llegar al STUN y al relay (ADR-018); `null` si no lleva bloque.
    pub connectivity: Option<ConnectivityInfo>,
}

pub fn code_info(code: &JoinCode) -> CodeInfo<'_> {
    CodeInfo {
        name: &code.name,
        server: server_of(&code.url),
        network_id: uuid(&code.network_id),
        root_fingerprint: anchor_fingerprint(&code.root_anchor),
        has_token: code.token.is_some(),
        connectivity: code.connectivity.as_ref().map(|c| ConnectivityInfo {
            stun_host: c.stun_host.clone(),
            relay_ca_sha256: c.relay_ca_sha256.as_ref().map(|h| hex::encode(h)),
        }),
    }
}

pub fn write_code_info(info: &CodeInfo<'_>, output: &mut dyn Write) -> Result<(), String> {
    writeln!(
        output,
        "Red:            {}\nServidor:       {}\nId de la red:   {}\nHuella de la raíz: {}",
        info.name, info.server, info.network_id, info.root_fingerprint
    )
    .map_err(|e| e.to_string())?;
    if let Some(c) = &info.connectivity {
        let ca = match &c.relay_ca_sha256 {
            Some(h) => format!("SHA-256 {}…", &h[..16]),
            None => "pública (el código no lleva hash)".into(),
        };
        writeln!(
            output,
            "Conectividad:   STUN {} (puertos 3478 y 3479); CA del relay: {ca}",
            c.stun_host
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn inspect(text: &str, json: bool, output: &mut dyn Write) -> Result<(), String> {
    let code = JoinCode::decode(text).map_err(|e| format!("código no válido: {e}"))?;
    let info = code_info(&code);
    if json {
        writeln!(
            output,
            "{}",
            serde_json::to_string(&info).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())
    } else {
        write_code_info(&info, output)?;
        writeln!(output, "Lleva token:    {}", info.has_token).map_err(|e| e.to_string())
    }
}

/// Estado de una red según su socket de estado.
pub enum State {
    Stopped,
    NoConfig,
    Connected,
    Conflict(String),
    Unknown(String),
}

impl State {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Stopped => "detenida",
            Self::NoConfig => "iniciada_sin_configuracion",
            Self::Connected => "conectada",
            Self::Conflict(_) => "conflicto",
            Self::Unknown(_) => "desconocida",
        }
    }
    pub fn text(&self) -> String {
        match self {
            Self::Stopped => "detenida".into(),
            Self::NoConfig => "iniciada, sin configuración".into(),
            Self::Connected => "conectada".into(),
            Self::Conflict(d) => format!("conflicto: {d}"),
            Self::Unknown(d) => format!("desconocida ({d})"),
        }
    }
}

/// Interpreta la respuesta JSON de `status` de un agente que contestó.
pub fn state_from_status(text: &str) -> State {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let cfg = &v["config"];
    if matches!(
        cfg["last_error"].as_str(),
        Some("network_overlap" | "dns_domain_conflict")
    ) {
        State::Conflict(format!(
            "{}: {}",
            cfg["last_error"].as_str().unwrap_or("conflicto"),
            cfg["last_error_detail"]
                .as_str()
                .unwrap_or("solapa con otra red o ruta local")
        ))
    } else if cfg["version"].as_u64().unwrap_or(0) > 0 {
        State::Connected
    } else {
        State::NoConfig
    }
}
