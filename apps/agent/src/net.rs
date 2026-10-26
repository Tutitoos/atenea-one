// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Descubrimiento de candidatos: interfaces locales y STUN sobre el MISMO socket de datos
//! (el mapeo del NAT depende del puerto local; un socket distinto daría un endpoint inservible).

use atenea_nat_traversal::{
    Message, NatMapping, TxId, classify_mapping, encode_binding_request, parse,
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

/// Un socket `[::]` de doble pila entrega las direcciones IPv4 como IPv4-mapeadas: se normalizan.
pub fn from_wire(a: SocketAddr) -> SocketAddr {
    match a {
        SocketAddr::V6(v6) => v6
            .ip()
            .to_ipv4_mapped()
            .map_or(a, |v4| SocketAddr::new(IpAddr::V4(v4), v6.port())),
        SocketAddr::V4(_) => a,
    }
}

/// Dirección a usar con `send_to`: en un socket de doble pila las IPv4 deben ir mapeadas; en uno
/// IPv4 no se puede alcanzar una dirección IPv6 (`None`).
pub fn to_wire(a: SocketAddr, dual: bool) -> Option<SocketAddr> {
    match (a, dual) {
        (SocketAddr::V4(v4), true) => Some(SocketAddr::new(
            IpAddr::V6(v4.ip().to_ipv6_mapped()),
            v4.port(),
        )),
        (SocketAddr::V6(_), false) => None,
        _ => Some(a),
    }
}

/// Direcciones globales de las interfaces (salida de `ip -o addr show scope global`), con el puerto
/// de escucha. Se excluye la interfaz del túnel: sus direcciones no son candidatos de red subyacente.
pub fn parse_ip_addr(text: &str, skip_iface: &str, port: u16) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        // formato: "<n>: <iface> inet|inet6 <addr>/<prefijo> ..."
        if t.len() < 4
            || t[1].trim_end_matches(':') == skip_iface
            || !matches!(t[2], "inet" | "inet6")
        {
            continue;
        }
        if line.contains("deprecated") || line.contains("tentative") {
            continue;
        }
        if let Some(ip) = t[3]
            .split('/')
            .next()
            .and_then(|a| a.parse::<IpAddr>().ok())
        {
            let sa = SocketAddr::new(ip, port);
            if atenea_nat_traversal::candidate_ok(&sa) && !out.contains(&sa) {
                out.push(sa);
            }
        }
    }
    out
}

pub fn exclude_tunnel_candidates(
    candidates: &mut Vec<SocketAddr>,
    excluded: &[atenea_identity::Prefix],
) {
    candidates.retain(|address| {
        !excluded
            .iter()
            .any(|prefix| atenea_policies::prefix_contains(prefix, &address.ip()))
    });
}

pub async fn local_addrs(
    skip_iface: &str,
    port: u16,
    excluded: &[atenea_identity::Prefix],
) -> Vec<SocketAddr> {
    use crate::platform::HostNetwork;
    let host = crate::platform::Platform::new(skip_iface.into(), Default::default());
    let mut candidates = host.local_addrs(port).await;
    exclude_tunnel_candidates(&mut candidates, excluded);
    candidates
}

/// Cliente STUN: dos consultas (dos puertos del mismo servidor) desde el socket de datos.
pub struct Stun {
    names: Vec<String>,
    pending: HashMap<TxId, (usize, Instant)>,
    mapped: [Option<SocketAddr>; 2],
}

impl Stun {
    pub fn new(names: Vec<String>) -> Stun {
        Stun {
            names,
            pending: HashMap::new(),
            mapped: [None, None],
        }
    }

    /// Resuelve los servidores y lanza las dos consultas.
    pub async fn start(&mut self, socket: &UdpSocket, dual: bool, now: Instant) {
        self.pending.clear();
        self.mapped = [None, None];
        for (i, name) in self.names.clone().iter().enumerate().take(2) {
            let Ok(addrs) = tokio::net::lookup_host(name.as_str()).await else {
                continue;
            };
            let addrs: Vec<SocketAddr> = addrs.collect();
            // Preferencia por IPv4: es lo que ven la mayoría de los NAT domésticos.
            let Some(addr) = addrs
                .iter()
                .find(|a| a.is_ipv4())
                .or(addrs.first())
                .copied()
            else {
                continue;
            };
            let Some(wire) = to_wire(addr, dual) else {
                continue;
            };
            let mut tx: TxId = [0; 12];
            tx.copy_from_slice(&atenea_identity::generate_seed()[..12]);
            if socket
                .send_to(&encode_binding_request(&tx), wire)
                .await
                .is_ok()
            {
                self.pending.insert(tx, (i, now));
            }
        }
    }

