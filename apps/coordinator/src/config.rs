use atenea_identity::PublicKey;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};

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
        }
    }
}
impl Config {
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let mut f = Figment::from(Serialized::defaults(Self::default()));
        if let Ok(path) = std::env::var("ATENEA_COORD_CONFIG") {
            f = f.merge(Toml::file(path));
        }
        f.merge(Env::prefixed("ATENEA_COORD_"))
            .extract()
            .map_err(Box::new)
    }
    pub fn validate(&self) -> Result<Vec<PublicKey>, &'static str> {
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
}
