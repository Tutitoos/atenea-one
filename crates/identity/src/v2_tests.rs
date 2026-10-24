use super::*;

fn prefix(s: &str) -> Prefix {
    let (addr, len) = s.split_once('/').unwrap();
    Prefix {
        addr: addr.parse().unwrap(),
        len: len.parse().unwrap(),
    }
}

fn config() -> DeviceConfig {
    let mut c = setup().3;
    c.peers[0].routes = vec![prefix("192.168.50.0/24")];
    c.advertised = vec![AdvertisedRoute {
        prefix: prefix("172.16.0.0/16"),
        snat: true,
    }];
    c.dns = DnsConfig {
        servers: vec!["192.168.50.53".parse().unwrap()],
        domains: vec!["lab.internal".into()],
    };
    c.rules.push(Rule {
        src: Selector::Prefix(prefix("172.16.1.0/24")),
        dst: Selector::Prefix(prefix("192.168.50.0/25")),
        proto: 0,
        port_min: 0,
        port_max: 0,
    });
    c.canonicalize();
    c
}

#[test]
fn vectors_and_structural_rules() {
    let (_, issuer, cert, _, local, state) = setup();
    let c = config();
    let valid = issue_config(&issuer, &cert, &c).unwrap();
    fixture("v2_valid.bin", &valid);
    assert_eq!(
        verify_config(&valid, &local, &state, NOW).unwrap().config,
        c
    );
    let body = c.encode().unwrap();
    assert_eq!(body[0], 2);
    assert_eq!(DeviceConfig::parse(&body).unwrap().encode().unwrap(), body);
    let mut bad = Vec::new();
    for (name, route) in [
        ("public", "203.0.113.0/24"),
        ("default4", "0.0.0.0/0"),
        ("default6", "::/0"),
        ("short4", "10.0.0.0/7"),
        ("short6", "fc00::/31"),
        ("cross_private", "172.0.0.0/8"),
        ("loopback", "127.0.0.0/8"),
        ("link_local", "169.254.0.0/16"),
        ("multicast", "224.0.0.0/4"),
    ] {
        let mut b = c.clone();
        b.peers[0].routes = vec![prefix(route)];
        bad.push((name, b));
    }
    let mut b = c.clone();
    b.peers[0].routes = vec![prefix("10.0.0.0/24")];
    bad.push(("network_overlap", b));
    let mut b = c.clone();
    b.advertised[0].prefix = prefix("192.168.50.0/25");
    bad.push(("advertised_overlap", b));
    let mut b = c.clone();
    b.peers[0].routes.push(prefix("192.168.50.0/25"));
    bad.push(("route_overlap", b));
    let mut b = c.clone();
    let mut peer = b.peers[0].clone();
    peer.device_id = [9; 16];
    peer.noise_pubkey = [10; 32];
    peer.addrs = vec!["10.0.0.3".parse().unwrap()];
    b.peers.push(peer);
    bad.push(("peer_overlap", b));
    let mut b = c.clone();
    b.dns.servers = vec!["192.168.99.1".parse().unwrap()];
    bad.push(("dns_unreachable", b));
    let mut b = c.clone();
    b.rules[0].src = Selector::Prefix(prefix("192.168.99.0/24"));
    bad.push(("selector_outside", b));
    for (name, mut b) in bad {
        b.canonicalize();
        let bytes = raw(&issuer, &cert, &b);
        fixture(&format!("v2_invalid_{name}.bin"), &bytes);
        rejects(&bytes, &local, &state, NOW, ConfigError::InvalidStructure);
        assert_eq!(
            DeviceConfig::parse(&b.encode_wire()),
            Err(Error::InvalidStructure)
        );
    }
    // Ambos valores SNAT sobreviven firmados, sin inferir ningún valor por defecto.
    let mut c = c;
    c.advertised[0].snat = false;
    let verified = verify_config(&raw(&issuer, &cert, &c), &local, &state, NOW).unwrap();
    assert!(!verified.config.advertised[0].snat);
}

fn malformed(name: &str, body: &[u8]) {
    fixture(&format!("v2_malformed_{name}.bin"), body);
    assert_eq!(DeviceConfig::parse(body), Err(Error::Malformed), "{name}");
    let (_, issuer, cert, _, local, state) = setup();
    let mut signed = b"AOCF\x01".to_vec();
    signed.extend_from_slice(&cert.encode());
    signed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    signed.extend_from_slice(body);
    signed.extend_from_slice(&sign(&issuer, CONFIG_DOMAIN, body));
    rejects(&signed, &local, &state, NOW, ConfigError::Malformed);
}

