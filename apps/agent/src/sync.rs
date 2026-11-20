// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Descarga acotada: solo `poll_config` decide si la configuración es de confianza.
use crate::config::SyncConfig;
use crate::platform::private_files::PrivateFiles;
use atenea_identity::{DeviceId, MAX_V3_BODY_LEN, SigningKey, api_auth::sign_request};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// Fichero firmado más grande posible (cuerpo v3 más cabecera, certificado y firma) con margen.
pub const RESPONSE_LIMIT: usize = MAX_V3_BODY_LEN + 512;

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub url: String,
    pub last_ok_unix: Option<u64>,
    pub last_error: Option<&'static str>,
    pub sync_ok: u64,
    pub sync_denied: u64,
    pub sync_errors: u64,
    pub report_errors: u64,
    pub route_announce_errors: u64,
}

impl Status {
    pub fn record(&mut self, result: Result<bool, &'static str>, unix: u64) {
        match result {
            Ok(_) => {
                self.sync_ok += 1;
                self.last_ok_unix = Some(unix);
                self.last_error = None;
            }
            Err(code) => {
                if code == "acceso denegado" {
                    self.sync_denied += 1;
                } else {
                    self.sync_errors += 1;
                }
                self.last_error = Some(code);
                tracing::warn!(code, "sincronización fallida");
            }
        }
    }
}

fn request(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
    nonce: [u8; 16],
) -> Result<(String, String), &'static str> {
    let url = cfg.endpoint().map_err(|_| "URL inválida")?;
    let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
    let path = uri.path_and_query().ok_or("URL inválida")?.as_str();
    let authorization = sign_request(key, device, "GET", path, &[], ts, nonce);
    Ok((url, authorization))
}

fn validate_body(body: &[u8]) -> Result<(), &'static str> {
    if body.len() > RESPONSE_LIMIT {
        return Err("respuesta demasiado grande");
    }
    if !body.starts_with(b"AOCF") {
        return Err("respuesta no es SignedConfig");
    }
    Ok(())
}

/// Comprobación completa de la configuración candidata (firma, serial, versión, reloj) contra el
/// estado vigente; el fichero en uso solo se sustituye si la acepta.
pub type Verify<'a> = &'a dyn Fn(&[u8]) -> Result<(), &'static str>;

fn write_changed(path: &Path, body: &[u8], verify: Verify) -> Result<bool, &'static str> {
    validate_body(body)?;
    match PrivateFiles::read(path) {
        Ok(current) if current == body => return Ok(false),
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        #[cfg(not(any(unix, windows)))]
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            return Err("ficheros privados no soportados: backend con DACL restrictiva pendiente");
        }
        Err(_) => return Err("no se pudo leer config_file"),
    }
    verify(body)?;
    PrivateFiles::replace(path, body)?;
    Ok(true)
}

pub fn fetch(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    path: &Path,
    ts: u64,
    verify: Verify,
) -> Result<bool, &'static str> {
    let mut nonce = [0; 16];
    nonce.copy_from_slice(&atenea_identity::generate_seed()[..16]);
    let (url, authorization) = request(cfg, key, device, ts, nonce)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("Authorization", &authorization)
        .call()
        .map_err(|e| crate::coord::transport_reason(&e))?;
    match response.status().as_u16() {
        200 => (),
        401 | 403 => return Err("acceso denegado"),
        _ => return Err("error HTTP del control plane"),
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "respuesta ilegible")?;
    write_changed(path, &bytes, verify)
}

#[derive(Serialize)]
pub struct Report {
    nat_mapping: Option<&'static str>,
    paths: Vec<ReportPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forwarding_ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dns: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    routes: Option<Vec<crate::netconf::RouteStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hostname: Option<String>,
    /// `Some([])` sustituye lo guardado por «sin servicios»; `None` no toca nada (ADR-024, D1).
    #[serde(skip_serializing_if = "Option::is_none")]
    services: Option<Vec<crate::services::PeerService>>,
    /// `Some` siempre que ya se detectó (incluso `[]`, que sustituye lo guardado); `None` no toca nada (ADR-025).
    #[serde(skip_serializing_if = "Option::is_none")]
    versions: Option<Vec<crate::versions::ComponentVersion>>,
}

impl Report {
    pub fn network(&mut self, status: &crate::netconf::Status) {
        self.forwarding_ok = status.forwarding_ok;
        self.dns = Some(status.dns);
        self.routes = Some(
            status
                .routes
                .iter()
                .map(|route| crate::netconf::RouteStatus {
                    reason: route.reason.as_deref().and_then(report_text),
                    ..route.clone()
                })
                .collect(),
        );
    }
}

#[derive(Serialize)]
struct ReportPath {
    peer: String,
    path: &'static str,
    last_rx_secs: Option<u64>,
    rtt_ms: Option<u32>,
    relay: Option<String>,
}

