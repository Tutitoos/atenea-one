// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Evaluación de ACL (contrato `signed-config-v1/v2`): denegación por defecto.
//!
//! Los puertos de una regla se comparan con el puerto de **destino**. Solo se leen puertos del
//! primer fragmento y de TCP/UDP; en cualquier otro caso no hay puerto y una regla con puertos no coincide.

use atenea_identity::{DeviceConfig, DeviceId, Prefix, Rule, Selector};
use std::collections::HashMap;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flow {
    pub src: IpAddr,
    pub dst: IpAddr,
    pub proto: u8,
    pub dport: Option<u16>,
    pub non_first_fragment: bool,
}

const TCP: u8 = 6;
const UDP: u8 = 17;
const IPV6_FRAGMENT: u8 = 44;

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

/// Clasifica un paquete IP completo. `None` si no es IPv4/IPv6 coherente.
pub fn classify(pkt: &[u8]) -> Option<Flow> {
    match pkt.first()? >> 4 {
        4 => {
            if pkt.len() < 20 {
                return None;
            }
            let ihl = usize::from(pkt[0] & 0x0f) * 4;
            let total = usize::from(be16(pkt, 2));
            if ihl < 20 || total < ihl || total > pkt.len() {
                return None;
            }
            let proto = pkt[9];
            let offset = be16(pkt, 6) & 0x1fff;
            let dport = (offset == 0 && matches!(proto, TCP | UDP) && ihl + 4 <= total)
                .then(|| be16(pkt, ihl + 2));
            Some(Flow {
                src: IpAddr::from([pkt[12], pkt[13], pkt[14], pkt[15]]),
                dst: IpAddr::from([pkt[16], pkt[17], pkt[18], pkt[19]]),
                proto,
                dport,
                non_first_fragment: offset != 0,
            })
        }
        6 => {
            if pkt.len() < 40 {
                return None;
            }
            let total = 40 + usize::from(be16(pkt, 4));
            if total > pkt.len() {
                return None;
            }
            let proto = pkt[6];
            let mut src = [0u8; 16];
            let mut dst = [0u8; 16];
            src.copy_from_slice(&pkt[8..24]);
            dst.copy_from_slice(&pkt[24..40]);
            let dport = (matches!(proto, TCP | UDP) && 44 <= total).then(|| be16(pkt, 42));
            let non_first_fragment =
                proto == IPV6_FRAGMENT && 48 <= total && (be16(pkt, 42) >> 3) != 0;
            Some(Flow {
                src: src.into(),
                dst: dst.into(),
                proto,
                dport,
                non_first_fragment,
            })
        }
        _ => None,
    }
}

pub fn prefix_contains(p: &Prefix, ip: &IpAddr) -> bool {
    let (a, b, bits): (u128, u128, u32) = match (p.addr, ip) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (u32::from(a).into(), u32::from(*b).into(), 32),
        (IpAddr::V6(a), IpAddr::V6(b)) => (a.into(), (*b).into(), 128),
        _ => return false,
    };
    let shift = bits.saturating_sub(u32::from(p.len));
    shift >= 128 || (a >> shift) == (b >> shift)
}

/// Pertenencia a las subredes firmadas de un par, sin incluir sus direcciones exactas.
pub fn peer_route_contains(peer: &atenea_identity::Peer, ip: IpAddr) -> bool {
    peer.routes.iter().any(|p| p.contains(ip))
}

/// Pertenencia a una subred que sirve el dispositivo local.
pub fn advertised_contains(cfg: &DeviceConfig, ip: IpAddr) -> bool {
    cfg.advertised.iter().any(|a| a.prefix.contains(ip))
}

/// Dirección exacta primero; en su defecto, ruta de prefijo más largo.
pub fn peer_for_destination(cfg: &DeviceConfig, ip: IpAddr) -> Option<&atenea_identity::Peer> {
    cfg.peers
        .iter()
        .find(|p| p.addrs.contains(&ip))
        .or_else(|| {
            cfg.peers
                .iter()
                .flat_map(|p| p.routes.iter().map(move |r| (p, r)))
                .filter(|(_, r)| r.contains(ip))
                .max_by_key(|(_, r)| r.len)
                .map(|(p, _)| p)
        })
}
pub struct Policy {
    devices: HashMap<DeviceId, Vec<IpAddr>>,
    rules: Vec<Rule>,
}