#[test]
fn strict_wire_and_domains() {
    let c = config();
    let mut empty = setup().3;
    empty.peers.clear();
    empty.rules.clear();
    let mut body = empty.encode().unwrap();
    body[0] = 2;
    body.extend_from_slice(&[0, 0, 0]);
    malformed("empty", &body);
    let mut body = c.encode().unwrap();
    body[0] = 3;
    assert_eq!(DeviceConfig::parse(&body), Err(Error::UnsupportedVersion));
    let mut body = c.encode().unwrap();
    body.push(0);
    malformed("trailing", &body);
    for (name, domain) in [
        ("dot", ".".to_owned()),
        ("uppercase", "Lab.internal".into()),
        ("long_label", "a".repeat(64)),
        ("leading_hyphen", "-lab.internal".into()),
        ("trailing_hyphen", "lab-.internal".into()),
        ("empty_label", "lab..internal".into()),
        ("trailing_dot", "lab.internal.".into()),
        ("non_ascii", "láb.internal".into()),
        ("empty_domain", String::new()),
        ("long_domain", "a".repeat(254)),
    ] {
        let mut b = c.clone();
        b.dns.domains = vec![domain];
        malformed(name, &b.encode_wire());
    }
    let mut b = c.clone();
    b.dns.domains = vec!["z.internal".into(), "a.internal".into()];
    malformed("domain_order", &b.encode_wire());
    let mut b = c.clone();
    b.dns.domains.push(b.dns.domains[0].clone());
    malformed("domain_duplicate", &b.encode_wire());
    let mut b = c.clone();
    b.peers[0].routes = vec![prefix("192.168.51.0/24"), prefix("192.168.50.0/24")];
    malformed("route_order", &b.encode_wire());
    let mut b = c.clone();
    let route = b.peers[0].routes[0].clone();
    b.peers[0].routes.push(route);
    malformed("route_duplicate", &b.encode_wire());
    let mut b = c.clone();
    b.dns.servers.push(b.dns.servers[0]);
    malformed("dns_duplicate", &b.encode_wire());
    let mut b = c.clone();
    b.dns.servers = vec![
        "192.168.50.54".parse().unwrap(),
        "192.168.50.53".parse().unwrap(),
    ];
    malformed("dns_order", &b.encode_wire());
    let mut b = c.clone();
    b.advertised.push(b.advertised[0].clone());
    malformed("advertised_duplicate", &b.encode_wire());
    let mut b = c.clone();
    b.advertised.push(AdvertisedRoute {
        prefix: prefix("10.100.0.0/16"),
        snat: false,
    });
    malformed("advertised_order", &b.encode_wire());
    // La configuración mínima deja la bandera inmediatamente después del prefijo anunciado.
    let mut b = empty;
    b.advertised = c.advertised.clone();
    let mut body = b.encode().unwrap();
    let flag = body.len() - 3;
    body[flag] = 2;
    malformed("flags", &body);
    for (name, count) in [
        ("routes_limit", 9),
        ("dns_limit", 4),
        ("domains_limit", 9),
        ("advertised_limit", 9),
    ] {
        let mut b = c.clone();
        match name {
            "routes_limit" => b.peers[0].routes.resize(count, prefix("192.168.60.0/24")),
            "dns_limit" => b.dns.servers.resize(count, "10.0.0.1".parse().unwrap()),
            "domains_limit" => b.dns.domains.resize(count, "x.internal".into()),
            _ => b.advertised.resize(count, c.advertised[0].clone()),
        }
        malformed(name, &b.encode_wire());
    }
    let body = c.encode().unwrap();
    for n in 0..body.len() {
        assert!(DeviceConfig::parse(&body[..n]).is_err());
    }
}

proptest! {
    #[test]
    fn private_route_and_roundtrip(octet in 1u8..=254, snat in any::<bool>(), label in "[a-z][a-z0-9]{0,30}") {
        let mut c = config();
        c.peers[0].routes = vec![prefix(&format!("192.168.{octet}.0/24"))];
        c.dns.servers = vec![format!("192.168.{octet}.53").parse().unwrap()];
        c.dns.domains = vec![format!("{label}.internal")];
        c.advertised[0].snat = snat;
        c.rules.retain(|r| matches!(r.src, Selector::Device(_)));
        let b = c.encode().unwrap();
        prop_assert_eq!(DeviceConfig::parse(&b).unwrap(), c);
    }
}
