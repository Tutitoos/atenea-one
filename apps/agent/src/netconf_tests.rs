use super::*;
use crate::platform::linux::{Binaries, Network};
use atenea_identity::{AdvertisedRoute, Peer};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Host {
    root: PathBuf,
}

impl Host {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "atenea-netconf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let host = Self { root };
        host.put("ip4.json", "[]");
        host.put("ip6.json", "[]");
        host.put("tables.json", r#"{"nftables":[]}"#);
        host.script(
            "ip",
            r#"#!/bin/sh
set -eu
base=$(dirname "$0")
printf '%s\n' "$*" >> "$base/ip.log"
if [ "$1" = '-j' ]; then
    case "$2" in
        -4) cat "$base/ip4.json" ;;
        -6) cat "$base/ip6.json" ;;
    esac
fi
"#,
        );
        host.script(
            "nft",
            r#"#!/bin/sh
set -eu
base=$(dirname "$0")
printf '%s\n' "$*" >> "$base/nft.log"
case "$1" in
    -j) cat "$base/tables.json" ;;
    -f) printf '%s\n' '--- transaction ---' >> "$base/transactions"; cat >> "$base/transactions" ;;
esac
"#,
        );
        host.script(
            "resolvectl",
            r#"#!/bin/sh
set -eu
base=$(dirname "$0")
printf '%s\n' "$*" >> "$base/dns.log"
if [ -f "$base/dns-fail" ]; then exit 1; fi
"#,
        );
        host
    }

    fn put(&self, name: &str, text: &str) {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn script(&self, name: &str, text: &str) {
        self.put(name, text);
        fs::set_permissions(self.root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn log(&self, name: &str) -> String {
        match fs::read_to_string(self.root.join(name)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => panic!("no se pudo leer {name}: {e}"),
        }
    }

    fn netconf(&self) -> NetConf<Network> {
        NetConf::with_host(Network::new(
            "atenea0".into(),
            Binaries {
                ip: self.root.join("ip"),
                nft: self.root.join("nft"),
                resolvectl: self.root.join("resolvectl"),
                proc_sys: self.root.join("proc_sys"),
            },
        ))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
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
            endpoints: vec!["192.0.2.2:51820".parse().unwrap()],
            routes: vec![prefix("192.168.40.0/24")],
        }],
        rules: vec![],
        advertised: vec![],
        dns: DnsConfig::default(),
    }
}

#[test]
fn routes_apply_only_the_diff_and_ignore_defaults_and_own_interface() {
    let host = Host::new();
    host.put(
        "ip4.json",
        r#"[{"dst":"default","dev":"eth0"},{"dst":"192.168.40.0/24","dev":"atenea0"}]"#,
    );
    let mut net = host.netconf();
    let mut cfg = config();
    net.apply(&cfg, &[]).unwrap();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.routes[0].state, "installed");
    cfg.peers[0].routes = vec![prefix("fd40::/64")];
    net.apply(&cfg, &[]).unwrap();
    cfg.peers[0].routes.clear();
    net.apply(&cfg, &[]).unwrap();
    let commands: Vec<_> = host
        .log("ip.log")
        .lines()
        .filter(|line| !line.starts_with("-j "))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        commands,
        [
            "-4 route add 192.168.40.0/24 dev atenea0",
            "-4 route del 192.168.40.0/24 dev atenea0",
            "-6 route add fd40::/64 dev atenea0",
            "-6 route del fd40::/64 dev atenea0",
        ]
    );
    assert!(net.status.routes.is_empty());
}

#[test]
fn route_conflicts_remove_previously_installed_routes() {
    let host = Host::new();
    let mut net = host.netconf();
    let cfg = config();
    net.apply(&cfg, &[]).unwrap();
    host.put("ip4.json", r#"[{"dst":"192.168.0.0/16","dev":"eth0"}]"#);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.routes[0].state, "conflict");
    assert!(
        net.status.routes[0]
            .reason
            .as_ref()
            .unwrap()
            .contains("otra interfaz")
    );
    assert!(net.installed.is_empty());
    let commands = host.log("ip.log");
    assert_eq!(
        commands
            .lines()
            .filter(|line| line.contains(" route add "))
            .count(),
        1
    );
    assert_eq!(
        commands
            .lines()
            .filter(|line| line.contains(" route del "))
            .count(),
        1
    );
}

#[test]
fn protected_control_addresses_but_not_peer_endpoints_block_routes() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    net.apply(&cfg, &["192.168.40.8".parse().unwrap()]).unwrap();
    assert_eq!(net.status.routes[0].state, "conflict");
    assert!(
        net.status.routes[0]
            .reason
            .as_ref()
            .unwrap()
            .contains("endpoint")
    );
    cfg.peers[0].endpoints = vec!["192.168.40.9:51820".parse().unwrap()];
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.routes[0].state, "installed");
    assert!(host.log("ip.log").contains(" route add "));
}

