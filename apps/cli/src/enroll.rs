// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Incorporación del dispositivo sin transmitir sus claves privadas.
use atenea_crypto::hex;
use atenea_identity::DeviceBundle;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `https` siempre; `http` solo hacia loopback o con `--insecure-http` (laboratorio). El token de
/// enrolamiento y la topología no deben viajar en claro por una red real.
fn base_url(url: &str, insecure_http: bool) -> Result<&str, String> {
    let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
    let loopback = uri.host().is_some_and(|h| {
        h == "localhost"
            || h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !(uri.scheme_str() == Some("https")
        || (uri.scheme_str() == Some("http") && (loopback || insecure_http)))
        || uri.host().is_none()
        || uri.authority().is_some_and(|a| a.as_str().contains('@'))
        || uri.query().is_some()
        || url.contains('#')
    {
        return Err("se exige una URL https (http solo en loopback o con --insecure-http) sin credenciales, query ni fragmento".into());
    }
    Ok(url.trim_end_matches('/'))
}

fn bundle(dir: &Path) -> Result<DeviceBundle, String> {
    let bytes = std::fs::read(dir.join("bundle.bin")).map_err(|e| format!("bundle.bin: {e}"))?;
    let bundle = DeviceBundle::parse(&bytes).map_err(|_| "bundle.bin inválido")?;
    bundle
        .verify()
        .map_err(|_| "prueba de posesión del bundle inválida")?;
    Ok(bundle)
}

fn http_error(status: u16, body: &str) -> String {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error")?.get("code")?.as_str().map(str::to_owned));
    match (status, code.as_deref()) {
        (401, Some("invalid_token")) => "token inválido, caducado o ya utilizado".into(),
        (401, _) => "enrolamiento no autorizado".into(),
        (403, _) => "enrolamiento denegado".into(),
        (409, _) => "conflicto: dispositivo o claves ya registrados, o red sin capacidad".into(),
        (429, _) => "demasiadas peticiones; inténtalo más tarde".into(),
        (400 | 422, _) => "solicitud de enrolamiento inválida".into(),
        (500..=599, _) => "control plane no disponible".into(),
        _ => format!("respuesta HTTP inesperada: {status}"),
    }
}

#[derive(Deserialize)]
struct Enrolled {
    device_id: String,
    network_id: String,
    status: String,
}

pub fn enroll(
    url: &str,
    token: &str,
    dir: &Path,
    name: &str,
    insecure_http: bool,
) -> Result<(), String> {
    let url = format!("{}/api/v1/enroll", base_url(url, insecure_http)?);
    let bundle = bundle(dir)?;
    let body =
        serde_json::json!({"token": token, "bundle": hex::encode(&bundle.encode()), "name": name})
            .to_string();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(&url)
        .header("Content-Type", "application/json")
        .send(&body)
        .map_err(|_| "no se pudo contactar con el control plane".to_string())?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(16384)
        .read_to_string()
        .map_err(|_| "respuesta ilegible o demasiado grande")?;
    if status != 201 {
        return Err(http_error(status, &text));
    }
    let enrolled: Enrolled =
        serde_json::from_str(&text).map_err(|_| "respuesta de enrolamiento inválida")?;
    println!(
        "device_id={}\nnetwork_id={}\nstatus={}",
        enrolled.device_id, enrolled.network_id, enrolled.status
    );
    Ok(())
}

#[derive(Deserialize)]
struct Join {
    url: String,
    network_id: String,
    root_anchor: String,
}

fn parse_join(text: &str, insecure_http: bool) -> Result<Join, String> {
    let join: Join = serde_json::from_str(text).map_err(|_| "join: JSON inválido")?;
    base_url(&join.url, insecure_http)?;
    let network = join.network_id.replace('-', "");
    hex::decode::<16>(&network).ok_or("join.network_id: UUID o 32 dígitos hex requeridos")?;
    hex::decode::<32>(&join.root_anchor).ok_or("join.root_anchor: se esperan 64 dígitos hex")?;
    Ok(Join {
        network_id: network,
        ..join
    })
}

#[derive(clap::Args)]
pub struct AgentConfigArgs {
    #[arg(long)]
    join: String,
    #[arg(long)]
    dir: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "atenea0")]
    interface: String,
    #[arg(long, default_value = "0.0.0.0:51820")]
    listen: std::net::SocketAddr,
    /// INIT con MAC1 válido por segundo antes de exigir cookie; siempre positivo.
    #[arg(long, default_value = "100")]
    handshake_cookie_threshold_per_second: std::num::NonZeroU32,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    config_file: Option<PathBuf>,
    #[arg(long)]
    ipc_socket: Option<PathBuf>,
    /// Solicita una subred privada (repetible, máximo ocho); requiere aprobación administrativa.
    #[arg(long = "advertise-route")]
    advertise_routes: Vec<String>,
    /// Socket opcional de solo lectura, distinto del socket de control.
    #[arg(long)]
    status_socket: Option<PathBuf>,
    /// Grupo numérico autorizado a consultar el socket de estado.
    #[arg(long, requires = "status_socket")]
    status_gid: Option<u32>,
    /// Solo laboratorio: acepta una URL `http://` que no sea loopback y la deja anotada en el TOML.
    #[arg(long)]
    insecure_http: bool,
}

