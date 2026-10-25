//! Cambios por enlace: nunca modifica rutas por defecto, sysctls ni resolv.conf.
#[cfg(target_os = "linux")]
pub use crate::platform::linux::Binaries;
#[cfg(target_os = "macos")]
pub use crate::platform::macos::Binaries;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use crate::platform::unsupported::Binaries;
#[cfg(target_os = "windows")]
pub use crate::platform::windows::Binaries;
use crate::platform::{HostNetwork, Platform};
use atenea_identity::{AdvertisedRoute, DeviceConfig, DnsConfig, Prefix};
use atenea_policies::prefix_contains;
use serde::Serialize;
use std::collections::BTreeSet;
use std::net::IpAddr;
#[derive(Clone, Debug, Serialize)]
pub struct RouteStatus {
    pub prefix: String,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Status {
    pub routes: Vec<RouteStatus>,
    pub dns: &'static str,
    pub forwarding_ok: Option<bool>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            dns: "none",
            forwarding_ok: None,
        }
    }
}
pub(crate) struct NetConf<H: HostNetwork = Platform> {
    host: H,
    installed: BTreeSet<String>,
    dns: Option<DnsConfig>,
    snat: Option<Vec<AdvertisedRoute>>,
    endpoints_resolved: bool,
    forwarding_families: [bool; 2],
    status: Status,
}
fn cidr(p: &Prefix) -> String {
    format!("{}/{}", p.addr, p.len)
}
#[cfg(test)]
fn parse_prefix(text: &str) -> Option<Prefix> {
    let (addr, len) = text
        .split_once('/')
        .map_or((text, None), |(a, l)| (a, Some(l)));
    let addr: IpAddr = addr.parse().ok()?;
    let len = match len {
        Some(l) => l.parse().ok()?,
        None => {
            if addr.is_ipv4() {
                32
            } else {
                128
            }
        }
    };
    if len > if addr.is_ipv4() { 32 } else { 128 } {
        return None;
    }
    Some(Prefix { addr, len })
}
fn overlaps(a: &Prefix, b: &Prefix) -> bool {
    prefix_contains(a, &b.addr) || prefix_contains(b, &a.addr)
}
impl NetConf<Platform> {
    pub fn new(interface: String, bins: Binaries) -> Self {
        Self::with_host(Platform::new(interface, bins))
    }
    #[cfg(target_os = "windows")]
    pub fn remove_orphan_adapter(&self) -> Result<(), String> {
        self.host.remove_orphan_adapter()
    }
}
impl<H: HostNetwork> NetConf<H> {
    pub fn with_host(host: H) -> Self {
        Self {
            host,
            installed: BTreeSet::new(),
            dns: None,
            snat: None,
            endpoints_resolved: true,
            forwarding_families: [false; 2],
            status: Status::default(),
        }
    }
    pub fn status(&self) -> Status {
        let mut status = self.status.clone();
        status.forwarding_ok = if self.forwarding_families.iter().any(|enabled| *enabled) {
            Some(
                self.forwarding_families
                    .iter()
                    .zip([false, true])
                    .all(|(required, ipv6)| {
                        !required
                            || (self.host.supports_subnet_routing() && self.host.forwarding(ipv6))
                    }),
            )
        } else {
            None
        };
        status
    }
    pub fn endpoints_resolved(&mut self, resolved: bool) {
        self.endpoints_resolved = resolved;
    }
    pub fn configure_link(&self, mtu: usize, addrs: &[(IpAddr, u8)]) -> Result<(), String> {
        self.host.configure_link(mtu, addrs)
    }
    pub fn cleanup(&mut self) -> Result<(), String> {
        self.host.cleanup()?;
        self.snat = None;
        #[cfg(target_os = "windows")]
        {
            // Windows retira rutas y NRPT; una recarga debe volver a instalarlos.
            self.installed.clear();
            self.dns = None;
        }
        Ok(())
    }
    pub fn apply(&mut self, cfg: &DeviceConfig, protected: &[IpAddr]) -> Result<(), String> {
        let wanted: Vec<&Prefix> = cfg.peers.iter().flat_map(|p| &p.routes).collect();
        let wanted_keys: BTreeSet<String> = wanted.iter().map(|p| cidr(p)).collect();
        let removed: Vec<String> = self.installed.difference(&wanted_keys).cloned().collect();
        for p in removed {
            self.host.remove_route(&p)?;
            self.installed.remove(&p);
        }
        let existing = if wanted.is_empty() {
            Ok(Vec::new())
        } else {
            self.host.existing_routes()
        };
        let mut reports = Vec::new();
        for p in wanted {
            let text = cidr(p);
            let reason = match &existing {
                _ if !self.endpoints_resolved => Some("endpoints_unresolved".into()),
                Err(e) => Some(format!("no se pudo comprobar conflictos: {e}")),
                Ok(routes) if routes.iter().any(|r| overlaps(p, r)) => {
                    Some("solapa una ruta de otra interfaz".into())
                }
                _ if protected.iter().any(|a| prefix_contains(p, a)) => {
                    Some("cubre un endpoint del plano de control".into())
                }
                _ => None,
            };
            let reason = if reason.is_some() {
                if self.installed.contains(&text) {
                    self.host.remove_route(&text)?;
                    self.installed.remove(&text);
                }
                reason
            } else if !self.installed.contains(&text) {
                match self.host.add_route(&text) {
                    Ok(()) => {
                        self.installed.insert(text.clone());
                        None
                    }
                    Err(e) => Some(e),
                }
            } else {
                None
            };
            reports.push(RouteStatus {
                prefix: text,
                state: if !self.endpoints_resolved {
                    "pending"
                } else if reason.is_some() {
                    "conflict"
                } else {
                    "installed"
                },
                reason,
            });
        }
        self.status.routes = reports;
        if self.host.supports_subnet_routing() {
            self.apply_snat(cfg)?;
        } else {
            self.status
                .routes
                .extend(cfg.advertised.iter().map(|r| RouteStatus {
                    prefix: cidr(&r.prefix),
                    state: "unsupported",
                    reason: Some("esta plataforma no admite servir subredes".into()),
                }));
        }
        self.apply_dns(&cfg.dns);
        self.forwarding_families = [
            cfg.advertised
                .iter()
                .any(|route| route.prefix.addr.is_ipv4()),
            cfg.advertised
                .iter()
                .any(|route| route.prefix.addr.is_ipv6()),
        ];
        Ok(())
    }
    fn apply_dns(&mut self, dns: &DnsConfig) {
        if self.dns.as_ref() == Some(dns) && self.status.dns != "unsupported" {
            return;
        }
        if dns.servers.is_empty() || dns.domains.is_empty() {
            self.status.dns = if self
                .dns
                .as_ref()
                .is_some_and(|d| !d.servers.is_empty() && !d.domains.is_empty())
                && self.host.clear_dns().is_err()
            {
                "unsupported"
            } else {
                "none"
            };
        } else {
            let result = self.host.set_dns(dns);
            self.status.dns = if result.is_ok() {
                "applied"
            } else {
                let _ = self.host.clear_dns();
                "unsupported"
            };
        }
        self.dns = Some(dns.clone());
    }
    fn apply_snat(&mut self, cfg: &DeviceConfig) -> Result<(), String> {
        let routes: Vec<AdvertisedRoute> =
            cfg.advertised.iter().filter(|r| r.snat).cloned().collect();
        if self.snat.as_ref() == Some(&routes) {
            return Ok(());
        }
        if routes.is_empty() {
            if self.snat.is_some() {
                self.cleanup()?;
            }
            return Ok(());
        }
        self.host.set_snat(&routes, self.snat.is_some())?;
        self.snat = Some(routes);
        Ok(())
    }
}
impl<H: HostNetwork> Drop for NetConf<H> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicts_ignore_other_families_but_include_containment() {
        let subnet = parse_prefix("192.168.50.0/24").unwrap();
        assert!(overlaps(&subnet, &parse_prefix("192.168.0.0/16").unwrap()));
        assert!(overlaps(&subnet, &parse_prefix("192.168.50.1").unwrap()));
        assert!(!overlaps(&subnet, &parse_prefix("fd00::/64").unwrap()));
        assert!(parse_prefix("10.0.0.0/33").is_none());
    }
}

#[cfg(all(test, unix))]
#[path = "netconf_tests.rs"]
mod integration_tests;

#[cfg(test)]
#[path = "netconf_policy_tests.rs"]
mod policy_tests;
