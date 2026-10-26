// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Cabeceras IP mínimas del paquete interno (origen, destino y longitud total).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Devuelve `(origen, destino, longitud_total)` si el paquete es IPv4/IPv6 coherente
/// y su longitud declarada cabe en el búfer.
pub fn parse_ip(pkt: &[u8]) -> Option<(IpAddr, IpAddr, usize)> {
    match pkt.first()? >> 4 {
        4 => {
            if pkt.len() < 20 {
                return None;
            }
            let ihl = usize::from(pkt[0] & 0x0f) * 4;
            let total = usize::from(u16::from_be_bytes([pkt[2], pkt[3]]));
            if ihl < 20 || total < ihl || total > pkt.len() {
                return None;
            }
            let s = Ipv4Addr::new(pkt[12], pkt[13], pkt[14], pkt[15]);
            let d = Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]);
            Some((s.into(), d.into(), total))
        }
        6 => {
            if pkt.len() < 40 {
                return None;
            }
            let total = 40 + usize::from(u16::from_be_bytes([pkt[4], pkt[5]]));
            if total > pkt.len() {
                return None;
            }
            let mut s = [0u8; 16];
            let mut d = [0u8; 16];
            s.copy_from_slice(&pkt[8..24]);
            d.copy_from_slice(&pkt[24..40]);
            Some((Ipv6Addr::from(s).into(), Ipv6Addr::from(d).into(), total))
        }
        _ => None,
    }
}

#[cfg(test)]
pub fn build_ipv4(src: Ipv4Addr, dst: Ipv4Addr, payload: &[u8]) -> Vec<u8> {
    let total = 20 + payload.len();
    let mut p = vec![0u8; 20];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    p[8] = 64;
    p[9] = 17;
    p[12..16].copy_from_slice(&src.octets());
    p[16..20].copy_from_slice(&dst.octets());
    p.extend_from_slice(payload);
    p
}

#[cfg(test)]
pub fn build_ipv6(src: Ipv6Addr, dst: Ipv6Addr, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; 40];
    p[0] = 0x60;
    p[4..6].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    p[6] = 17;
    p[7] = 64;
    p[8..24].copy_from_slice(&src.octets());
    p[24..40].copy_from_slice(&dst.octets());
    p.extend_from_slice(payload);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_and_v6() {
        let a = Ipv4Addr::new(10, 0, 0, 1);
        let b = Ipv4Addr::new(10, 0, 0, 2);
        let p = build_ipv4(a, b, b"hola");
        assert_eq!(parse_ip(&p), Some((a.into(), b.into(), 24)));
        let a6: Ipv6Addr = "fd00::1".parse().unwrap();
        let b6: Ipv6Addr = "fd00::2".parse().unwrap();
        let p = build_ipv6(a6, b6, b"hola");
        assert_eq!(parse_ip(&p), Some((a6.into(), b6.into(), 44)));
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse_ip(&[]), None);
        assert_eq!(parse_ip(&[0x45; 10]), None);
        assert_eq!(parse_ip(&[0x10; 60]), None); // versión 1
        let mut p = build_ipv4(Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST, b"x");
        p.truncate(p.len() - 1); // longitud declarada mayor que el búfer
        assert_eq!(parse_ip(&p), None);
        let mut p = build_ipv4(Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST, b"x");
        p[0] = 0x44; // IHL < 5
        assert_eq!(parse_ip(&p), None);
        let mut p = build_ipv6("::1".parse().unwrap(), "::1".parse().unwrap(), b"x");
        p.truncate(p.len() - 1);
        assert_eq!(parse_ip(&p), None);
    }
}