#[test]
fn dns_is_split_per_link_idempotent_and_reverted_when_removed() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    cfg.dns = DnsConfig {
        servers: vec!["10.77.0.53".parse().unwrap()],
        domains: vec!["corp.example".into(), "lab.example".into()],
    };
    net.apply(&cfg, &[]).unwrap();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.dns, "applied");
    let commands = host.log("dns.log");
    assert_eq!(
        commands.lines().collect::<Vec<_>>(),
        [
            "dns atenea0 10.77.0.53",
            "domain atenea0 ~corp.example ~lab.example",
            "default-route atenea0 no",
        ]
    );
    assert!(!commands.split_whitespace().any(|arg| arg == "~."));
    cfg.dns = DnsConfig::default();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.dns, "none");
    assert!(host.log("dns.log").ends_with("revert atenea0\n"));
}

#[test]
fn unavailable_or_failing_resolved_is_reported_unsupported_and_retried() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    cfg.dns = DnsConfig {
        servers: vec!["10.77.0.53".parse().unwrap()],
        domains: vec!["corp.example".into()],
    };
    host.put("dns-fail", "");
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.dns, "unsupported");
    assert!(host.log("dns.log").ends_with("revert atenea0\n"));
    fs::remove_file(host.root.join("dns-fail")).unwrap();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.dns, "applied");
    fs::remove_file(host.root.join("resolvectl")).unwrap();
    cfg.dns.domains.push("other.example".into());
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.dns, "unsupported");
}

#[test]
fn snat_uses_only_signed_true_flags_and_replaces_in_one_transaction() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    cfg.advertised = vec![
        AdvertisedRoute {
            prefix: prefix("192.168.50.0/24"),
            snat: true,
        },
        AdvertisedRoute {
            prefix: prefix("192.168.60.0/24"),
            snat: false,
        },
        AdvertisedRoute {
            prefix: prefix("fd50::/64"),
            snat: true,
        },
    ];
    net.apply(&cfg, &[]).unwrap();
    net.apply(&cfg, &[]).unwrap();
    let first = host.log("transactions");
    assert_eq!(first.matches("--- transaction ---").count(), 1);
    assert!(first.contains("table inet atenea_atenea0"));
    assert!(first.contains(
        "iifname \"atenea0\" oifname != \"atenea0\" ip daddr 192.168.50.0/24 masquerade"
    ));
    assert!(first.contains("ip6 daddr fd50::/64 masquerade"));
    assert!(!first.contains("192.168.60.0"));
    cfg.advertised[0].snat = false;
    net.apply(&cfg, &[]).unwrap();
    let transactions = host.log("transactions");
    let replacement = transactions.strip_prefix(&first).unwrap();
    assert!(replacement.starts_with(
        "--- transaction ---\ndelete table inet atenea_atenea0\ntable inet atenea_atenea0"
    ));
    assert!(!replacement.contains("192.168.50.0"));
    assert!(replacement.contains("fd50::/64 masquerade"));
    assert_eq!(
        host.log("nft.log")
            .lines()
            .filter(|line| *line == "-f -")
            .count(),
        2
    );
    assert!(!host.log("nft.log").contains("delete table"));
    host.put(
        "tables.json",
        r#"{"nftables":[{"table":{"family":"inet","name":"atenea_atenea0"}}]}"#,
    );
    cfg.advertised[2].snat = false;
    net.apply(&cfg, &[]).unwrap();
    assert!(net.snat.is_none());
    assert!(
        host.log("nft.log")
            .contains("delete table inet atenea_atenea0\n")
    );
    assert_eq!(host.log("transactions"), transactions);
}

#[test]
fn cleanup_of_an_absent_table_is_idempotent_and_preserves_other_tables() {
    let host = Host::new();
    host.put(
        "tables.json",
        r#"{"nftables":[{"table":{"family":"inet","name":"unrelated"}}]}"#,
    );
    let mut net = host.netconf();
    net.cleanup().unwrap();
    net.cleanup().unwrap();
    assert_eq!(host.log("nft.log"), "-j list tables\n-j list tables\n");
    assert!(net.snat.is_none());
}