impl Policy {
    pub fn new(cfg: &DeviceConfig) -> Policy {
        let mut devices = HashMap::new();
        devices.insert(cfg.device_id, cfg.tunnel_addrs.clone());
        for p in &cfg.peers {
            devices.insert(p.device_id, p.addrs.clone());
        }
        Policy {
            devices,
            rules: cfg.rules.clone(),
        }
    }

    fn selects(&self, s: &Selector, ip: &IpAddr) -> bool {
        match s {
            Selector::Any => true,
            Selector::Device(id) => self.devices.get(id).is_some_and(|a| a.contains(ip)),
            Selector::Prefix(p) => prefix_contains(p, ip),
        }
    }

    /// `true` solo si alguna regla permite el flujo.
    pub fn allows(&self, f: &Flow) -> bool {
        self.rules.iter().any(|r| {
            self.selects(&r.src, &f.src)
                && self.selects(&r.dst, &f.dst)
                && (r.proto == 0 || r.proto == f.proto)
                && ((r.port_min, r.port_max) == (0, 0)
                    || (!f.non_first_fragment
                        && f.dport
                            .is_some_and(|p| (r.port_min..=r.port_max).contains(&p))))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atenea_identity::Peer;
    use proptest::prelude::*;

    const OWN: DeviceId = [1; 16];
    const PEER: DeviceId = [2; 16];

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn cfg(rules: Vec<Rule>) -> DeviceConfig {
        DeviceConfig {
            network_id: [9; 16],
            device_id: OWN,
            issuer_serial: 1,
            config_version: 1,
            not_before: 0,
            not_after: 100,
            noise_pubkey: [3; 32],
            identity_pubkey: [4; 32],
            mtu: 1400,
            tunnel_addrs: vec![ip("10.0.0.1"), ip("fd00::1")],
            network_prefixes: vec![
                Prefix {
                    addr: ip("10.0.0.0"),
                    len: 24,
                },
                Prefix {
                    addr: ip("fd00::"),
                    len: 64,
                },
            ],
            peers: vec![Peer {
                device_id: PEER,
                noise_pubkey: [5; 32],
                addrs: vec![ip("10.0.0.2"), ip("fd00::2")],
                endpoints: vec![],
                routes: vec![],
            }],
            rules,
            advertised: vec![],
            dns: Default::default(),
        }
    }

    fn rule(src: Selector, dst: Selector, proto: u8, port_min: u16, port_max: u16) -> Rule {
        Rule {
            src,
            dst,
            proto,
            port_min,
            port_max,
        }
    }

    fn flow(src: &str, dst: &str, proto: u8, dport: Option<u16>) -> Flow {
        Flow {
            src: ip(src),
            dst: ip(dst),
            proto,
            dport,
            non_first_fragment: false,
        }
    }

    #[test]
    fn subnet_acl_and_routing() {
        let route = Prefix {
            addr: ip("192.168.50.0"),
            len: 24,
        };
        let own = Prefix {
            addr: ip("172.16.0.0"),
            len: 16,
        };
        let mut c = cfg(vec![rule(
            Selector::Prefix(route.clone()),
            Selector::Prefix(own.clone()),
            0,
            0,
            0,
        )]);
        c.peers[0].routes = vec![route];
        c.advertised = vec![atenea_identity::AdvertisedRoute {
            prefix: own,
            snat: false,
        }];
        assert!(c.validate_structure().is_ok());
        assert!(peer_route_contains(&c.peers[0], ip("192.168.50.22")));
        assert!(!peer_route_contains(&c.peers[0], ip("10.0.0.2")));
        assert!(advertised_contains(&c, ip("172.16.2.3")));
        assert!(!advertised_contains(&c, ip("192.168.50.22")));
        assert!(Policy::new(&c).allows(&flow("192.168.50.22", "172.16.2.3", 1, None)));
        assert!(!Policy::new(&c).allows(&flow("172.16.2.3", "192.168.50.22", 1, None)));
        assert_eq!(
            peer_for_destination(&c, ip("192.168.50.22"))
                .unwrap()
                .device_id,
            PEER
        );
        assert!(peer_for_destination(&c, ip("192.168.51.1")).is_none());
        // También es determinista antes de la validación de solapes.
        let mut narrow = c.peers[0].clone();
        narrow.device_id = [3; 16];
        narrow.addrs = vec![ip("10.0.0.3")];
        narrow.routes[0].len = 25;
        c.peers.push(narrow);
        assert_eq!(
            peer_for_destination(&c, ip("192.168.50.22"))
                .unwrap()
                .device_id,
            [3; 16]
        );
        c.peers[0].addrs.push(ip("192.168.50.22"));
        assert_eq!(
            peer_for_destination(&c, ip("192.168.50.22"))
                .unwrap()
                .device_id,
            PEER
        );
    }

    #[test]
    fn default_deny() {
        let p = Policy::new(&cfg(vec![]));
        assert!(!p.allows(&flow("10.0.0.2", "10.0.0.1", 6, Some(80))));
    }

    #[test]
    fn device_and_any_selectors() {
        let p = Policy::new(&cfg(vec![rule(
            Selector::Device(PEER),
            Selector::Device(OWN),
            0,
            0,
            0,
        )]));
        assert!(p.allows(&flow("10.0.0.2", "10.0.0.1", 1, None)));
        assert!(p.allows(&flow("fd00::2", "fd00::1", 58, None)));
        assert!(!p.allows(&flow("10.0.0.1", "10.0.0.2", 1, None))); // sentido contrario
        assert!(!p.allows(&flow("10.0.0.9", "10.0.0.1", 1, None))); // origen ajeno
        let p = Policy::new(&cfg(vec![rule(Selector::Any, Selector::Any, 0, 0, 0)]));
        assert!(p.allows(&flow("1.2.3.4", "5.6.7.8", 99, None)));
    }

    #[test]
    fn unknown_device_selector_matches_nothing() {
        let p = Policy::new(&cfg(vec![rule(
            Selector::Device([7; 16]),
            Selector::Any,
            0,
            0,
            0,
        )]));
        assert!(!p.allows(&flow("10.0.0.2", "10.0.0.1", 6, Some(1))));
    }

    #[test]
    fn prefix_selector_is_family_aware() {
        let p = Policy::new(&cfg(vec![rule(
            Selector::Prefix(Prefix {
                addr: ip("10.0.0.0"),
                len: 24,
            }),
            Selector::Any,
            0,
            0,
            0,
        )]));
        assert!(p.allows(&flow("10.0.0.77", "10.0.0.1", 6, Some(1))));
        assert!(!p.allows(&flow("10.0.1.77", "10.0.0.1", 6, Some(1))));
        assert!(!p.allows(&flow("fd00::2", "fd00::1", 6, Some(1))));
        let all = Policy::new(&cfg(vec![rule(
            Selector::Prefix(Prefix {
                addr: ip("0.0.0.0"),
                len: 0,
            }),
            Selector::Any,
            0,
            0,
            0,
        )]));
        assert!(all.allows(&flow("9.9.9.9", "10.0.0.1", 6, Some(1))));
        assert!(!all.allows(&flow("::1", "fd00::1", 6, Some(1))));
    }

    #[test]
    fn protocol_and_port_ranges_are_inclusive() {
        let p = Policy::new(&cfg(vec![rule(Selector::Any, Selector::Any, 6, 80, 90)]));
        for port in [80, 85, 90] {
            assert!(
                p.allows(&flow("10.0.0.2", "10.0.0.1", 6, Some(port))),
                "{port}"
            );
        }
        for port in [79, 91] {
            assert!(
                !p.allows(&flow("10.0.0.2", "10.0.0.1", 6, Some(port))),
                "{port}"
            );
        }
        assert!(!p.allows(&flow("10.0.0.2", "10.0.0.1", 17, Some(85)))); // otro protocolo
        assert!(!p.allows(&flow("10.0.0.2", "10.0.0.1", 6, None))); // sin puerto
        let mut frag = flow("10.0.0.2", "10.0.0.1", 6, None);
        frag.non_first_fragment = true;
        assert!(!p.allows(&frag));
    }

    #[test]
    fn any_port_rule_allows_fragments_and_icmp() {
        let p = Policy::new(&cfg(vec![rule(Selector::Any, Selector::Any, 1, 0, 0)]));
        assert!(p.allows(&flow("10.0.0.2", "10.0.0.1", 1, None)));
        assert!(!p.allows(&flow("10.0.0.2", "10.0.0.1", 6, Some(1))));
        let p = Policy::new(&cfg(vec![rule(Selector::Any, Selector::Any, 0, 0, 0)]));
        let mut frag = flow("10.0.0.2", "10.0.0.1", 17, None);
        frag.non_first_fragment = true;
        assert!(p.allows(&frag));
    }

    fn v4(proto: u8, frag: u16, l4: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; 20];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&((20 + l4.len()) as u16).to_be_bytes());
        p[6..8].copy_from_slice(&frag.to_be_bytes());
        p[9] = proto;
        p[12..16].copy_from_slice(&[10, 0, 0, 2]);
        p[16..20].copy_from_slice(&[10, 0, 0, 1]);
        p.extend_from_slice(l4);
        p
    }

    fn v6(next: u8, l4: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; 40];
        p[0] = 0x60;
        p[4..6].copy_from_slice(&(l4.len() as u16).to_be_bytes());
        p[6] = next;
        p[8..24].copy_from_slice(
            &ip("fd00::2")
                .to_string()
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        p[24..40].copy_from_slice(
            &ip("fd00::1")
                .to_string()
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        p.extend_from_slice(l4);
        p
    }

    #[test]
    fn classify_ipv4() {
        let f = classify(&v4(6, 0, &[0x30, 0x39, 0x00, 0x50, 0, 0, 0, 0])).unwrap();
        assert_eq!(
            (f.proto, f.dport, f.non_first_fragment),
            (6, Some(80), false)
        );
        assert_eq!(f.src, ip("10.0.0.2"));
        // fragmento no inicial: sin puerto aunque haya bytes
        let f = classify(&v4(6, 0x0010, &[0x30, 0x39, 0x00, 0x50, 0, 0, 0, 0])).unwrap();
        assert_eq!((f.dport, f.non_first_fragment), (None, true));
        // primer fragmento con MF: sí hay puerto
        let f = classify(&v4(17, 0x2000, &[0, 1, 0, 53, 0, 8, 0, 0])).unwrap();
        assert_eq!((f.dport, f.non_first_fragment), (Some(53), false));
        // ICMP sin puerto; cabecera L4 truncada sin puerto
        assert_eq!(classify(&v4(1, 0, &[8, 0, 0, 0])).unwrap().dport, None);
        assert_eq!(classify(&v4(6, 0, &[0, 1])).unwrap().dport, None);
    }

    #[test]
    fn classify_ipv6() {
        let f = classify(&v6(17, &[0, 1, 0, 53, 0, 8, 0, 0])).unwrap();
        assert_eq!((f.proto, f.dport), (17, Some(53)));
        assert_eq!(classify(&v6(58, &[128, 0, 0, 0])).unwrap().dport, None);
        // extensión (saltos): proto = 0, sin puerto
        let f = classify(&v6(0, &[6, 0, 0, 0, 0, 0, 0, 0, 0, 80, 0, 0])).unwrap();
        assert_eq!((f.proto, f.dport, f.non_first_fragment), (0, None, false));
        // cabecera de fragmento con offset != 0
        let f = classify(&v6(44, &[17, 0, 0x00, 0x08, 0, 0, 0, 1])).unwrap();
        assert_eq!((f.proto, f.dport, f.non_first_fragment), (44, None, true));
        let f = classify(&v6(44, &[17, 0, 0x00, 0x01, 0, 0, 0, 1])).unwrap();
        assert!(!f.non_first_fragment);
    }

    #[test]
    fn classify_rejects_malformed() {
        assert_eq!(classify(&[]), None);
        assert_eq!(classify(&[0x45; 10]), None);
        assert_eq!(classify(&[0x10; 60]), None);
        let mut p = v4(6, 0, &[0; 8]);
        p.truncate(p.len() - 1);
        assert_eq!(classify(&p), None);
        let mut p = v4(6, 0, &[0; 8]);
        p[0] = 0x44;
        assert_eq!(classify(&p), None);
        let mut p = v6(6, &[0; 8]);
        p.truncate(p.len() - 1);
        assert_eq!(classify(&p), None);
    }

    proptest! {
        #[test]
        fn classify_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..200)) {
            let _ = classify(&buf);
        }

        #[test]
        fn allows_never_panics_and_empty_policy_denies(buf in proptest::collection::vec(any::<u8>(), 0..200)) {
            let p = Policy::new(&cfg(vec![]));
            if let Some(f) = classify(&buf) {
                prop_assert!(!p.allows(&f));
            }
        }
    }
}