/// El control plane rechaza el informe ENTERO si un texto pasa de 255 bytes o lleva controles
/// (`apps/control-plane/src/panel.rs`). Los nombres de relay y los errores de las órdenes del sistema
/// vienen de fuera: se envía su primera línea con contenido, sin controles y recortada a ese límite.
fn report_text(raw: &str) -> Option<String> {
    let line = raw
        .split(char::is_control)
        .map(str::trim)
        .find(|s| !s.is_empty())?;
    let end = (0..=line.len().min(255))
        .rev()
        .find(|&i| line.is_char_boundary(i))
        .unwrap_or(0);
    Some(line[..end].to_owned())
}

/// Lista blanca: el estado IPC también contiene claves públicas y direcciones que no se envían.
/// `services` solo se informa con `publish = true`; apagado, el informe no cambia ni un byte.
pub fn build_report(
    status: &crate::engine::Status,
    relays: &crate::relay::RelayStatus,
    services: &crate::config::ServicesConfig,
) -> Option<Report> {
    if !status.config.lease_active {
        return None;
    }
    Some(Report {
        forwarding_ok: None,
        dns: None,
        routes: None,
        services: services
            .publish
            .then(|| services.published(&status.services)),
        hostname: status.hostname.clone(),
        versions: status.versions.clone(),
        nat_mapping: status.nat.as_ref().map(|nat| nat.mapping),
        paths: status
            .peers
            .iter()
            .map(|peer| {
                let id = atenea_crypto::hex::encode(&peer.device_id);
                ReportPath {
                    peer: format!(
                        "{}-{}-{}-{}-{}",
                        &id[..8],
                        &id[8..12],
                        &id[12..16],
                        &id[16..20],
                        &id[20..]
                    ),
                    path: peer.path,
                    last_rx_secs: peer.last_rx_secs,
                    rtt_ms: peer
                        .paths
                        .iter()
                        .find(|p| p.kind == peer.path)
                        .and_then(|p| p.rtt_ms),
                    relay: relays
                        .entries
                        .iter()
                        .find(|entry| {
                            entry.peer == id
                                && !entry.relay.is_empty()
                                && matches!(
                                    (peer.path, entry.transport),
                                    ("relay-udp", crate::relay::RelayKind::Udp)
                                        | ("relay-tls", crate::relay::RelayKind::Tls)
                                )
                        })
                        .and_then(|entry| report_text(&entry.relay)),
                }
            })
            .collect(),
    })
}

pub fn send_report(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    report: &Report,
    ts: u64,
) -> Result<(), &'static str> {
    post_json(
        cfg,
        key,
        device,
        "/api/v1/device/status?config_v3=true",
        serde_json::to_vec(report).map_err(|_| "informe inválido")?,
        ts,
    )
}

/// Metadatos de presentación (ADR-012/014/024): nunca autorizan ni enrutan.
#[derive(Clone, Default)]
pub struct PeerMetadata {
    pub names: HashMap<DeviceId, String>,
    pub roles: HashMap<DeviceId, &'static str>,
    pub services: HashMap<DeviceId, Vec<crate::services::PeerService>>,
    /// Última lista de versiones instaladas de cada par que ya informó (ADR-025).
    pub versions: HashMap<DeviceId, crate::versions::PeerVersions>,
}

/// 499 pares con nombre, rol, versiones y hasta 64 servicios cada uno caben con margen (ADR-031); es el mismo tope
/// que el de las respuestas IPC (ADR-011).
pub(crate) const METADATA_LIMIT: usize = 4 * 1024 * 1024;

pub fn fetch_peer_metadata(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
) -> Result<PeerMetadata, &'static str> {
    match get_metadata(cfg, key, device, ts, "/api/v1/device/peers")? {
        Some(body) => parse_peer_metadata(&body),
        None => Ok(PeerMetadata::default()),
    }
}

