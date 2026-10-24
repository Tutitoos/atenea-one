//! Configuración de laboratorio del agente (TOML). En la fase 3 la sustituye la configuración firmada.

use atenea_crypto::{StaticKeypair, hex};
use atenea_protocol::{MAX_MTU, MIN_MTU};
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    pub addr: IpAddr,
    pub len: u8,
}

impl FromStr for Net {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (a, l) = s.split_once('/').ok_or("falta /prefijo")?;
        let addr: IpAddr = a.parse().map_err(|_| "dirección inválida")?;
        let len: u8 = l.parse().map_err(|_| "prefijo inválido")?;
        if len > if addr.is_ipv4() { 32 } else { 128 } {
            return Err("prefijo fuera de rango".into());
        }
        Ok(Net { addr, len })
    }
}

impl Net {
    pub fn contains(&self, ip: &IpAddr) -> bool {
        let (a, b): (u128, u128) = match (self.addr, ip) {
            (IpAddr::V4(a), IpAddr::V4(b)) => (u32::from(a).into(), u32::from(*b).into()),
            (IpAddr::V6(a), IpAddr::V6(b)) => (a.into(), (*b).into()),
            _ => return false,
        };
        let bits: u32 = if self.addr.is_ipv4() { 32 } else { 128 };
        let shift = bits - u32::from(self.len);
        if shift >= 128 {
            return true;
        }
        (a >> shift) == (b >> shift)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    network_id: String,
    private_key_file: PathBuf,
    interface: String,
    mtu: Option<usize>,
    listen: SocketAddr,
    ipc_socket: PathBuf,
    addresses: Vec<IpAddr>,
    networks: Vec<String>,
    #[serde(default, rename = "peer")]
    peers: Vec<RawPeer>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPeer {
    public_key: String,
    addresses: Vec<IpAddr>,
    endpoint: Option<SocketAddr>,
}

#[derive(Debug)]
pub struct PeerConfig {
    pub public: [u8; 32],
    pub addresses: Vec<IpAddr>,
    pub endpoint: Option<SocketAddr>,
}

#[derive(Debug)]
pub struct Config {
    pub network_id: [u8; 16],
    pub private_key_file: PathBuf,
    pub interface: String,
    pub mtu: usize,
    pub listen: SocketAddr,
    pub ipc_socket: PathBuf,
    pub addresses: Vec<IpAddr>,
    pub networks: Vec<Net>,
    pub peers: Vec<PeerConfig>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        let network_id =
            hex::decode::<16>(&raw.network_id).ok_or("network_id: se esperan 32 dígitos hex")?;
        let mtu = raw.mtu.unwrap_or(atenea_protocol::DEFAULT_MTU);
        if !(MIN_MTU..=MAX_MTU).contains(&mtu) {
            return Err(format!("mtu fuera de {MIN_MTU}..={MAX_MTU}"));
        }
        let networks = raw
            .networks
            .iter()
            .map(|n| n.parse::<Net>().map_err(|e| format!("networks {n}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        if raw.addresses.is_empty() || networks.is_empty() {
            return Err("addresses y networks no pueden estar vacíos".into());
        }
        let inside = |ip: &IpAddr| networks.iter().any(|n| n.contains(ip));
        if let Some(bad) = raw.addresses.iter().find(|a| !inside(a)) {
            return Err(format!("dirección {bad} fuera de networks"));
        }
        let mut peers = Vec::new();
        let mut seen_addrs: Vec<IpAddr> = raw.addresses.clone();
        let mut seen_keys: Vec<[u8; 32]> = Vec::new();
        for p in &raw.peers {
            let public = hex::decode::<32>(&p.public_key)
                .ok_or("peer.public_key: se esperan 64 dígitos hex")?;
            if p.addresses.is_empty() {
                return Err("peer sin addresses".into());
            }
            if seen_keys.contains(&public) {
                return Err("peer duplicado".into());
            }
            seen_keys.push(public);
            for a in &p.addresses {
                if !inside(a) {
                    return Err(format!("dirección de peer {a} fuera de networks"));
                }
                if seen_addrs.contains(a) {
                    return Err(format!("dirección {a} repetida"));
                }
                seen_addrs.push(*a);
            }
            peers.push(PeerConfig {
                public,
                addresses: p.addresses.clone(),
                endpoint: p.endpoint,
            });
        }
        Ok(Config {
            network_id,
            private_key_file: raw.private_key_file,
            interface: raw.interface,
            mtu,
            listen: raw.listen,
            ipc_socket: raw.ipc_socket,
            addresses: raw.addresses,
            networks,
            peers,
        })
    }

    /// Prefijo de la red que contiene `ip` (para asignarlo a la interfaz).
    pub fn prefix_for(&self, ip: &IpAddr) -> Option<u8> {
        self.networks.iter().find(|n| n.contains(ip)).map(|n| n.len)
    }
}

/// Lee la clave privada (64 dígitos hex). Exige permisos sin acceso a grupo/otros.
pub fn load_key(path: &Path) -> Result<StaticKeypair, String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{}: permisos demasiado abiertos (se exige 0600)",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let private = hex::decode::<32>(&text).ok_or("clave privada: se esperan 64 dígitos hex")?;
    Ok(StaticKeypair::from_private(private))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
network_id = "00112233445566778899aabbccddeeff"
private_key_file = "/k"
interface = "atenea0"
listen = "0.0.0.0:51820"
ipc_socket = "/s"
addresses = ["10.77.0.1", "fd77::1"]
networks = ["10.77.0.0/24", "fd77::/64"]

[[peer]]
public_key = "0000000000000000000000000000000000000000000000000000000000000001"
addresses = ["10.77.0.2", "fd77::2"]
endpoint = "192.0.2.1:51820"
"#;

    #[test]
    fn parses_valid() {
        let c = Config::parse(GOOD).unwrap();
        assert_eq!(c.mtu, 1400);
        assert_eq!(c.peers.len(), 1);
        assert_eq!(c.prefix_for(&"10.77.0.1".parse().unwrap()), Some(24));
        assert_eq!(c.prefix_for(&"fd77::1".parse().unwrap()), Some(64));
    }

    #[test]
    fn rejects_invalid() {
        let bad = |from: &str, to: &str| Config::parse(&GOOD.replace(from, to)).is_err();
        assert!(bad("00112233445566778899aabbccddeeff", "00"));
        assert!(bad("10.77.0.2\", \"fd77::2", "10.78.0.2\", \"fd77::2")); // fuera de red
        assert!(bad("10.77.0.2\"", "10.77.0.1\"")); // repetida con la propia
        assert!(bad(
            "interface = \"atenea0\"",
            "interface = \"atenea0\"\nextra = 1"
        ));
        assert!(bad(
            "listen = \"0.0.0.0:51820\"",
            "mtu = 1000\nlisten = \"0.0.0.0:51820\""
        ));
        assert!(Config::parse(&GOOD.replace("10.77.0.0/24", "10.77.0.0/33")).is_err());
    }

    #[test]
    fn net_contains() {
        let n: Net = "10.0.0.0/8".parse().unwrap();
        assert!(n.contains(&"10.255.0.1".parse().unwrap()));
        assert!(!n.contains(&"11.0.0.1".parse().unwrap()));
        assert!(!n.contains(&"fd00::1".parse().unwrap()));
        let all: Net = "0.0.0.0/0".parse().unwrap();
        assert!(all.contains(&"1.2.3.4".parse().unwrap()));
    }
}
