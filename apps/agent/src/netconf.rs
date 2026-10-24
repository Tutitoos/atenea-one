//! Cambios por enlace: nunca modifica rutas por defecto, sysctls ni resolv.conf.
use atenea_identity::{DeviceConfig, DnsConfig, Prefix};
use atenea_policies::prefix_contains;
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Write;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Clone)]
pub struct Binaries {
    pub ip: PathBuf,
    pub nft: PathBuf,
    pub resolvectl: PathBuf,
    pub proc_sys: PathBuf,
}
impl Default for Binaries {
    fn default() -> Self {
        Self {
            ip: "ip".into(),
            nft: "nft".into(),
            resolvectl: "resolvectl".into(),
            proc_sys: "/proc/sys".into(),
        }
    }
}
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
pub struct NetConf {
    interface: String,
    bins: Binaries,
    installed: BTreeSet<String>,
    dns: Option<DnsConfig>,
    snat: Option<String>,
    endpoints_resolved: bool,
    forwarding_families: [bool; 2],
    status: Status,
}
fn cidr(p: &Prefix) -> String {
    format!("{}/{}", p.addr, p.len)
}
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
fn run(bin: &PathBuf, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .map_err(|_| "binario no disponible".to_string())?;
    if !out.status.success() {
        return Err("orden de red fallida".into());
    }
    Ok(out.stdout)
}
impl NetConf {
    pub fn new(interface: String, bins: Binaries) -> Self {
        Self {
            interface,
            bins,
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
                    .zip(["net/ipv4/ip_forward", "net/ipv6/conf/all/forwarding"])
                    .all(|(required, file)| {
                        !required
                            || std::fs::read_to_string(self.bins.proc_sys.join(file))
                                .is_ok_and(|value| value.trim() == "1")
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
    pub fn cleanup(&mut self) -> Result<(), String> {
        let table = format!("atenea_{}", self.interface);
        // Consultar primero permite distinguir tabla ausente de un error de permisos.
        let tables = run(&self.bins.nft, &["-j", "list", "tables"])?;
        let tables: serde_json::Value =
            serde_json::from_slice(&tables).map_err(|_| "respuesta nft inválida")?;
        let exists = tables["nftables"]
            .as_array()
            .ok_or("respuesta nft inválida")?
            .iter()
            .any(|v| v["table"]["family"] == "inet" && v["table"]["name"] == table);
        if exists {
            run(&self.bins.nft, &["delete", "table", "inet", &table])?;
        }
        self.snat = None;
        Ok(())
    }
    fn route(&self, verb: &str, prefix: &str) -> Result<(), String> {
        let family = if prefix.contains(':') { "-6" } else { "-4" };
        run(
            &self.bins.ip,
            &[family, "route", verb, prefix, "dev", &self.interface],
        )
        .map(|_| ())
    }
    fn existing_routes(&self) -> Result<Vec<Prefix>, String> {
        let mut routes = Vec::new();
        for family in ["-4", "-6"] {
            let bytes = run(
                &self.bins.ip,
                &["-j", family, "route", "show", "table", "all"],
            )?;
            let entries: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "respuesta ip inválida")?;
            for entry in entries.as_array().ok_or("respuesta ip inválida")? {
                let dst = entry["dst"].as_str().ok_or("ruta sin destino")?;
                if dst == "default" || entry["dev"].as_str() == Some(&self.interface) {
                    continue;
                }
                let prefix = parse_prefix(dst).ok_or("prefijo del host inválido")?;
                if prefix.len != 0 {
                    routes.push(prefix);
                }
            }
        }
        Ok(routes)
    }
    pub fn apply(&mut self, cfg: &DeviceConfig, protected: &[IpAddr]) -> Result<(), String> {
        let wanted: Vec<&Prefix> = cfg.peers.iter().flat_map(|p| &p.routes).collect();
        let wanted_keys: BTreeSet<String> = wanted.iter().map(|p| cidr(p)).collect();
        let removed: Vec<String> = self.installed.difference(&wanted_keys).cloned().collect();
        for p in removed {
            self.route("del", &p)?;
            self.installed.remove(&p);
        }
        let existing = if wanted.is_empty() {
            Ok(Vec::new())
        } else {
            self.existing_routes()
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
                _ if protected.iter().any(|a| prefix_contains(p, a))
                    || cfg
                        .peers
                        .iter()
                        .flat_map(|p| &p.endpoints)
                        .any(|ep| prefix_contains(p, &ep.ip())) =>
                {
                    Some("cubre un endpoint del plano de control".into())
                }
                _ => None,
            };
            let reason = if reason.is_some() {
                if self.installed.contains(&text) {
                    self.route("del", &text)?;
                    self.installed.remove(&text);
                }
                reason
            } else if !self.installed.contains(&text) {
                match self.route("add", &text) {
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
        self.apply_snat(cfg)?;
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
                && run(&self.bins.resolvectl, &["revert", &self.interface]).is_err()
            {
                "unsupported"
            } else {
                "none"
            };
        } else {
            let servers: Vec<String> = dns.servers.iter().map(ToString::to_string).collect();
            let domains: Vec<String> = dns.domains.iter().map(|s| format!("~{s}")).collect();
            let mut args = vec!["dns", self.interface.as_str()];
            args.extend(servers.iter().map(String::as_str));
            let result = run(&self.bins.resolvectl, &args)
                .and_then(|_| {
                    let mut args = vec!["domain", self.interface.as_str()];
                    args.extend(domains.iter().map(String::as_str));
                    run(&self.bins.resolvectl, &args)
                })
                .and_then(|_| {
                    run(
                        &self.bins.resolvectl,
                        &["default-route", &self.interface, "no"],
                    )
                });
            self.status.dns = if result.is_ok() {
                "applied"
            } else {
                let _ = run(&self.bins.resolvectl, &["revert", &self.interface]);
                "unsupported"
            };
        }
        self.dns = Some(dns.clone());
    }
    fn apply_snat(&mut self, cfg: &DeviceConfig) -> Result<(), String> {
        let table = format!("atenea_{}", self.interface);
        let mut script = format!(
            "table inet {table} {{\n chain postrouting {{\n type nat hook postrouting priority srcnat; policy accept;\n"
        );
        for r in cfg.advertised.iter().filter(|r| r.snat) {
            let family = if r.prefix.addr.is_ipv4() { "ip" } else { "ip6" };
            script.push_str(&format!(
                "iifname \"{}\" oifname != \"{}\" {family} daddr {} masquerade\n",
                self.interface,
                self.interface,
                cidr(&r.prefix)
            ));
        }
        script.push_str(" }\n}\n");
        if self.snat.as_ref() == Some(&script) {
            return Ok(());
        }
        if !cfg.advertised.iter().any(|r| r.snat) {
            if self.snat.is_some() {
                self.cleanup()?;
            }
            return Ok(());
        }
        let transaction = if self.snat.is_some() {
            format!("delete table inet {table}\n{script}")
        } else {
            script.clone()
        };
        let mut child = Command::new(&self.bins.nft)
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "nft no disponible")?;
        let write = child
            .stdin
            .take()
            .ok_or("stdin nft no disponible")?
            .write_all(transaction.as_bytes());
        let status = child.wait().map_err(|_| "no se pudo esperar nft")?;
        write.map_err(|_| "no se pudo escribir nft")?;
        if !status.success() {
            return Err("transacción nft fallida".into());
        }
        self.snat = Some(script);
        Ok(())
    }
}
impl Drop for NetConf {
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

#[cfg(test)]
#[path = "netconf_tests.rs"]
mod integration_tests;
