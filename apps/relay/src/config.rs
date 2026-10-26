// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub relay_id: String,
    pub udp_listen: SocketAddr,
    pub tcp_listen: SocketAddr,
    pub metrics_listen: String,
    pub coordinator_keys: Vec<String>,
    pub tls_cert_file: String,
    pub tls_key_file: String,
    pub proxy_protocol: bool,
    pub status_socket: String,
    pub max_allocations: usize,
    pub max_per_network: usize,
    pub max_per_ip: usize,
    pub packets_per_second: u32,
    pub bits_per_second: u32,
    pub cookie_per_second: u32,
    pub alloc_per_second: u32,
    pub max_datagram: usize,
    pub tcp_queue: usize,
    pub idle_secs: u64,
    pub tcp_per_ip: usize,
    pub tcp_total: usize,
    pub handshake_secs: u64,
    pub alloc_secs: u64,
    pub max_rate_entries: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            relay_id: String::new(),
            udp_listen: "0.0.0.0:443".parse().unwrap(),
            tcp_listen: "127.0.0.1:8443".parse().unwrap(),
            metrics_listen: "127.0.0.1:9103".into(),
            coordinator_keys: vec![],
            tls_cert_file: String::new(),
            tls_key_file: String::new(),
            proxy_protocol: false,
            status_socket: "/tmp/atenea-relay.sock".into(),
            max_allocations: 4096,
            max_per_network: 1024,
            max_per_ip: 32,
            packets_per_second: 2000,
            bits_per_second: 24_000_000,
            cookie_per_second: 20,
            alloc_per_second: 10,
            max_datagram: 1530,
            tcp_queue: 256,
            idle_secs: 90,
            tcp_per_ip: 8,
            tcp_total: 2048,
            handshake_secs: 10,
            alloc_secs: 5,
            max_rate_entries: 65536,
        }
    }
}
impl Config {
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let mut f = Figment::from(Serialized::defaults(Self::default()));
        if let Ok(p) = std::env::var("ATENEA_RELAY_CONFIG") {
            f = f.merge(Toml::file(p));
        }
        let f = f.merge(Env::prefixed("ATENEA_RELAY_"));
        let f = if let Ok(id) = std::env::var("ATENEA_RELAY_ID") {
            f.merge(Serialized::default("relay_id", id))
        } else {
            f
        };
        f.extract().map_err(Box::new)
    }
    pub fn validate(&self) -> Result<Vec<[u8; 32]>, &'static str> {
        self.id()?;
        if [
            self.max_allocations,
            self.max_per_network,
            self.max_per_ip,
            self.max_datagram,
            self.tcp_queue,
            self.tcp_per_ip,
            self.tcp_total,
            self.max_rate_entries,
        ]
        .contains(&0)
            || [
                self.packets_per_second,
                self.bits_per_second,
                self.cookie_per_second,
                self.alloc_per_second,
            ]
            .contains(&0)
            || [self.idle_secs, self.handshake_secs, self.alloc_secs].contains(&0)
            || self.max_datagram > 1530
            || self.bits_per_second < 8
            || self.tcp_queue > 65536
            || self.max_allocations > 1_000_000
            || self.max_rate_entries > 1_000_000
            || self.tcp_total > 65536
            || self.max_per_network > self.max_allocations
            || self.max_per_ip > self.max_allocations
            || self.tcp_per_ip > self.tcp_total
            || self.idle_secs > 86400
            || self.handshake_secs > 300
            || self.alloc_secs > 300
        {
            return Err("límites fuera de rango");
        }
        if self.proxy_protocol && !self.tcp_listen.ip().is_loopback() {
            return Err("PROXY requiere listener loopback");
        }
        if self.tls_cert_file.is_empty()
            || self.tls_key_file.is_empty()
            || self.status_socket.is_empty()
        {
            return Err("faltan certificado, clave o socket de estado");
        }
        if self.coordinator_keys.is_empty() || self.coordinator_keys.len() > 32 {
            return Err("se requieren de 1 a 32 claves de coordinator");
        }
        self.coordinator_keys
            .iter()
            .map(|s| {
                let k: [u8; 32] = hex::decode(s)
                    .map_err(|_| "clave no hexadecimal")?
                    .try_into()
                    .map_err(|_| "clave debe medir 32 bytes")?;
                if atenea_identity::VerifyingKey::from_bytes(&k).map_or(true, |k| k.is_weak()) {
                    return Err("clave inválida");
                }
                Ok(k)
            })
            .collect()
    }
    pub fn id(&self) -> Result<atenea_relay_core::RelayId, &'static str> {
        let s = &self.relay_id;
        if s.len() != 32
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("relay_id requiere 32 hex minúsculos");
        }
        let id: [u8; 16] = hex::decode(s)
            .map_err(|_| "relay_id inválido")?
            .try_into()
            .map_err(|_| "relay_id inválido")?;
        if id == [0; 16] {
            return Err("relay_id no puede ser cero");
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_ip_capacity_is_configurable_without_changing_the_default() {
        assert_eq!(Config::default().max_per_ip, 32);
        let mut c: Config = Figment::from(Serialized::defaults(Config::default()))
            .merge(Toml::string("max_per_ip = 64"))
            .extract()
            .unwrap();
        c.coordinator_keys = vec![hex::encode(
            atenea_identity::SigningKey::from_bytes(&[1; 32])
                .verifying_key()
                .to_bytes(),
        )];
        c.tls_cert_file = "test.pem".into();
        c.relay_id = "11".repeat(16);
        c.tls_key_file = "test-key.pem".into();
        assert_eq!(c.max_per_ip, 64);
        assert!(c.validate().is_ok());
        for invalid in [0, c.max_allocations + 1] {
            c.max_per_ip = invalid;
            assert_eq!(c.validate(), Err("límites fuera de rango"));
        }
    }

    #[test]
    fn proxy_never_binds_external_and_limits_are_validated() {
        let mut c = Config {
            relay_id: "11".repeat(16),
            proxy_protocol: true,
            tcp_listen: "0.0.0.0:443".parse().unwrap(),
            coordinator_keys: vec![hex::encode(
                atenea_identity::SigningKey::from_bytes(&[1; 32])
                    .verifying_key()
                    .to_bytes(),
            )],
            tls_cert_file: "test.pem".into(),
            tls_key_file: "test-key.pem".into(),
            ..Config::default()
        };
        assert_eq!(c.validate(), Err("PROXY requiere listener loopback"));
        c.tcp_listen = "127.0.0.1:443".parse().unwrap();
        assert!(c.validate().is_ok());
        c.max_datagram = 1531;
        assert_eq!(c.validate(), Err("límites fuera de rango"));
    }

    #[test]
    fn relay_id_is_mandatory_lower_hex_and_nonzero() {
        assert!(Config::default().id().is_err());
        for id in [
            "00".repeat(16),
            "AA".repeat(16),
            "11".repeat(15),
            "11".repeat(17),
            "gg".repeat(16),
        ] {
            assert!(
                Config {
                    relay_id: id,
                    ..Config::default()
                }
                .id()
                .is_err()
            );
        }
        assert_eq!(
            Config {
                relay_id: "11".repeat(16),
                ..Config::default()
            }
            .id(),
            Ok([0x11; 16])
        );
    }
}