    /// Procesa una respuesta STUN. `true` si correspondía a una consulta pendiente.
    pub fn on_response(&mut self, buf: &[u8]) -> bool {
        let Ok(Message::BindingSuccess { tx, mapped }) = parse(buf) else {
            return false;
        };
        let Some((i, _)) = self.pending.remove(&tx) else {
            return false;
        };
        self.mapped[i] = Some(from_wire(mapped));
        true
    }

    /// Descarta consultas sin respuesta pasado el plazo.
    pub fn expire(&mut self, now: Instant, ttl: Duration) {
        self.pending
            .retain(|_, (_, at)| now.duration_since(*at) < ttl);
    }

    pub fn complete(&self) -> bool {
        self.pending.is_empty()
    }

    /// Endpoint reflexivo (de la primera consulta) y clasificación del mapeo si hay las dos respuestas.
    pub fn result(&self) -> (&'static str, Option<SocketAddr>) {
        match (self.mapped[0], self.mapped[1]) {
            (Some(a), Some(b)) => (
                match classify_mapping(a, b) {
                    NatMapping::Independent => "independent",
                    NatMapping::Dependent => "dependent",
                },
                Some(a),
            ),
            (Some(a), None) | (None, Some(a)) => ("unknown", Some(a)),
            (None, None) => ("unknown", None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atenea_nat_traversal::encode_binding_success;

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn wire_mapping_roundtrip() {
        let v4 = sa("192.0.2.1:5");
        let wired = to_wire(v4, true).unwrap();
        assert!(wired.is_ipv6());
        assert_eq!(from_wire(wired), v4);
        assert_eq!(to_wire(v4, false), Some(v4));
        assert_eq!(
            to_wire(sa("[2001:db8::1]:5"), true),
            Some(sa("[2001:db8::1]:5"))
        );
        assert_eq!(to_wire(sa("[2001:db8::1]:5"), false), None);
        assert_eq!(from_wire(sa("[2001:db8::1]:5")), sa("[2001:db8::1]:5"));
    }

    #[test]
    fn interface_parsing_skips_tunnel_loopback_and_tentative() {
        let text = "\
2: eth0    inet 192.168.1.2/24 brd 192.168.1.255 scope global eth0\\       valid_lft forever preferred_lft forever
2: eth0    inet6 2001:db8:1::2/64 scope global \\       valid_lft forever preferred_lft forever
2: eth0    inet6 2001:db8:1::99/64 scope global tentative \\       valid_lft forever preferred_lft forever
3: atenea0    inet 10.77.0.1/24 scope global atenea0\\       valid_lft forever preferred_lft forever
4: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever preferred_lft forever
5: eth1    inet 169.254.3.3/16 scope global eth1\\       valid_lft forever preferred_lft forever";
        assert_eq!(
            parse_ip_addr(text, "atenea0", 51820),
            vec![sa("192.168.1.2:51820"), sa("[2001:db8:1::2]:51820")]
        );
    }

    #[test]
    fn stun_classifies_mapping_and_ignores_unsolicited_responses() {
        let mut s = Stun::new(vec!["h:1".into(), "h:2".into()]);
        let t0 = Instant::now();
        s.pending.insert([1; 12], (0, t0));
        s.pending.insert([2; 12], (1, t0));
        // respuesta no solicitada: se ignora
        assert!(!s.on_response(&encode_binding_success(&[9; 12], sa("203.0.113.5:1000"))));
        assert!(s.on_response(&encode_binding_success(&[1; 12], sa("203.0.113.5:40000"))));
        assert!(!s.complete());
        assert_eq!(s.result(), ("unknown", Some(sa("203.0.113.5:40000"))));
        assert!(s.on_response(&encode_binding_success(&[2; 12], sa("203.0.113.5:40000"))));
        assert!(s.complete());
        assert_eq!(s.result(), ("independent", Some(sa("203.0.113.5:40000"))));
        // mapeo dependiente del destino
        let mut d = Stun::new(vec!["h:1".into(), "h:2".into()]);
        d.pending.insert([1; 12], (0, t0));
        d.pending.insert([2; 12], (1, t0));
        d.on_response(&encode_binding_success(&[1; 12], sa("203.0.113.5:40000")));
        d.on_response(&encode_binding_success(&[2; 12], sa("203.0.113.5:51234")));
        assert_eq!(d.result().0, "dependent");
        // caducidad de consultas
        let mut e = Stun::new(vec!["h:1".into(), "h:2".into()]);
        e.pending.insert([1; 12], (0, t0));
        e.expire(t0 + Duration::from_secs(5), Duration::from_secs(3));
        assert!(e.complete());
    }
}