fn render(join: &Join, args: &AgentConfigArgs, device: &[u8; 16]) -> Result<String, String> {
    let mut table = toml::Table::new();
    for (key, value) in [
        ("interface", args.interface.clone()),
        ("listen", args.listen.to_string()),
        ("network_id", join.network_id.clone()),
        ("device_id", hex::encode(device)),
        ("trust_anchor", join.root_anchor.clone()),
    ] {
        table.insert(key.into(), value.into());
    }
    table.insert(
        "handshake_cookie_threshold_per_second".into(),
        i64::from(args.handshake_cookie_threshold_per_second.get()).into(),
    );
    for (key, path) in [
        ("noise_key_file", args.dir.join("noise.key")),
        ("identity_key_file", args.dir.join("identity.key")),
        (
            "state_dir",
            args.state_dir
                .clone()
                .unwrap_or_else(|| args.dir.join("state")),
        ),
        (
            "config_file",
            args.config_file
                .clone()
                .unwrap_or_else(|| args.dir.join("config.bin")),
        ),
        (
            "ipc_socket",
            args.ipc_socket.clone().unwrap_or_else(|| {
                if cfg!(windows) {
                    PathBuf::from(r"\\.\pipe\atenea-control")
                } else {
                    args.dir.join("agent.sock")
                }
            }),
        ),
    ] {
        table.insert(key.into(), path.to_str().ok_or("ruta no UTF-8")?.into());
    }
    table.insert(
        "advertise_routes".into(),
        toml::Value::Array(
            args.advertise_routes
                .iter()
                .cloned()
                .map(toml::Value::String)
                .collect(),
        ),
    );
    if let Some(path) = &args.status_socket {
        table.insert(
            "status_socket".into(),
            path.to_str().ok_or("ruta no UTF-8")?.into(),
        );
    }
    if let Some(gid) = args.status_gid {
        table.insert("status_gid".into(), i64::from(gid).into());
    }
    let mut sync = toml::Table::new();
    sync.insert("url".into(), join.url.clone().into());
    if args.insecure_http {
        sync.insert("insecure_http".into(), true.into());
    }
    table.insert("sync".into(), toml::Value::Table(sync));
    toml::to_string(&table).map_err(|e| format!("TOML: {e}"))
}

pub fn agent_config(mut args: AgentConfigArgs) -> Result<(), String> {
    let text = if args.join.trim_start().starts_with('{') {
        args.join.clone()
    } else {
        std::fs::read_to_string(&args.join).map_err(|e| format!("join: {e}"))?
    };
    let join = parse_join(&text, args.insecure_http)?;
    args.dir = std::fs::canonicalize(&args.dir).map_err(|e| format!("dir: {e}"))?;
    let bundle = bundle(&args.dir)?;
    let text = render(&join, &args, &bundle.device_id)?;
    crate::issue::write_secret(&args.out, &text)
}

