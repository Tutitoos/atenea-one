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