#[test]
fn forwarding_is_read_for_each_advertised_family_and_never_written() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().forwarding_ok, None);
    cfg.advertised = vec![AdvertisedRoute {
        prefix: prefix("192.168.50.0/24"),
        snat: false,
    }];
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().forwarding_ok, Some(false));
    host.put("proc_sys/net/ipv4/ip_forward", "1\n");
    assert_eq!(net.status().forwarding_ok, Some(true));
    cfg.advertised.push(AdvertisedRoute {
        prefix: prefix("fd50::/64"),
        snat: false,
    });
    host.put("proc_sys/net/ipv6/conf/all/forwarding", "0\n");
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().forwarding_ok, Some(false));
    assert_eq!(host.log("proc_sys/net/ipv6/conf/all/forwarding"), "0\n");
    host.put("proc_sys/net/ipv6/conf/all/forwarding", "1\n");
    assert_eq!(net.status().forwarding_ok, Some(true));
    assert_eq!(host.log("proc_sys/net/ipv4/ip_forward"), "1\n");
    assert_eq!(host.log("proc_sys/net/ipv6/conf/all/forwarding"), "1\n");
    assert!(!host.log("ip.log").contains("sysctl"));
    host.put("proc_sys/net/ipv4/ip_forward", "0\n");
    assert_eq!(net.status().forwarding_ok, Some(false));
    cfg.advertised.clear();
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().forwarding_ok, None);
}

#[test]
fn unresolved_endpoints_defer_routes_and_remove_them_until_resolution_recovers() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    cfg.peers[0].routes.push(prefix("fd40::/64"));

    net.endpoints_resolved(false);
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status.routes.len(), 2);
    assert!(net.status.routes.iter().all(|route| {
        route.state == "pending" && route.reason.as_deref() == Some("endpoints_unresolved")
    }));
    assert!(net.installed.is_empty());
    assert!(!host.log("ip.log").contains(" route add "));

    net.endpoints_resolved(true);
    net.apply(&cfg, &[]).unwrap();
    assert!(
        net.status
            .routes
            .iter()
            .all(|route| { route.state == "installed" && route.reason.is_none() })
    );
    assert_eq!(net.installed.len(), 2);

    net.endpoints_resolved(false);
    net.apply(&cfg, &[]).unwrap();
    assert!(net.status.routes.iter().all(|route| {
        route.state == "pending" && route.reason.as_deref() == Some("endpoints_unresolved")
    }));
    assert!(net.installed.is_empty());
    let commands: Vec<_> = host
        .log("ip.log")
        .lines()
        .filter(|line| !line.starts_with("-j "))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        commands,
        [
            "-4 route add 192.168.40.0/24 dev atenea0",
            "-6 route add fd40::/64 dev atenea0",
            "-4 route del 192.168.40.0/24 dev atenea0",
            "-6 route del fd40::/64 dev atenea0",
        ]
    );
}

#[test]
fn withdrawing_peer_routes_preserves_dns_and_removes_both_families() {
    let host = Host::new();
    let mut net = host.netconf();
    let mut cfg = config();
    cfg.peers[0].routes = vec![prefix("192.168.50.0/24"), prefix("fd50::/64")];
    cfg.dns = DnsConfig {
        servers: vec!["10.77.0.2".parse().unwrap()],
        domains: vec!["lab.internal".into()],
    };
    net.apply(&cfg, &[]).unwrap();
    assert_eq!(net.status().routes.len(), 2);
    let dns_commands = host.log("dns.log");
    cfg.config_version += 1;
    cfg.peers[0].routes.clear();
    assert_eq!(cfg.encode().unwrap()[0], 2);
    net.apply(&cfg, &[]).unwrap();
    assert!(net.status().routes.is_empty());
    assert!(net.installed.is_empty());
    assert_eq!(net.status().dns, "applied");
    assert_eq!(host.log("dns.log"), dns_commands);
    let deletions: Vec<String> = host
        .log("ip.log")
        .lines()
        .filter(|line| line.contains(" route del "))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        deletions,
        [
            "-4 route del 192.168.50.0/24 dev atenea0",
            "-6 route del fd50::/64 dev atenea0",
        ]
    );
}
