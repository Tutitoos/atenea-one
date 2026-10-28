// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Descarga acotada: solo `poll_config` decide si la configuración es de confianza.
use crate::config::SyncConfig;
use crate::platform::private_files::PrivateFiles;
use atenea_identity::{DeviceId, MAX_BODY_LEN, SigningKey, api_auth::sign_request};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

pub const RESPONSE_LIMIT: usize = MAX_BODY_LEN + 512;

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
        .map_err(|_| "error de red")?;
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
}

impl Report {
    pub fn network(&mut self, status: &crate::netconf::Status) {
        self.forwarding_ok = status.forwarding_ok;
        self.dns = Some(status.dns);
        self.routes = Some(status.routes.clone());
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

/// Lista blanca: el estado IPC también contiene claves públicas y direcciones que no se envían.
pub fn build_report(
    status: &crate::engine::Status,
    relays: &crate::relay::RelayStatus,
) -> Option<Report> {
    if !status.config.lease_active {
        return None;
    }
    Some(Report {
        forwarding_ok: None,
        dns: None,
        routes: None,
        hostname: status.hostname.clone(),
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
                        .map(|entry| entry.relay.clone()),
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
        "/api/v1/device/status",
        serde_json::to_vec(report).map_err(|_| "informe inválido")?,
        ts,
    )
}

/// Nombres de visualización de los pares (ADR-012). Tope de respuesta propio: son solo texto.
const NAMES_LIMIT: usize = 256 * 1024;

/// `Ok(vacío)` si el control plane es anterior y no tiene el endpoint. Un fallo aquí no afecta al
/// estado de la sincronización: los nombres son solo para mostrar.
pub fn fetch_peer_names(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
) -> Result<HashMap<DeviceId, String>, &'static str> {
    cfg.endpoint().map_err(|_| "URL inválida")?;
    let url = format!("{}/api/v1/device/peers", cfg.url.trim_end_matches('/'));
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
        .map_err(|_| "error de red")?;
    match response.status().as_u16() {
        200 => (),
        404 => return Ok(HashMap::new()),
        401 | 403 => return Err("acceso denegado"),
        _ => return Err("error HTTP del control plane"),
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((NAMES_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "respuesta ilegible")?;
    parse_peer_names(&bytes)
}

/// Cada entrada se valida por separado: un nombre inválido se descarta sin perder los demás.
fn parse_peer_names(body: &[u8]) -> Result<HashMap<DeviceId, String>, &'static str> {
    #[derive(serde::Deserialize)]
    struct Entry {
        device_id: String,
        name: String,
    }
    if body.len() > NAMES_LIMIT {
        return Err("respuesta demasiado grande");
    }
    let entries: Vec<Entry> = serde_json::from_slice(body).map_err(|_| "nombres inválidos")?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            let id = atenea_crypto::hex::decode::<16>(&e.device_id.replace('-', ""))?;
            Some((id, crate::names::accept(&e.name)?))
        })
        .collect())
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
        .map_err(|_| "error de red")?;
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
            peers: ["direct", "relay-udp", "relay-tls", "probing", "none"]
                .into_iter()
                .map(|path| PeerStatus {
                    device_id: [0xab; 16],
                    public_key: "no es el identificador".into(),
                    addrs: vec!["10.0.0.8".into()],
                    // El informe es una lista blanca: el nombre del par no debe aparecer en él.
                    name: Some("nombre del par".into()),
                    endpoint: Some("203.0.113.1:1234".into()),
                    session: Some("sesión".into()),
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
        assert!(build_report(&status, &relays).is_none());
        status.config.lease_active = true;
        let json = serde_json::to_value(build_report(&status, &relays).unwrap()).unwrap();
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
        let known = serde_json::to_value(build_report(&status, &relays).unwrap()).unwrap();
        assert_eq!(known["paths"][1]["relay"], "sintético");
        assert!(known["paths"][0]["relay"].is_null());
        assert!(known["paths"][2]["relay"].is_null());
        status.nat = None;
        status.peers.clear();
        assert_eq!(
            serde_json::to_value(build_report(&status, &relays).unwrap()).unwrap(),
            serde_json::json!({"nat_mapping": null, "paths": []})
        );
        status.hostname = Some("equipo de prueba".into());
        assert_eq!(
            serde_json::to_value(build_report(&status, &relays).unwrap()).unwrap(),
            serde_json::json!({"nat_mapping": null, "paths": [], "hostname": "equipo de prueba"})
        );
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
        let names = parse_peer_names(body.as_bytes()).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[&[0xab; 16]], "portátil de Ana");
        let id = atenea_crypto::hex::decode::<16>("12345678123412341234123456789abc").unwrap();
        assert_eq!(names[&id], "servidor");
        assert!(parse_peer_names(br#"{"device_id":"x"}"#).is_err());
        assert!(parse_peer_names(&vec![b' '; NAMES_LIMIT + 1]).is_err());
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
}