/// GET firmado y acotado compartido por los metadatos; 404 significa servidor anterior.
pub(crate) fn get_metadata(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
    endpoint: &str,
) -> Result<Option<Vec<u8>>, &'static str> {
    cfg.endpoint().map_err(|_| "URL inválida")?;
    let url = format!("{}{endpoint}", cfg.url.trim_end_matches('/'));
    let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
    let path = uri.path_and_query().ok_or("URL inválida")?.as_str();
    let mut nonce = [0; 16];
    nonce.copy_from_slice(&atenea_identity::generate_seed()[..16]);
    let authorization = sign_request(key, device, "GET", path, &[], ts, nonce);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("Authorization", &authorization)
        .call()
        .map_err(|e| crate::coord::transport_reason(&e))?;
    match response.status().as_u16() {
        200 => (),
        404 => return Ok(None),
        401 | 403 => return Err("acceso denegado"),
        _ => return Err("error HTTP del control plane"),
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((METADATA_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "respuesta ilegible")?;
    if bytes.len() > METADATA_LIMIT {
        return Err("respuesta demasiado grande");
    }
    Ok(Some(bytes))
}

/// Nombre, rol y cada servicio se validan por separado: una entrada mala no elimina las demás.
fn parse_peer_metadata(body: &[u8]) -> Result<PeerMetadata, &'static str> {
    #[derive(serde::Deserialize)]
    struct Entry {
        device_id: String,
        name: String,
        #[serde(default)]
        role: serde_json::Value,
        #[serde(default)]
        services: serde_json::Value,
        #[serde(default)]
        versions: serde_json::Value,
        #[serde(default)]
        versions_at: serde_json::Value,
    }
    if body.len() > METADATA_LIMIT {
        return Err("respuesta demasiado grande");
    }
    let entries: Vec<Entry> = serde_json::from_slice(body).map_err(|_| "nombres inválidos")?;
    let mut metadata = PeerMetadata::default();
    for e in entries {
        let Some(id) = atenea_crypto::hex::decode::<16>(&e.device_id.replace('-', "")) else {
            continue;
        };
        if let Some(name) = crate::names::accept(&e.name) {
            metadata.names.insert(id, name);
        }
        if let Some(list) = e.services.as_array() {
            let services: Vec<_> = list
                .iter()
                .filter_map(parse_service)
                .take(crate::services::MAX_SERVICES)
                .collect();
            if !services.is_empty() {
                metadata.services.insert(id, services);
            }
        }
        if let Some(versions) = parse_versions(&e.versions, &e.versions_at) {
            metadata.versions.insert(id, versions);
        }
        let role = match e.role.as_str() {
            Some("device") => "device",
            Some("server") => "server",
            _ => continue,
        };
        metadata.roles.insert(id, role);
    }
    Ok(metadata)
}

/// La lista de versiones de un par: cada entrada se valida por separado y se descartan las malas, las repetidas y las que
/// sobran; sin una fecha de informe válida no hay «informó» y el par queda «sin informar».
fn parse_versions(
    list: &serde_json::Value,
    at: &serde_json::Value,
) -> Option<crate::versions::PeerVersions> {
    use crate::versions::{
        Component, ComponentVersion, MAX_VERSIONS, valid_commit, valid_reported_at, valid_version,
    };
    let at = at.as_str().filter(|at| valid_reported_at(at))?;
    let mut parsed: Vec<ComponentVersion> = Vec::new();
    // Un servidor correcto manda como mucho tres; se revisa un margen fijo y se ignora el resto.
    for entry in list.as_array()?.iter().take(16) {
        let (Some(component), Some(version), Some(commit)) = (
            entry
                .get("component")
                .and_then(|v| v.as_str())
                .and_then(Component::parse),
            entry.get("version").and_then(|v| v.as_str()),
            entry.get("commit").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if valid_version(version)
            && valid_commit(commit)
            && parsed.iter().all(|p| p.component != component)
        {
            parsed.push(ComponentVersion {
                component,
                version: version.to_owned(),
                commit: commit.to_owned(),
            });
        }
        if parsed.len() == MAX_VERSIONS {
            break;
        }
    }
    Some(crate::versions::PeerVersions {
        list: parsed,
        at: at.to_owned(),
    })
}

/// Solo TCP con puerto 1..=65535; esquema `http` por defecto; etiqueta como un nombre.
fn parse_service(v: &serde_json::Value) -> Option<crate::services::PeerService> {
    use crate::services::{PeerService, Proto, Scheme};
    let port = u16::try_from(v.get("port")?.as_u64()?)
        .ok()
        .filter(|p| *p != 0)?;
    if v.get("proto")?.as_str()? != "tcp" {
        return None;
    }
    let scheme = match v.get("scheme").map(|s| s.as_str()) {
        None => Scheme::Http,
        Some(Some("http")) => Scheme::Http,
        Some(Some("https")) => Scheme::Https,
        Some(_) => return None,
    };
    let label = match v.get("label") {
        None | Some(serde_json::Value::Null) => None,
        Some(l) => Some(crate::names::accept(l.as_str()?)?),
    };
    Some(PeerService {
        port,
        proto: Proto::Tcp,
        scheme,
        label,
    })
}

pub fn declare_routes(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    routes: &[String],
    ts: u64,
) -> Result<(), &'static str> {
    let body =
        serde_json::to_vec(&serde_json::json!({"routes":routes})).map_err(|_| "rutas inválidas")?;
    post_json(cfg, key, device, "/api/v1/device/routes", body, ts)
}

