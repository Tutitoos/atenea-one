// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! La política compartida no necesita procesos ni privilegios del host.
use super::*;
use atenea_identity::Peer;
use std::cell::{Cell, RefCell};
use std::net::SocketAddr;

#[derive(Default)]
struct MemoryHost {
    routes: RefCell<BTreeSet<String>>,
    existing: RefCell<Vec<Prefix>>,
    dns: RefCell<Option<DnsConfig>>,
    dns_fails: Cell<bool>,
    subnet_routing: bool,
    forwarding: Cell<bool>,
    snat_calls: Cell<usize>,
}
impl HostNetwork for MemoryHost {
    fn supports_subnet_routing(&self) -> bool {
        self.subnet_routing
    }
    fn configure_link(&self, _: usize, _: &[(IpAddr, u8)]) -> Result<(), String> {
        Ok(())
    }
    fn existing_routes(&self) -> Result<Vec<Prefix>, String> {
        Ok(self.existing.borrow().clone())
    }
    fn add_route(&self, prefix: &str) -> Result<(), String> {
        self.routes.borrow_mut().insert(prefix.into());
        Ok(())
    }
    fn remove_route(&self, prefix: &str) -> Result<(), String> {
        self.routes.borrow_mut().remove(prefix);
        Ok(())
    }
    fn set_dns(&self, dns: &DnsConfig) -> Result<(), String> {
        if self.dns_fails.get() {
            return Err("DNS no disponible".into());
        }
        self.dns.replace(Some(dns.clone()));
        Ok(())
    }
    fn clear_dns(&self) -> Result<(), String> {
        self.dns.replace(None);
        Ok(())
    }
    fn set_snat(&self, _: &[AdvertisedRoute], _: bool) -> Result<(), String> {
        self.snat_calls.set(self.snat_calls.get() + 1);
        Ok(())
    }
    fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }
    fn forwarding(&self, _: bool) -> bool {
        self.forwarding.get()
    }
    async fn local_addrs(&self, _: u16) -> Vec<SocketAddr> {
        Vec::new()
    }
}
fn prefix(text: &str) -> Prefix {
    parse_prefix(text).unwrap()
}
fn config() -> DeviceConfig {
    DeviceConfig {
        network_id: [7; 16],
        device_id: [1; 16],
        issuer_serial: 1,
        config_version: 1,
        not_before: 999_000,
        not_after: 1_000_000,
        noise_pubkey: [1; 32],
        identity_pubkey: [2; 32],
        mtu: 1400,
        tunnel_addrs: vec!["10.77.0.1".parse().unwrap()],
        network_prefixes: vec![prefix("10.77.0.0/24")],
        peers: vec![Peer {
            device_id: [2; 16],
            noise_pubkey: [3; 32],
            addrs: vec!["10.77.0.2".parse().unwrap()],
            endpoints: vec!["192.168.40.1:51820".parse().unwrap()],
            routes: vec![prefix("192.168.40.0/24")],
        }],
        rules: vec![],
        advertised: vec![],
        dns: DnsConfig::default(),
    }
}
#[test]
fn peer_endpoint_inside_route_installs_but_control_endpoint_conflicts() {
    let mut net = NetConf::with_host(MemoryHost::default());
    let mut cfg = config();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes[0].state, "installed");
    assert!(net.host.routes.borrow().contains("192.168.40.0/24"));
    net.apply(&cfg, &["192.168.40.2".parse().unwrap()]).unwrap();
    assert_eq!(net.status().routes[0].state, "conflict");
    assert!(net.host.routes.borrow().is_empty());
    net.apply(&cfg, &[]).unwrap();
    cfg.peers[0].routes.clear();
    net.apply(&cfg, &[]).unwrap();
    assert!(net.status().routes.is_empty());
    assert!(net.host.routes.borrow().is_empty());
}
#[cfg(target_os = "windows")]
#[test]
fn windows_cleanup_forgets_removed_routes_and_nrpt_before_reload() {
    let mut net = NetConf::with_host(MemoryHost::default());
    let mut cfg = config();
    cfg.dns = DnsConfig {
        servers: vec!["10.77.0.53".parse().unwrap()],
        domains: vec!["internal.example".into()],
    };
    net.apply(&cfg, &[]).unwrap();
    net.host.routes.borrow_mut().clear();
    net.host.dns.replace(None);
    net.cleanup().unwrap();
    assert!(net.installed.is_empty());
    assert!(net.dns.is_none());
    net.apply(&cfg, &[]).unwrap();
    assert!(net.host.routes.borrow().contains("192.168.40.0/24"));
    assert!(net.host.dns.borrow().is_some());
}
#[test]
fn host_conflict_pending_and_recovery_without_commands() {
    let mut net = NetConf::with_host(MemoryHost::default());
    let cfg = config();
    net.host.existing.replace(vec![prefix("192.168.0.0/16")]);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes[0].state, "conflict");
    net.host.existing.borrow_mut().clear();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes[0].state, "installed");
    net.endpoints_resolved(false);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes[0].state, "pending");
    assert!(net.host.routes.borrow().is_empty());
    net.endpoints_resolved(true);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes[0].state, "installed");
}
#[test]
fn dns_retries_and_reverts_in_memory() {
    let mut net = NetConf::with_host(MemoryHost::default());
    let mut cfg = config();
    cfg.dns = DnsConfig {
        servers: vec!["10.77.0.53".parse().unwrap()],
        domains: vec!["internal.example".into()],
    };
    net.host.dns_fails.set(true);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().dns, "unsupported");
    assert!(net.host.dns.borrow().is_none());
    net.host.dns_fails.set(false);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().dns, "applied");
    assert_eq!(net.host.dns.borrow().as_ref(), Some(&cfg.dns));
    cfg.dns = DnsConfig::default();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().dns, "none");
    assert!(net.host.dns.borrow().is_none());
}
#[test]
fn client_platform_reports_advertised_unsupported_without_snat() {
    let mut cfg = config();
    cfg.advertised.push(AdvertisedRoute {
        prefix: prefix("172.16.0.0/16"),
        snat: true,
    });
    let mut net = NetConf::with_host(MemoryHost::default());
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes[1].state, "unsupported");
    assert_eq!(net.status().forwarding_ok, Some(false));
    assert_eq!(net.host.snat_calls.get(), 0);
    let mut router = NetConf::with_host(MemoryHost {
        subnet_routing: true,
        ..MemoryHost::default()
    });
    router.apply(&cfg, &[]).unwrap();
    assert_eq!(router.host.snat_calls.get(), 1);
    assert_eq!(router.status().forwarding_ok, Some(false));
    router.host.forwarding.set(true);
    assert_eq!(router.status().forwarding_ok, Some(true));
}
// Salida real de macOS (prefijos sintéticos): el túnel propio es utun12; la dirección propia del túnel
// IPv6 aparece como ruta de host en `lo0`, no en la interfaz del túnel.
const MAC_V6: &str = "Internet6:\n\
Destination                             Gateway                                 Flags               Netif Expi\n\
fd77::/64                     fe80::1111:2222:3333:4444%utun12        Uc                 utun12\n\
fd77::2                       link#33                                 UHL                   lo0\n\
fe80::%utun12/64                        fe80::1111:2222:3333:4444%utun12        UcI                utun12\n\
fe80::1111:2222:3333:4444%utun12        link#33                                 UHLI                  lo0\n\
ff00::/8                                fe80::1111:2222:3333:4444%utun12        UmCI               utun12\n";
const MAC_V4: &str = "Internet:\n\
Destination        Gateway            Flags               Netif Expire\n\
default            192.0.2.1          UGScg                 en0\n\
10.77.0.0/24       link#33            UCS                utun12\n\
10.77.0.2          10.77.0.2          UH                 utun12\n\
127                127.0.0.1          UCS                   lo0\n\
192.0.2/24         link#4             UCS                   en0\n";
// `ip -j route show table all` real: túnel propio atenea0 (rutas, local, broadcast, fe80, multicast).
const LINUX_OWN: &str = r#"[
{"type":"unicast","dst":"default","gateway":"192.0.2.1","dev":"eth0","protocol":"dhcp","flags":[]},
{"dst":"10.77.0.0/24","dev":"atenea0","protocol":"kernel","scope":"link","prefsrc":"10.77.0.2","flags":[]},
{"dst":"192.0.2.0/24","dev":"eth0","protocol":"kernel","scope":"link","prefsrc":"192.0.2.5","flags":[]},
{"type":"local","dst":"10.77.0.2","dev":"atenea0","table":"local","protocol":"kernel","scope":"host","prefsrc":"10.77.0.2","flags":[]},
{"type":"broadcast","dst":"10.77.0.255","dev":"atenea0","table":"local","protocol":"kernel","scope":"link","prefsrc":"10.77.0.2","flags":[]},
{"type":"local","dst":"fd77::2","dev":"atenea0","table":"local","protocol":"kernel","metric":0,"flags":[],"pref":"medium"},
{"dst":"fd77::/64","dev":"atenea0","protocol":"kernel","metric":256,"flags":[],"pref":"medium"},
{"dst":"fe80::/64","dev":"atenea0","protocol":"kernel","metric":256,"flags":[],"pref":"medium"},
{"type":"multicast","dst":"ff00::/8","dev":"atenea0","table":"local","protocol":"kernel","metric":256,"flags":[],"pref":"medium"}
]"#;
fn mac_cfg() -> DeviceConfig {
    let mut cfg = config();
    cfg.tunnel_addrs = vec!["10.77.0.2".parse().unwrap(), "fd77::2".parse().unwrap()];
    cfg.network_prefixes = vec![prefix("10.77.0.0/24"), prefix("fd77::/64")];
    cfg
}
fn mac_routes(extra_v4: &str, extra_v6: &str) -> Vec<Prefix> {
    let mut routes =
        crate::platform::macos::parse_routes(&format!("{MAC_V4}{extra_v4}"), "utun12").unwrap();
    routes.extend(
        crate::platform::macos::parse_routes(&format!("{MAC_V6}{extra_v6}"), "utun12").unwrap(),
    );
    routes
}
fn overlap(routes: Vec<Prefix>, cfg: &DeviceConfig) -> Option<String> {
    let net = NetConf::with_host(MemoryHost::default());
    net.host.existing.replace(routes);
    let found = net.overlap_with_host(cfg).unwrap();
    assert!(net.host.routes.borrow().is_empty(), "la guarda solo lee");
    found
}
#[test]
fn own_tunnel_is_never_a_conflict_with_real_macos_netstat() {
    // Incluye `fd77::2` en lo0 y fe80/ff00 del propio utun12.
    assert_eq!(overlap(mac_routes("", ""), &mac_cfg()), None);
    // Direcciones propias en lo0 también en IPv4 y la ruta de host IPv6 de un fe80 ajeno a la red.
    let v4_lo0 = "10.77.0.2          lo0_link           UHL                   lo0\n";
    assert_eq!(overlap(mac_routes(v4_lo0, ""), &mac_cfg()), None);
    assert_eq!(
        overlap(
            mac_routes(
                "",
                "fe80::%en0/64 link#4 UCI en0\nff00::/8 link#4 UmCI en0\n"
            ),
            &mac_cfg()
        ),
        None
    );
}
#[test]
fn own_tunnel_is_never_a_conflict_with_real_linux_ip_output() {
    let routes = crate::platform::linux::parse_routes(LINUX_OWN.as_bytes(), "atenea0").unwrap();
    assert_eq!(overlap(routes, &mac_cfg()), None);
    // Sin saltar la interfaz propia, las rutas de host de nuestra dirección siguen sin contar.
    let all = crate::platform::linux::parse_routes(LINUX_OWN.as_bytes(), "otra").unwrap();
    let only_hosts: Vec<_> = all
        .into_iter()
        .filter(|r| r.len == 32 || r.len == 128)
        .collect();
    let mut cfg = mac_cfg();
    cfg.tunnel_addrs.push("10.77.0.255".parse().unwrap());
    assert_eq!(overlap(only_hosts, &cfg), None);
}
#[test]
fn another_atenea_with_same_cidr_is_a_conflict() {
    let mac = mac_routes("10.77.0.0/24 link#40 UCS utun13\n", "");
    assert!(overlap(mac, &mac_cfg()).unwrap().contains("10.77.0.0/24"));
    let mac6 = mac_routes(
        "",
        "fd77::/64 fe80::1%utun13 Uc utun13\nfd77::2 link#40 UHL lo0\n",
    );
    assert!(overlap(mac6, &mac_cfg()).unwrap().contains("fd77::/64"));
    // Linux: otra instancia atenea1 con el mismo CIDR; su /24 y /64 siguen detectándose.
    let other = LINUX_OWN.replace("atenea0", "atenea1");
    let routes = crate::platform::linux::parse_routes(other.as_bytes(), "atenea0").unwrap();
    assert!(overlap(routes, &mac_cfg()).is_some());
}
#[test]
fn broader_routes_are_not_conflicts_but_more_specific_ones_are() {
    // VPN corporativa, Docker o LAN: contienen la red del túnel; gana la más específica.
    assert_eq!(
        overlap(
            mac_routes(
                "10.0.0.0/8 192.0.2.1 UGSc en0\n",
                "fd00::/8 fe80::1%en0 UGc en0\n"
            ),
            &mac_cfg()
        ),
        None
    );
    assert_eq!(
        overlap(
            vec![
                prefix("10.0.0.0/8"),
                prefix("fd00::/8"),
                prefix("fd77::/48")
            ],
            &mac_cfg()
        ),
        None
    );
    // Otra interfaz reclama parte de la red del túnel.
    let v4 = overlap(
        mac_routes("10.77.0.128/25 link#4 UCS en0\n", ""),
        &mac_cfg(),
    )
    .unwrap();
    assert!(
        v4.contains("10.77.0.0/24") && v4.contains("10.77.0.128/25"),
        "{v4}"
    );
    let v6 = overlap(vec![prefix("fd77:0:0:0:1::/80")], &mac_cfg()).unwrap();
    assert!(v6.contains("fd77::/64") && v6.contains("/80"), "{v6}");
    // Un /80 fuera del /64 (cuarto grupo distinto) no es parte de la red del túnel.
    assert_eq!(overlap(vec![prefix("fd77:0:0:1::/80")], &mac_cfg()), None);
    // Una ruta de host ajena dentro de la red, que no es una dirección propia, sí es un conflicto.
    assert!(overlap(vec![prefix("10.77.0.9/32")], &mac_cfg()).is_some());
}
#[test]
fn failing_route_listing_is_an_error_not_a_pass() {
    struct Failing(MemoryHost);
    impl HostNetwork for Failing {
        fn supports_subnet_routing(&self) -> bool {
            false
        }
        fn configure_link(&self, _: usize, _: &[(IpAddr, u8)]) -> Result<(), String> {
            Ok(())
        }
        fn existing_routes(&self) -> Result<Vec<Prefix>, String> {
            Err("netstat: fallo".into())
        }
        fn add_route(&self, p: &str) -> Result<(), String> {
            self.0.add_route(p)
        }
        fn remove_route(&self, p: &str) -> Result<(), String> {
            self.0.remove_route(p)
        }
        fn set_dns(&self, d: &DnsConfig) -> Result<(), String> {
            self.0.set_dns(d)
        }
        fn clear_dns(&self) -> Result<(), String> {
            self.0.clear_dns()
        }
        fn set_snat(&self, r: &[AdvertisedRoute], c: bool) -> Result<(), String> {
            self.0.set_snat(r, c)
        }
        fn cleanup(&self) -> Result<(), String> {
            Ok(())
        }
        fn forwarding(&self, v6: bool) -> bool {
            self.0.forwarding(v6)
        }
        async fn local_addrs(&self, p: u16) -> Vec<SocketAddr> {
            self.0.local_addrs(p).await
        }
    }
    let net = NetConf::with_host(Failing(MemoryHost::default()));
    assert_eq!(
        net.overlap_with_host(&config()),
        Err("netstat: fallo".into())
    );
}
#[test]
fn router_never_announces_tunnel_or_advertised_addresses() {
    let mut candidates = vec![
        "192.168.50.1:51820".parse().unwrap(),
        "10.77.0.1:51820".parse().unwrap(),
        "192.168.1.2:51820".parse().unwrap(),
    ];
    crate::net::exclude_tunnel_candidates(
        &mut candidates,
        &[prefix("192.168.50.0/24"), prefix("10.77.0.0/24")],
    );
    assert_eq!(
        candidates,
        vec!["192.168.1.2:51820".parse::<SocketAddr>().unwrap()]
    );
}
