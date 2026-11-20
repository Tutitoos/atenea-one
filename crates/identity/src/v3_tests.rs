// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::net::Ipv6Addr;

fn ip(red: u16, equipo: u16, indice: u16) -> IpAddr {
    Ipv6Addr::new(red, 0, 0, 0, 0, 0, equipo, indice).into()
}

fn red(pares: u16) -> DeviceConfig {
    let mut cfg = setup().3;
    cfg.tunnel_addrs = vec![ip(0xfd00, 0, 1)];
    cfg.network_prefixes = vec![Prefix {
        addr: ip(0xfd00, 0, 0),
        len: 64,
    }];
    cfg.rules.clear();
    cfg.peers = (1..=pares)
        .map(|i| {
            let mut noise_pubkey = [0; 32];
            noise_pubkey[..2].copy_from_slice(&i.to_be_bytes());
            Peer {
                device_id: u128::from(i).to_be_bytes(),
                noise_pubkey,
                addrs: vec![ip(0xfd00, i, 1)],
                endpoints: vec![],
                routes: vec![],
            }
        })
        .collect();
    cfg
}

fn tcp(src: Selector, dst: Selector, puertos: u16) -> Vec<Rule> {
    (1..=puertos)
        .map(|port| Rule {
            src: src.clone(),
            dst: dst.clone(),
            proto: 6,
            port_min: port,
            port_max: port,
        })
        .collect()
}

#[test]
fn version_minima_por_cada_contador() {
    for (pares, reglas, dns, version) in [
        (256, 256, false, 1),
        (256, 256, true, 2),
        (257, 0, false, 3),
        (1, 257, false, 3),
        (1, 257, true, 3),
    ] {
        let mut cfg = red(pares);
        cfg.rules = tcp(
            Selector::Device(cfg.device_id),
            Selector::Device(cfg.peers[0].device_id),
            reglas,
        );
        if dns {
            cfg.dns.servers = cfg.tunnel_addrs.clone();
        }
        let mut body = cfg.encode().unwrap();
        assert_eq!(body[0], version);
        assert_eq!(DeviceConfig::parse(&body).unwrap(), cfg);
        if version >= 2 {
            // v2 y v3 tienen el mismo layout, pero nunca son intercambiables.
            body[0] = if version == 2 { 3 } else { 2 };
            assert_eq!(DeviceConfig::parse(&body), Err(Error::Malformed));
        }
    }
}

#[test]
fn limites_v3_declarados_y_reales() {
    let (_, issuer, cert, _, local, state) = setup();
    for (version, max) in [(1, 131_072), (2, 131_072), (3, 238_225)] {
        let mut body = vec![0; max + 1];
        body[..2].copy_from_slice(&[version, 2]);
        assert_eq!(DeviceConfig::parse(&body), Err(Error::TooLarge));
        let mut file = raw(&issuer, &cert, &red(1));
        file[187] = version;
        file[183..187].copy_from_slice(&((max + 1) as u32).to_be_bytes());
        rejects(&file, &local, &state, NOW, ConfigError::TooLarge);
        file[183..187].copy_from_slice(&1u32.to_be_bytes());
        file.resize(251 + max + 1, 0);
        rejects(&file, &local, &state, NOW, ConfigError::TooLarge);
    }
    for (pares, reglas) in [(500, 0), (1, 1537)] {
        let mut cfg = red(pares);
        cfg.rules = tcp(
            Selector::Device(cfg.device_id),
            Selector::Device(cfg.peers[0].device_id),
            reglas,
        );
        assert_eq!(cfg.encode(), Err(Error::InvalidStructure));
        assert_eq!(
            DeviceConfig::parse(&cfg.encode_wire()),
            Err(Error::Malformed)
        );
        rejects(
            &raw(&issuer, &cert, &cfg),
            &local,
            &state,
            NOW,
            ConfigError::Malformed,
        );
    }
}

#[test]
fn cuerpo_maximo_coincide_con_el_limite() {
    let v6 = |i: u16, j: u16| ip(0xfd00, i, j);
    let amplio = |i: u16, j: u16| Prefix {
        addr: v6(i, j),
        len: 128,
    };
    let mut cfg = red(MAX_V3_PEERS as u16);
    cfg.tunnel_addrs = (0..4).map(|j| v6(0, j)).collect();
    cfg.network_prefixes = (0..4).map(|j| amplio(0, j)).collect();
    for (i, p) in cfg.peers.iter_mut().enumerate() {
        let i = i as u16 + 1;
        p.addrs = (0..4).map(|j| v6(i, j)).collect();
        p.endpoints = (0..4).map(|j| SocketAddr::new(v6(i, j), 1)).collect();
        p.routes = (0..8).map(|j| amplio(i, j)).collect();
    }
    cfg.rules = (0..MAX_V3_RULES as u16)
        .map(|i| Rule {
            src: Selector::Prefix(amplio(i, 0)),
            dst: Selector::Prefix(amplio(i, 1)),
            proto: 6,
            port_min: 1,
            port_max: 1,
        })
        .collect();
    cfg.advertised = (0..8)
        .map(|j| AdvertisedRoute {
            prefix: amplio(0, j),
            snat: true,
        })
        .collect();
    cfg.dns.servers = (0..3).map(|j| v6(0, j)).collect();
    let etiqueta = "a".repeat(63);
    cfg.dns.domains = (0..8)
        .map(|j| format!("{etiqueta}.{etiqueta}.{etiqueta}.{}{j}", "a".repeat(60)))
        .collect();
    assert!(cfg.dns.domains.iter().all(|d| d.len() == 253));
    // Solo tamaño: los contadores están al máximo; la validez semántica no afecta a la longitud.
    assert_eq!(cfg.encode_wire().len(), MAX_V3_BODY_LEN);
}