fn post_json(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    endpoint: &str,
    body: Vec<u8>,
    ts: u64,
) -> Result<(), &'static str> {
    cfg.endpoint().map_err(|_| "URL inválida")?;
    let url = format!("{}{endpoint}", cfg.url.trim_end_matches('/'));
    let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
    let path = uri.path_and_query().ok_or("URL inválida")?.as_str();
    let mut nonce = [0; 16];
    nonce.copy_from_slice(&atenea_identity::generate_seed()[..16]);
    let authorization = sign_request(key, device, "POST", path, &body, ts, nonce);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let response = agent
        .post(&url)
        .header("Authorization", &authorization)
        .header("Content-Type", "application/json")
        .send(body.as_slice())
        .map_err(|e| crate::coord::transport_reason(&e))?;
    if response.status().as_u16() != 204 {
        return Err("error HTTP del control plane");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::io::Write;
    #[cfg(unix)]
    use std::net::TcpListener;
    #[test]
    fn report_uses_device_ids_and_only_public_report_fields() {
        use crate::engine::{ConfigStatus, NatStatus, PathStatus, PeerStatus};
        let mut status = crate::engine::Status {
            nat: Some(NatStatus {
                mapping: "dependent",
                reflexive: Some("198.51.100.1:1234".into()),
                candidates: vec!["192.0.2.1:1234".into()],
            }),
            public_key: "clave local".into(),
            addrs: vec!["10.0.0.9".into()],
            hostname: None,
            counters: Default::default(),
            config: ConfigStatus {
                version: 1,
                valid_until_unix: Some(300),
                lease_active: false,
                last_error: None,
                last_error_detail: None,
            },
            services: vec![crate::services::Service {
                port: 8080,
                proto: crate::services::Proto::Tcp,
            }],
            services_truncated: false,
            services_publish: false,
            versions: None,
            peers: ["direct", "relay-udp", "relay-tls", "probing", "none"]
                .into_iter()
                .map(|path| PeerStatus {
                    device_id: [0xab; 16],
                    public_key: "no es el identificador".into(),
                    addrs: vec!["10.0.0.8".into()],
                    // El informe es una lista blanca: el nombre del par no debe aparecer en él.
                    name: Some("nombre del par".into()),
                    role: Some("server"),
                    // Lo que informa el par tampoco debe aparecer en el informe.
                    versions: vec![crate::versions::ComponentVersion {
                        component: crate::versions::Component::Cli,
                        version: "9.9.9".into(),
                        commit: "unknown".into(),
                    }],
                    versions_at: Some("2026-11-16T10:20:30Z".into()),
                    probe: crate::engine::ProbeStatus::default(),
                    endpoint: Some("203.0.113.1:1234".into()),
                    session: Some("sesión".into()),
                    services: vec![crate::services::PeerService {
                        port: 9,
                        proto: crate::services::Proto::Tcp,
                        scheme: crate::services::Scheme::Http,
                        label: Some("del par".into()),
                    }],
                    last_handshake_secs_ago: Some(1),
                    tx_bytes: 123,
                    rx_bytes: 456,
                    path,
                    candidates: 1,
                    last_rx_secs: Some(3),
                    paths: vec![PathStatus {
                        kind: "relay-udp",
                        ready: true,
                        healthy: true,
                        last_rx_secs: Some(3),
                        rtt_ms: Some(20),
                    }],
                })
                .collect(),
        };
        let mut relays = crate::relay::RelayStatus::default();
        let off = crate::config::ServicesConfig::default();
        assert!(build_report(&status, &relays, &off).is_none());
        status.config.lease_active = true;
        let json = serde_json::to_value(build_report(&status, &relays, &off).unwrap()).unwrap();
        let paths: Vec<_> = ["direct", "relay-udp", "relay-tls", "probing", "none"]
            .into_iter()
            .map(|path| {
                serde_json::json!({
                    "peer": "abababab-abab-abab-abab-abababababab",
                    "path": path,
                    "last_rx_secs": 3,
                    "rtt_ms": if path == "relay-udp" { Some(20) } else { None },
                    "relay": null,
                })
            })
            .collect();
        assert_eq!(
            json,
            serde_json::json!({"nat_mapping": "dependent", "paths": paths})
        );
        relays.entries.push(crate::relay::AllocationStatus {
            peer: "ab".repeat(16),
            relay_id: "11".repeat(16),
            transport: crate::relay::RelayKind::Udp,
            relay: "sintético".into(),
            state: "ready".into(),
            peer_ready: true,
            rtt_ms: Some(20),
            pings_sent: 0,
            pings_lost: 0,
            tx_bytes: 0,
            tx_packets: 0,
            rx_bytes: 0,
            rx_packets: 0,
            drops: Default::default(),
            last_error: None,
            renew_at: u64::MAX,
        });
        let known = serde_json::to_value(build_report(&status, &relays, &off).unwrap()).unwrap();
        assert_eq!(known["paths"][1]["relay"], "sintético");
        assert!(known["paths"][0]["relay"].is_null());
        assert!(known["paths"][2]["relay"].is_null());
        status.nat = None;
        status.peers.clear();
        assert_eq!(
            serde_json::to_value(build_report(&status, &relays, &off).unwrap()).unwrap(),
            serde_json::json!({"nat_mapping": null, "paths": []})
        );
        status.hostname = Some("equipo de prueba".into());
        assert_eq!(
            serde_json::to_value(build_report(&status, &relays, &off).unwrap()).unwrap(),
            serde_json::json!({"nat_mapping": null, "paths": [], "hostname": "equipo de prueba"})
        );
    }

    #[test]
    fn report_texts_from_outside_fit_what_the_control_plane_accepts() {
        for (raw, expected) in [
            ("principal", Some("principal")),
            ("  eu-1\n", Some("eu-1")),
            ("error: x\nAt line:1 char:1\n+ ...", Some("error: x")),
            ("a\u{1b}[2Jb", Some("a")),
            ("\u{0}\n\t", None),
            ("", None),
        ] {
            assert_eq!(report_text(raw).as_deref(), expected, "{raw:?}");
        }
        // 150 `ñ` ocupan 300 bytes: se corta en un límite de carácter, sin pasar de 255.
        let long = report_text(&"ñ".repeat(150)).unwrap();
        assert_eq!((long.len(), long.chars().count()), (254, 127));

        // Por los dos caminos reales del informe: nombre de relay y motivo de una ruta.
        let mut status = services_status();
        status.peers.push(crate::engine::PeerStatus {
            device_id: [0xab; 16],
            public_key: String::new(),
            addrs: vec![],
            name: None,
            services: vec![],
            role: None,
            versions: vec![],
            versions_at: None,
            endpoint: None,
            session: None,
            last_handshake_secs_ago: None,
            tx_bytes: 0,
            rx_bytes: 0,
            path: "relay-udp",
            candidates: 0,
            last_rx_secs: None,
            paths: vec![],
            probe: Default::default(),
        });
        let mut relays = crate::relay::RelayStatus::default();
        relays.entries.push(crate::relay::AllocationStatus {
            peer: "ab".repeat(16),
            relay_id: "11".repeat(16),
            transport: crate::relay::RelayKind::Udp,
            relay: "x".repeat(300),
            state: "ready".into(),
            peer_ready: true,
            rtt_ms: None,
            pings_sent: 0,
            pings_lost: 0,
            tx_bytes: 0,
            tx_packets: 0,
            rx_bytes: 0,
            rx_packets: 0,
            drops: Default::default(),
            last_error: None,
            renew_at: u64::MAX,
        });
        let mut report = build_report(&status, &relays, &Default::default()).unwrap();
        report.network(&crate::netconf::Status {
            routes: vec![crate::netconf::RouteStatus {
                prefix: "192.0.2.0/24".into(),
                state: "conflict",
                reason: Some("ip route add: File exists\nsegunda línea".into()),
            }],
            dns: "none",
            forwarding_ok: None,
        });
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["paths"][0]["relay"], "x".repeat(255));
        assert_eq!(json["routes"][0]["reason"], "ip route add: File exists");
    }

    fn services_status() -> crate::engine::Status {
        use crate::services::{Proto, Service};
        crate::engine::Status {
            nat: None,
            public_key: String::new(),
            addrs: vec![],
            hostname: None,
            counters: Default::default(),
            config: crate::engine::ConfigStatus {
                version: 1,
                valid_until_unix: Some(300),
                lease_active: true,
                last_error: None,
                last_error_detail: None,
            },
            services: [22, 8080, 8443]
                .map(|port| Service {
                    port,
                    proto: Proto::Tcp,
                })
                .to_vec(),
            services_truncated: false,
            services_publish: false,
            versions: None,
            peers: vec![],
        }
    }

    #[test]
    fn report_without_publish_is_byte_identical_to_the_previous_format() {
        let report =
            build_report(&services_status(), &Default::default(), &Default::default()).unwrap();
        assert_eq!(
            serde_json::to_string(&report).unwrap(),
            r#"{"nat_mapping":null,"paths":[]}"#
        );
    }

    #[test]
    fn report_carries_the_detected_versions_and_nothing_before_the_first_detection() {
        use crate::versions::{Component, ComponentVersion};
        // Sin detección el informe no lleva el campo (no borra lo que el servidor ya tiene).
        let report =
            build_report(&services_status(), &Default::default(), &Default::default()).unwrap();
        assert!(
            serde_json::to_value(&report)
                .unwrap()
                .get("versions")
                .is_none()
        );
        let mut status = services_status();
        status.versions = Some(vec![
            ComponentVersion {
                component: Component::Agent,
                version: "0.1.31".into(),
                commit: "dfe72ae".into(),
            },
            ComponentVersion {
                component: Component::Cli,
                version: "0.1.31".into(),
                commit: "unknown".into(),
            },
        ]);
        let json = serde_json::to_value(
            build_report(&status, &Default::default(), &Default::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json["versions"],
            serde_json::json!([
                {"component":"agent","version":"0.1.31","commit":"dfe72ae"},
                {"component":"cli","version":"0.1.31","commit":"unknown"}
            ])
        );
        // Detectado y vacío: se envía `[]`, que sustituye lo guardado («no hay nada instalado que informar»).
        status.versions = Some(vec![]);
        let json = serde_json::to_value(
            build_report(&status, &Default::default(), &Default::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(json["versions"], serde_json::json!([]));
        // Sin lease no hay informe, versiones o no.
        status.config.lease_active = false;
        assert!(build_report(&status, &Default::default(), &Default::default()).is_none());
    }

    #[test]
    fn peer_versions_are_validated_per_entry_and_need_a_report_date() {
        let id = "11111111-1111-1111-1111-111111111111";
        let body = |versions: &str, at: &str| {
            format!(
                r#"[{{"device_id":"{id}","name":"par","role":"device","services":[],"versions":{versions},"versions_at":{at}}}]"#
            )
        };
        let key = [0x11; 16];
        let good = r#"[{"component":"agent","version":"0.1.31","commit":"dfe72ae"},{"component":"cli","version":"0.1.31","commit":"unknown"}]"#;
        let parsed =
            parse_peer_metadata(body(good, r#""2026-11-16T10:20:30Z""#).as_bytes()).unwrap();
        let versions = &parsed.versions[&key];
        assert_eq!(versions.at, "2026-11-16T10:20:30Z");
        assert_eq!(versions.list.len(), 2);
        assert_eq!(versions.list[1].commit, "unknown");
        // Un servidor anterior (sin los campos) o un par que nunca informó: sin entrada («sin informar»); el nombre y el
        // rol no se pierden.
        let old = format!(r#"[{{"device_id":"{id}","name":"par","role":"server","services":[]}}]"#);
        let parsed = parse_peer_metadata(old.as_bytes()).unwrap();
        assert!(parsed.versions.is_empty());
        assert_eq!(parsed.roles[&key], "server");
        assert!(
            parse_peer_metadata(body("[]", "null").as_bytes())
                .unwrap()
                .versions
                .is_empty()
        );
        // Informó sin nada instalado: entrada con lista vacía (distinto de «sin informar»).
        let empty =
            parse_peer_metadata(body("[]", r#""2026-11-16T10:20:30Z""#).as_bytes()).unwrap();
        assert!(empty.versions[&key].list.is_empty());
        // Fecha ajena a una marca de tiempo, o que no es texto: sin entrada.
        for at in [
            r#""ayer""#,
            r#""<script>2026-11-16T10:20:30Z""#,
            "1763288430",
            "null",
        ] {
            assert!(
                parse_peer_metadata(body(good, at).as_bytes())
                    .unwrap()
                    .versions
                    .is_empty(),
                "{at}"
            );
        }
        // Cada entrada se valida por separado: se descartan las malas, las repetidas y las que sobran de tres.
        let mixed = r#"[
            {"component":"agent","version":"0.1.31","commit":"dfe72ae"},
            {"component":"agent","version":"9.9.9","commit":"dfe72ae"},
            {"component":"relay","version":"1","commit":"unknown"},
            {"component":"cli","version":"0.1 31","commit":"unknown"},
            {"component":"cli","version":"1","commit":"ABCDEF0"},
            {"component":"desktop","version":"0.1.31"},
            "texto",
            {"component":"desktop","version":"0.1.31","commit":"abcdef0"}
        ]"#;
        let parsed =
            parse_peer_metadata(body(mixed, r#""2026-11-16T10:20:30Z""#).as_bytes()).unwrap();
        let list = &parsed.versions[&key].list;
        assert_eq!(
            list.iter()
                .map(|v| (v.component, v.version.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (crate::versions::Component::Agent, "0.1.31"),
                (crate::versions::Component::Desktop, "0.1.31")
            ]
        );
        // No es una lista: sin entrada.
        assert!(
            parse_peer_metadata(body(r#""agent""#, r#""2026-11-16T10:20:30Z""#).as_bytes())
                .unwrap()
                .versions
                .is_empty()
        );
    }

    #[test]
    fn report_publishes_services_with_exclude_label_and_scheme() {
        let cfg = |extra: &str| {
            crate::config::Config::parse(&format!(
                "{}\n{extra}",
                r#"interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "/s"
noise_key_file = "/n"
identity_key_file = "/i"
config_file = "/c"
state_dir = "/d"
network_id = "00112233445566778899aabbccddeeff"
device_id = "ffeeddccbbaa99887766554433221100"
trust_anchor = "0000000000000000000000000000000000000000000000000000000000000001"
"#
            ))
            .unwrap()
            .services
        };
        let status = services_status();
        let json = |c: &crate::config::ServicesConfig| {
            serde_json::to_string(&build_report(&status, &Default::default(), c).unwrap()).unwrap()
        };
        let all = cfg(r#"[services]
publish = true
exclude = [22]
[[services.entry]]
port = 8443
label = "  Panel  "
scheme = "https"
[[services.entry]]
port = 9999
label = "no escucha"
"#);
        assert_eq!(
            json(&all),
            r#"{"nat_mapping":null,"paths":[],"services":[{"port":8080,"proto":"tcp","scheme":"http"},{"port":8443,"proto":"tcp","scheme":"https","label":"Panel"}]}"#
        );
        // Sin servicios (o todos excluidos) se envía la lista vacía, que sustituye la anterior.
        let none = cfg("[services]\npublish = true\nexclude = [22, 8080, 8443]\n");
        assert_eq!(
            json(&none),
            r#"{"nat_mapping":null,"paths":[],"services":[]}"#
        );
        // Publicar apagado ignora exclude y entradas.
        let off = cfg("[services]\nexclude = [22]\n");
        assert_eq!(json(&off), r#"{"nat_mapping":null,"paths":[]}"#);
        // Sin lease no hay informe aunque se publique.
        let mut inactive = services_status();
        inactive.config.lease_active = false;
        assert!(build_report(&inactive, &Default::default(), &all).is_none());
    }

    #[test]
    fn peer_services_are_validated_per_entry() {
        use crate::services::{PeerService, Proto, Scheme};
        let long = "x".repeat(65);
        let many: Vec<_> = (1..=70)
            .map(|port| serde_json::json!({"port": port, "proto": "tcp"}))
            .collect();
        let body = serde_json::json!([
            {"device_id": "01".repeat(16), "name": "uno", "role": "server", "services": [
                {"port": 80, "proto": "tcp"},
                {"port": 443, "proto": "tcp", "scheme": "https", "label": "Web"},
                {"port": 0, "proto": "tcp"},
                {"port": 70000, "proto": "tcp"},
                {"port": 53, "proto": "udp"},
                {"port": 22, "proto": "tcp", "scheme": "ftp"},
                {"port": 23, "proto": "tcp", "label": "mal\u{202e}"},
                {"port": 24, "proto": "tcp", "label": long},
                {"port": 25, "proto": "tcp", "label": 7},
                {"port": 8080, "proto": "tcp", "label": " api ", "x": 1},
                "basura"
            ]},
            {"device_id": "02".repeat(16), "name": "dos"},
            {"device_id": "03".repeat(16), "name": "tres", "services": "no es lista"},
            {"device_id": "04".repeat(16), "name": "cuatro", "services": many},
            {"device_id": "05".repeat(16), "name": "cinco", "services": []}
        ]);
        let m = parse_peer_metadata(&serde_json::to_vec(&body).unwrap()).unwrap();
        let svc = |port, scheme, label: Option<&str>| PeerService {
            port,
            proto: Proto::Tcp,
            scheme,
            label: label.map(Into::into),
        };
        assert_eq!(
            m.services[&[1; 16]],
            [
                svc(80, Scheme::Http, None),
                svc(443, Scheme::Https, Some("Web")),
                svc(8080, Scheme::Http, Some("api")),
            ]
        );
        // Una entrada mala no tumba el nombre ni el rol del par.
        assert_eq!(
            (m.names[&[1; 16]].as_str(), m.roles[&[1; 16]]),
            ("uno", "server")
        );
        // Servidor anterior (sin campo), valor no lista o lista vacía: sin servicios.
        for id in [2, 3, 5] {
            assert!(!m.services.contains_key(&[id; 16]));
            assert_eq!(m.names.len(), 5);
        }
        assert_eq!(m.services[&[4; 16]].len(), crate::services::MAX_SERVICES);
    }

    #[test]
    fn peer_metadata_accepts_only_known_roles_independently_of_names() {
        let body = serde_json::json!([
            {"device_id": "01".repeat(16), "name": "uno", "role": "server"},
            {"device_id": "02".repeat(16), "name": "dos", "role": "device"},
            {"device_id": "03".repeat(16), "name": "tres", "role": "owner"},
            {"device_id": "04".repeat(16), "name": "cuatro"},
            {"device_id": "05".repeat(16), "name": "", "role": "server"},
            {"device_id": "06".repeat(16), "name": "seis", "role": 7}
        ]);
        let metadata = parse_peer_metadata(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(metadata.roles.len(), 3);
        assert_eq!(metadata.roles[&[1; 16]], "server");
        assert_eq!(metadata.roles[&[2; 16]], "device");
        assert_eq!(metadata.roles[&[5; 16]], "server");
        assert_eq!(metadata.names.len(), 5);
    }

    #[test]
    fn peer_names_keep_valid_entries_and_drop_hostile_or_malformed_ones() {
        let ok = "ab".repeat(16);
        let dashed = "12345678-1234-1234-1234-123456789abc";
        let body = serde_json::json!([
            {"device_id": ok, "name": "  portátil de Ana "},
            {"device_id": dashed, "name": "servidor"},
            {"device_id": "cd".repeat(16), "name": "falso\u{202e}nombre"},
            {"device_id": "ef".repeat(16), "name": ""},
            {"device_id": "no es un id", "name": "huérfano"},
            {"device_id": "01".repeat(16), "name": "x".repeat(65)},
        ])
        .to_string();
        let names = parse_peer_metadata(body.as_bytes()).unwrap().names;
        assert_eq!(names.len(), 2);
        assert_eq!(names[&[0xab; 16]], "portátil de Ana");
        let id = atenea_crypto::hex::decode::<16>("12345678123412341234123456789abc").unwrap();
        assert_eq!(names[&id], "servidor");
        assert!(parse_peer_metadata(br#"{"device_id":"x"}"#).is_err());
        assert!(parse_peer_metadata(&vec![b' '; METADATA_LIMIT + 1]).is_err());
    }
    fn cfg(url: &str) -> SyncConfig {
        SyncConfig {
            url: url.into(),
            interval_secs: 60,
            insecure_http: false,
        }
    }
    #[cfg(unix)]
    fn temp() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "atenea-sync-{}",
            atenea_crypto::hex::encode(&atenea_identity::generate_seed())
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn signed_request_and_body_limits() {
        let key = atenea_identity::signing_key([1; 32]);
        let (url, auth) = request(
            &cfg("https://example.test/prefix/"),
            &key,
            [2; 16],
            123,
            [3; 16],
        )
        .unwrap();
        assert_eq!(url, "https://example.test/prefix/api/v1/device/config");
        assert_eq!(
            auth,
            sign_request(
                &key,
                [2; 16],
                "GET",
                "/prefix/api/v1/device/config",
                &[],
                123,
                [3; 16]
            )
        );
        assert!(validate_body(b"bad").is_err());
        assert!(validate_body(&vec![0; RESPONSE_LIMIT + 1]).is_err());
        assert!(validate_body(b"AOCF").is_ok());
        assert!(cfg("https://user:secret@example.test").endpoint().is_err());
    }
    #[test]
    #[cfg(unix)]
    fn atomic_write_and_noop() {
        let dir = temp();
        let path = dir.join("config.bin");
        let accept: Verify = &|_| Ok(());
        assert_eq!(write_changed(&path, b"AOCFfirst", accept), Ok(true));
        let metadata = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(write_changed(&path, b"AOCFfirst", accept), Ok(false));
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), metadata);
        assert!(write_changed(&path, b"invalid", accept).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"AOCFfirst");
        assert_eq!(write_changed(&path, b"AOCFsecond", accept), Ok(true));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn unverified_body_never_replaces_the_active_config() {
        let dir = temp();
        let path = dir.join("config.bin");
        write_changed(&path, b"AOCFvalid", &|_| Ok(())).unwrap();
        let reject: Verify = &|_| Err("config_rollback");
        assert_eq!(
            write_changed(&path, b"AOCFold", reject),
            Err("config_rollback")
        );
        assert_eq!(fs::read(&path).unwrap(), b"AOCFvalid");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn local_http_success_denied_invalid() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (status, body) in [(200, "AOCFtest"), (403, "denied"), (200, "invalid")] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("GET /api/v1/device/config HTTP/1.1\r\n"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: atdevice v1 ")
                );
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let dir = temp();
        let path = dir.join("config.bin");
        let key = atenea_identity::signing_key([1; 32]);
        let mut status = Status::default();
        for _ in 0..3 {
            status.record(
                fetch(&cfg(&url), &key, [2; 16], &path, 123, &|_| Ok(())),
                123,
            );
        }
        assert_eq!(
            (status.sync_ok, status.sync_denied, status.sync_errors),
            (1, 1, 1)
        );
        assert_eq!(status.last_ok_unix, Some(123));
        assert_eq!(fs::read(&path).unwrap(), b"AOCFtest");
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn peer_metadata_http_keeps_roles_and_accepts_old_servers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let key = atenea_identity::signing_key([1; 32]);
        let public = key.verifying_key().to_bytes();
        let body = serde_json::json!([
            {"device_id": "02".repeat(16), "name": "equipo", "role": "server"},
            {"device_id": "03".repeat(16), "name": "anterior"}
        ])
        .to_string();
        let server = std::thread::spawn(move || {
            for (code, body) in [(200, body), (404, String::new())] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.starts_with("GET /api/v1/device/peers HTTP/1.1\r\n"));
                let auth = request
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("authorization")
                            .then_some(value.trim())
                    })
                    .unwrap();
                let signed = atenea_identity::api_auth::parse_authorization(auth).unwrap();
                atenea_identity::api_auth::verify_request(
                    &signed,
                    &public,
                    "GET",
                    "/api/v1/device/peers",
                    &[],
                )
                .unwrap();
                write!(stream, "HTTP/1.1 {code} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let peers = fetch_peer_metadata(&cfg(&url), &key, [1; 16], 123).unwrap();
        assert_eq!(peers.names[&[2; 16]], "equipo");
        assert_eq!(peers.roles, HashMap::from([([2; 16], "server")]));
        let old = fetch_peer_metadata(&cfg(&url), &key, [1; 16], 123).unwrap();
        assert!(old.names.is_empty() && old.roles.is_empty());
        server.join().unwrap();
    }
}
