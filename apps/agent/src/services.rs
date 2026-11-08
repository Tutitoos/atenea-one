// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Servicios TCP que escucha el equipo (ADR-024, fase 2): solo detección local, nada sale del equipo.
use serde::Serialize;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

/// Máximo de servicios en el estado local; el resto se indica con `services_truncated`.
pub const MAX_SERVICES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Tcp,
}

/// Sin procesos, usuarios ni rutas: solo puerto y protocolo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Service {
    pub port: u16,
    pub proto: Proto,
}

/// Conserva los sockets en comodín o en una dirección de la interfaz de esta red; descarta loopback,
/// otras interfaces, el puerto 0 y los puertos del propio agente. Ordenado, sin duplicados y
/// recortado a `MAX_SERVICES` (el booleano indica el recorte).
pub fn select_services(
    listening: &[SocketAddr],
    network_addrs: &[IpAddr],
    own_ports: &[u16],
) -> (Vec<Service>, bool) {
    let network: Vec<IpAddr> = network_addrs.iter().map(IpAddr::to_canonical).collect();
    let all: BTreeSet<Service> = listening
        .iter()
        .filter(|a| a.port() != 0 && !own_ports.contains(&a.port()))
        .filter(|a| {
            let ip = a.ip().to_canonical();
            ip.is_unspecified() || (!ip.is_loopback() && network.contains(&ip))
        })
        .map(|a| Service {
            port: a.port(),
            proto: Proto::Tcp,
        })
        .collect();
    let truncated = all.len() > MAX_SERVICES;
    (all.into_iter().take(MAX_SERVICES).collect(), truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }
    fn tcp(port: u16) -> Service {
        Service {
            port,
            proto: Proto::Tcp,
        }
    }
    #[test]
    fn keeps_wildcard_and_own_network_addresses_only() {
        let net = ["10.99.0.2".parse().unwrap(), "fd00::2".parse().unwrap()];
        let listening = [
            sa("0.0.0.0:8080"),
            sa("127.0.0.1:5432"),
            sa("[::1]:6379"),
            sa("10.99.0.2:9000"),
            sa("[fd00::2]:9001"),
            sa("192.0.2.7:9002"),
            sa("[::ffff:10.99.0.2]:9003"),
            sa("[::ffff:127.0.0.1]:9004"),
            sa("0.0.0.0:0"),
            sa("[::]:51820"),
        ];
        let (s, t) = select_services(&listening, &net, &[51820]);
        assert!(!t);
        assert_eq!(s, [tcp(8080), tcp(9000), tcp(9001), tcp(9003)]);
    }
    #[test]
    fn deduplicates_and_orders() {
        let l = [sa("[::]:80"), sa("0.0.0.0:80"), sa("0.0.0.0:22")];
        assert_eq!(select_services(&l, &[], &[]).0, [tcp(22), tcp(80)]);
    }
    #[test]
    fn truncates_to_the_limit() {
        let l: Vec<SocketAddr> = (1..=70u16)
            .rev()
            .map(|p| sa(&format!("0.0.0.0:{p}")))
            .collect();
        let (s, t) = select_services(&l, &[], &[]);
        assert!(t);
        assert_eq!(s.len(), MAX_SERVICES);
        assert_eq!(s[0], tcp(1));
        assert_eq!(s[63], tcp(64));
        assert!(!select_services(&l[..64], &[], &[]).1);
    }
    #[test]
    fn serializes_as_port_and_proto() {
        assert_eq!(
            serde_json::to_string(&tcp(22)).unwrap(),
            r#"{"port":22,"proto":"tcp"}"#
        );
    }
}
