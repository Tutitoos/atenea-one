use atenea_identity::PublicKey;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Relay {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_name: Option<String>,
}

fn valid_endpoint(endpoint: &str) -> bool {
    let Some((host, port)) = endpoint.rsplit_once(':') else {
        return false;
    };
    let host_valid = if host.starts_with('[') {
        host.strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .is_some_and(|h| h.parse::<std::net::Ipv6Addr>().is_ok())
    } else {
        !host.is_empty()
            && host.split('.').all(|label| {
                !label.is_empty()
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    };
    host_valid && port.parse::<u16>().is_ok_and(|p| p != 0)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub listen: String,
    pub stun_ports: Vec<u16>,
    pub stun_bind: String,
    pub anchors: Vec<String>,
    pub presence_ttl_secs: u64,
    pub max_body_bytes: usize,
    pub max_presences: usize,
    pub max_long_polls: usize,
    pub max_nonces: usize,
    pub max_rate_entries: usize,
    pub announce_interval_secs: u64,
    pub rendezvous_pair_interval_secs: u64,
    pub rendezvous_per_minute: u32,
    pub stun_per_second: u32,
    pub stun_burst: u32,
    pub relay_signing_key_file: Option<PathBuf>,
    pub relays: Vec<Relay>,
    pub relay_token_ttl_secs: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8081".into(),
            stun_ports: vec![3478, 3479],
            stun_bind: "0.0.0.0".into(),
            anchors: vec![],
            presence_ttl_secs: 60,
            max_body_bytes: 160 * 1024,
            max_presences: 4096,
            max_long_polls: 256,
            max_nonces: 65536,
            max_rate_entries: 65536,
            announce_interval_secs: 2,
            rendezvous_pair_interval_secs: 1,
            rendezvous_per_minute: 10,
            stun_per_second: 20,
            stun_burst: 40,
            relay_signing_key_file: None,
            relays: vec![],
            relay_token_ttl_secs: 300,
        }
    }
}
impl Config {
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let mut f = Figment::from(Serialized::defaults(Self::default()));
        if let Ok(path) = std::env::var("ATENEA_COORD_CONFIG") {
            f = f.merge(Toml::file(path));
        }
        f = f.merge(Env::prefixed("ATENEA_COORD_").ignore(&["RELAYS"]));
        if let Ok(json) = std::env::var("ATENEA_COORD_RELAYS") {
            let relays: Vec<Relay> = serde_json::from_str(&json).map_err(|_| {
                Box::new(figment::Error::from(
                    "RELAYS requiere una lista JSON válida",
                ))
            })?;
            f = f.merge(Serialized::default("relays", relays));
        }
        f.extract().map_err(Box::new)
    }
    pub fn validate(&self) -> Result<Vec<PublicKey>, &'static str> {
        if self.relay_token_ttl_secs == 0 || self.relay_token_ttl_secs > 600 {
            return Err("relay_token_ttl_secs debe estar entre 1 y 600");
        }
        for relay in &self.relays {
            if relay.name.trim().is_empty()
                || (relay.udp.is_none() && relay.tls.is_none())
                || relay.udp.as_deref().is_some_and(|v| !valid_endpoint(v))
                || relay.tls.as_deref().is_some_and(|v| !valid_endpoint(v))
                || (relay.tls.is_some()
                    && relay
                        .tls_name
                        .as_deref()
                        .is_none_or(|v| v.trim().is_empty()))
            {
                return Err(
                    "relay inválido: requiere nombre, udp o tls host:puerto y tls_name para TLS",
                );
            }
        }
        if self.stun_ports.len() != 2
            || self.stun_ports[0] == self.stun_ports[1]
            || self.stun_ports.contains(&0)
        {
            return Err("stun_ports requiere dos puertos distintos no nulos");
        }
        if self.presence_ttl_secs == 0
            || self.max_body_bytes == 0
            || self.max_presences == 0
            || self.max_long_polls == 0
            || self.max_nonces == 0
            || self.max_rate_entries == 0
            || self.announce_interval_secs == 0
            || self.rendezvous_pair_interval_secs == 0
            || self.rendezvous_per_minute == 0
            || self.stun_per_second == 0
            || self.stun_burst == 0
        {
            return Err("los límites deben ser positivos");
        }
        self.listen
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "listen inválido")?;
        self.stun_bind
            .parse::<std::net::IpAddr>()
            .map_err(|_| "stun_bind inválido")?;
        if self.anchors.is_empty() {
            return Err("se requiere al menos un ancla");
        }
        self.anchors
            .iter()
            .map(|s| {
                let key: PublicKey = hex::decode(s)
                    .map_err(|_| "ancla hexadecimal inválida")?
                    .try_into()
                    .map_err(|_| "ancla de longitud inválida")?;
                if key == [0; 32]
                    || atenea_identity::VerifyingKey::from_bytes(&key).map_or(true, |k| k.is_weak())
                {
                    return Err("ancla inválida");
                }
                Ok(key)
            })
            .collect()
    }
    pub fn relay_signing_key(&self) -> Result<Option<atenea_identity::SigningKey>, &'static str> {
        let Some(path) = &self.relay_signing_key_file else {
            return Ok(None);
        };
        if !std::fs::metadata(path)
            .map_err(|_| "no se puede inspeccionar la clave de relay")?
            .is_file()
        {
            return Err("la clave de relay requiere fichero regular 0600");
        }
        let file = std::fs::File::open(path).map_err(|_| "no se puede abrir la clave de relay")?;
        let meta = file
            .metadata()
            .map_err(|_| "no se puede inspeccionar la clave de relay")?;
        if !meta.is_file() || meta.permissions().mode() & 0o777 != 0o600 {
            return Err("la clave de relay requiere fichero regular 0600");
        }
        let text =
            std::io::read_to_string(file).map_err(|_| "no se puede leer la clave de relay")?;
        let seed: [u8; 32] = hex::decode(text.trim())
            .map_err(|_| "semilla de relay hexadecimal inválida")?
            .try_into()
            .map_err(|_| "semilla de relay debe tener 32 bytes")?;
        Ok(Some(atenea_identity::signing_key(seed)))
    }
}