#[cfg(test)]
#[path = "../../agent/src/config.rs"]
#[allow(dead_code)]
mod agent_config_contract;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn join_and_generated_toml_match_agent() {
        let input = format!(
            r#"{{"url":"https://example.test","network_id":"00112233-4455-6677-8899-aabbccddeeff","root_anchor":"{}","token":"NO_GUARDAR"}}"#,
            "01".repeat(32)
        );
        let join = parse_join(&input, false).unwrap();
        let mut args = AgentConfigArgs {
            join: input,
            dir: "/tmp/device".into(),
            out: "unused".into(),
            interface: "atenea0".into(),
            listen: "0.0.0.0:51820".parse().unwrap(),
            handshake_cookie_threshold_per_second: std::num::NonZeroU32::new(7).unwrap(),
            state_dir: None,
            config_file: None,
            ipc_socket: None,
            advertise_routes: vec!["192.168.10.0/24".into(), "fd12:3456::/32".into()],
            status_socket: Some(
                if cfg!(windows) {
                    r"\\.\pipe\atenea-status"
                } else {
                    "/run/atenea/status.sock"
                }
                .into(),
            ),
            status_gid: Some(545),
            insecure_http: false,
        };
        let text = render(&join, &args, &[2; 16]).unwrap();
        let cfg = agent_config_contract::Config::parse(&text).unwrap();
        assert_eq!(cfg.trust_anchor, [1; 32]);
        assert_eq!(cfg.device_id, [2; 16]);
        assert_eq!(cfg.advertise_routes, args.advertise_routes);
        assert_eq!(cfg.status_socket, args.status_socket);
        assert_eq!(cfg.status_gid, Some(545));
        assert_eq!(cfg.handshake_cookie_threshold_per_second.get(), 7);
        assert_eq!(cfg.sync.unwrap().interval_secs, 60);
        assert!(!text.contains("NO_GUARDAR"));
        args.advertise_routes.clear();
        args.status_socket = None;
        args.status_gid = None;
        args.handshake_cookie_threshold_per_second = std::num::NonZeroU32::new(100).unwrap();
        let default_text = render(&join, &args, &[2; 16]).unwrap();
        let defaults = agent_config_contract::Config::parse(&default_text).unwrap();
        assert!(defaults.advertise_routes.is_empty());
        assert!(defaults.status_socket.is_none());
        assert!(defaults.status_gid.is_none());
        assert_eq!(defaults.handshake_cookie_threshold_per_second.get(), 100);
        assert!(parse_join("{}", false).is_err());
        assert!(
            parse_join(
                &args
                    .join
                    .replace("https://example.test", "ftp://example.test"),
                false
            )
            .is_err()
        );
        let plain = args
            .join
            .replace("https://example.test", "http://198.51.100.7:8080");
        assert!(parse_join(&plain, false).is_err());
        assert!(parse_join(&plain.replace("198.51.100.7", "127.0.0.1"), false).is_ok());
        let join = parse_join(&plain, true).unwrap();
        args.insecure_http = true;
        let text = render(&join, &args, &[2; 16]).unwrap();
        assert!(
            agent_config_contract::Config::parse(&text)
                .unwrap()
                .sync
                .unwrap()
                .insecure_http
        );
    }
    #[test]
    fn errors_never_echo_remote_secrets() {
        assert!(
            http_error(
                401,
                r#"{"error":{"code":"invalid_token","message":"SECRET"}}"#
            )
            .contains("caducado")
        );
        for status in [400, 401, 403, 409, 422, 429, 500, 502, 200] {
            assert!(!http_error(status, "SECRET").contains("SECRET"));
        }
        assert!(http_error(409, "{}").contains("conflicto"));
    }
}
