//! Configuración local del agente (TOML, sin firmar): rutas y arranque inicial.
//! La autorización (peers, direcciones, ACL) viene de la configuración firmada (`config_file`).

use atenea_crypto::{StaticKeypair, hex};
use atenea_identity::{DeviceId, NetworkId, PublicKey, SigningKey, signing_key};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    interface: String,
    listen: SocketAddr,
    ipc_socket: PathBuf,
    noise_key_file: PathBuf,
    identity_key_file: PathBuf,
    config_file: PathBuf,
    state_dir: PathBuf,
    /// Solo para el primer arranque; con estado existente se ignoran y se comprueban.
    network_id: String,
    device_id: String,
    trust_anchor: String,
    sync: Option<SyncConfig>,
    coordinator: Option<CoordinatorConfig>,
}

#[derive(Debug)]
pub struct Config {
    pub interface: String,
    pub listen: SocketAddr,
    pub ipc_socket: PathBuf,
    pub noise_key_file: PathBuf,
    pub identity_key_file: PathBuf,
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub network_id: NetworkId,
    pub device_id: DeviceId,
    pub trust_anchor: PublicKey,
    pub sync: Option<SyncConfig>,
    pub coordinator: Option<CoordinatorConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    pub url: String,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
}

fn default_interval() -> u64 {
    60
}

/// Coordinator y STUN propios (docs/protocol/coordinator-v1.md §4). No es de confianza: solo aporta pistas.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorConfig {
    pub url: String,
    /// Dos puertos del MISMO servidor STUN (`host:3478`, `host:3479`), en ese orden.
    pub stun: Vec<String>,
    #[serde(default)]
    pub relay_ca_file: Option<PathBuf>,
    #[serde(default = "default_relay")]
    pub relay: String,
}

fn default_relay() -> String {
    "auto".into()
}

impl CoordinatorConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.relay.as_str(), "auto" | "off") {
            return Err("coordinator.relay: se espera auto u off".into());
        }
        if let Some(path) = &self.relay_ca_file {
            crate::relay::load_roots(Some(path))?;
        }
        let uri: ureq::http::Uri = self.url.parse().map_err(|_| "coordinator.url inválida")?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.host().is_none()
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
            || uri.query().is_some()
            || self.url.contains('#')
        {
            return Err("coordinator: URL http(s) sin credenciales, query ni fragmento".into());
        }
        if self.stun.len() != 2
            || self.stun.iter().any(|s| {
                s.rsplit_once(':')
                    .is_none_or(|(h, p)| h.is_empty() || p.parse::<u16>().is_err())
            })
        {
            return Err("coordinator.stun: se esperan exactamente dos entradas host:puerto".into());
        }
        Ok(())
    }
}

impl SyncConfig {
    pub fn endpoint(&self) -> Result<String, String> {
        let uri: ureq::http::Uri = self.url.parse().map_err(|_| "sync.url inválida")?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.host().is_none()
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
            || uri.query().is_some()
            || self.url.contains('#')
            || self.interval_secs < 5
        {
            return Err(
                "sync: URL http(s) sin credenciales, query ni fragmento e intervalo mínimo de 5 s"
                    .into(),
            );
        }
        Ok(format!(
            "{}/api/v1/device/config",
            self.url.trim_end_matches('/')
        ))
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        if let Some(sync) = &raw.sync {
            sync.endpoint()?;
        }
        if let Some(c) = &raw.coordinator {
            c.validate()?;
        }
        Ok(Config {
            network_id: hex::decode::<16>(&raw.network_id)
                .ok_or("network_id: se esperan 32 dígitos hex")?,
            device_id: hex::decode::<16>(&raw.device_id)
                .ok_or("device_id: se esperan 32 dígitos hex")?,
            trust_anchor: hex::decode::<32>(&raw.trust_anchor)
                .ok_or("trust_anchor: se esperan 64 dígitos hex")?,
            interface: raw.interface,
            listen: raw.listen,
            ipc_socket: raw.ipc_socket,
            noise_key_file: raw.noise_key_file,
            identity_key_file: raw.identity_key_file,
            config_file: raw.config_file,
            state_dir: raw.state_dir,
            sync: raw.sync,
            coordinator: raw.coordinator,
        })
    }
}

/// Lee 32 bytes hex de un fichero de clave. Exige permisos sin acceso a grupo/otros.
fn load_seed(path: &Path) -> Result<[u8; 32], String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{}: permisos demasiado abiertos (se exige 0600)",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    hex::decode::<32>(&text).ok_or_else(|| format!("{}: se esperan 64 dígitos hex", path.display()))
}

pub fn load_noise_key(path: &Path) -> Result<StaticKeypair, String> {
    Ok(StaticKeypair::from_private(load_seed(path)?))
}

pub fn load_identity_key(path: &Path) -> Result<SigningKey, String> {
    Ok(signing_key(load_seed(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "/s"
noise_key_file = "/n"
identity_key_file = "/i"
config_file = "/c"
state_dir = "/d"
network_id = "00112233445566778899aabbccddeeff"
device_id = "ffeeddccbbaa99887766554433221100"
trust_anchor = "0000000000000000000000000000000000000000000000000000000000000001"
"#;

    #[test]
    fn parses_valid() {
        let c = Config::parse(GOOD).unwrap();
        assert_eq!(c.interface, "atenea0");
        assert_eq!(c.device_id[0], 0xff);
    }

    #[test]
    fn optional_sync_defaults_and_validation() {
        assert!(Config::parse(GOOD).unwrap().sync.is_none());
        let text = format!("{GOOD}\n[sync]\nurl = \"https://example.test\"");
        assert_eq!(
            Config::parse(&text).unwrap().sync.unwrap().interval_secs,
            60
        );
        assert!(Config::parse(&format!("{text}\ninterval_secs = 4")).is_err());
        assert!(Config::parse(&format!("{text}\ninterval_secs = 5")).is_ok());
        assert!(Config::parse(&text.replace("https://", "ftp://")).is_err());
        assert!(Config::parse(&text.replace("example.test", "example.test/?token=x")).is_err());
    }

    #[test]
    fn rejects_invalid() {
        assert!(Config::parse(&GOOD.replace("00112233445566778899aabbccddeeff", "00")).is_err());
        assert!(
            Config::parse(&GOOD.replace("state_dir = \"/d\"", "state_dir = \"/d\"\nextra = 1"))
                .is_err()
        );
        assert!(Config::parse(&GOOD.replace("trust_anchor", "anchor")).is_err());
        assert!(Config::parse("").is_err());
    }

    #[test]
    fn relay_options_default_and_validate() {
        let base = format!(
            "{GOOD}\n[coordinator]\nurl = \"https://coord.test\"\nstun = [\"stun.test:1\", \"stun.test:2\"]"
        );
        let c = Config::parse(&base).unwrap().coordinator.unwrap();
        assert_eq!(c.relay, "auto");
        assert!(c.relay_ca_file.is_none());
        assert!(Config::parse(&format!("{base}\nrelay = \"off\"")).is_ok());
        assert!(Config::parse(&format!("{base}\nrelay = \"invalid\"")).is_err());
        assert!(
            Config::parse(&format!("{base}\nrelay_ca_file = \"/nonexistent/ca.pem\"")).is_err()
        );
    }
}
