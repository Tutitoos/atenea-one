// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Cliente macOS: órdenes nativas y resolutores por dominio, sin tocar DNS global ni pf.
use super::HostNetwork;
#[cfg(all(test, unix))]
use crate::testutil::output;
use atenea_identity::{AdvertisedRoute, DnsConfig, Prefix};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
const OWNER: &str = "# Atenea One: resolver administrado\n";
#[derive(Clone)]
pub struct Binaries {
    pub ifconfig: PathBuf,
    pub route: PathBuf,
    pub netstat: PathBuf,
    pub resolver_dir: PathBuf,
}
impl Default for Binaries {
    fn default() -> Self {
        Self {
            ifconfig: "/sbin/ifconfig".into(),
            route: "/sbin/route".into(),
            netstat: "/usr/sbin/netstat".into(),
            resolver_dir: "/etc/resolver".into(),
        }
    }
}
pub struct Network {
    interface: String,
    bins: Binaries,
    resolver_marker: String,
}
#[cfg(not(all(test, unix)))]
fn output(command: &mut Command) -> std::io::Result<std::process::Output> {
    command.output()
}
fn run(bin: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out =
        output(Command::new(bin).args(args)).map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} {}: {}",
            bin.display(),
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}
fn prefix(text: &str, host: bool) -> Result<Prefix, String> {
    let (addr, len) = text
        .split_once('/')
        .map_or((text, None), |(a, l)| (a, Some(l)));
    let addr = addr.split('%').next().ok_or("ruta inválida")?;
    let addr: IpAddr = if addr.contains(':') {
        addr.parse().map_err(|_| "ruta IPv6 inválida")?
    } else {
        let mut parts: Vec<&str> = addr.split('.').collect();
        if parts.len() > 4 {
            return Err("ruta IPv4 inválida".into());
        }
        parts.resize(4, "0");
        parts.join(".").parse().map_err(|_| "ruta IPv4 inválida")?
    };
    let len = match len {
        Some(l) => l.parse::<u8>().map_err(|_| "máscara inválida")?,
        None if host => {
            if addr.is_ipv4() {
                32
            } else {
                128
            }
        }
        None if addr.is_ipv4() => (text.split('.').count() * 8) as u8,
        None => return Err("ruta sin máscara".into()),
    };
    if len > if addr.is_ipv4() { 32 } else { 128 } {
        return Err("máscara inválida".into());
    }
    Ok(Prefix { addr, len })
}
pub(crate) fn parse_routes(text: &str, skip: &str) -> Result<Vec<Prefix>, String> {
    let mut out = Vec::new();
    let mut iface_column = None;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.first() == Some(&"Destination") {
            iface_column = fields.iter().position(|s| *s == "Netif");
            continue;
        }
        let Some(column) = iface_column else {
            continue;
        };
        if fields.is_empty() {
            continue;
        }
        if fields.len() <= column || fields.len() < 3 {
            return Err("salida netstat inválida".into());
        }
        if fields[0] == "default" || fields[column] == skip {
            continue;
        }
        let p = prefix(fields[0], fields[2].contains('H'))?;
        if p.len != 0 {
            out.push(p);
        }
    }
    if iface_column.is_none() {
        return Err("netstat sin cabecera de rutas".into());
    }
    Ok(out)
}
fn parse_addrs(text: &str, skip: &str, port: u16) -> Vec<SocketAddr> {
    let mut iface = "";
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            iface = line.split(':').next().unwrap_or("");
        }
        let words: Vec<_> = line.split_whitespace().collect();
        if iface == skip
            || words.len() < 2
            || !matches!(words[0], "inet" | "inet6")
            || words
                .iter()
                .any(|s| matches!(*s, "tentative" | "duplicated" | "deprecated"))
        {
            continue;
        }
        if let Ok(ip) = words[1].parse::<IpAddr>() {
            let addr = SocketAddr::new(ip, port);
            if atenea_nat_traversal::candidate_ok(&addr) && !out.contains(&addr) {
                out.push(addr);
            }
        }
    }
    out
}
fn domain_ok(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
fn owned(path: &Path, marker: &str) -> Result<bool, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Ok(false);
    }
    // Solo importa el prefijo: un resolutor ajeno no UTF-8 no debe impedir la limpieza.
    let mut head = Vec::with_capacity(marker.len());
    std::fs::File::open(path)
        .and_then(|f| f.take(marker.len() as u64).read_to_end(&mut head))
        .map_err(|e| e.to_string())?;
    Ok(head == marker.as_bytes())
}
impl Network {
    pub fn new(interface: String, bins: Binaries) -> Self {
        Self {
            interface,
            bins,
            resolver_marker: OWNER.into(),
        }
    }
    pub fn with_resolver_owner(mut self, owner: Option<String>) -> Self {
        if let Some(owner) = owner {
            self.resolver_marker = format!("# Atenea One: resolver administrado ({owner})\n");
        }
        self
    }
    #[cfg_attr(test, allow(dead_code))]
    pub fn ensure_available() -> Result<(), String> {
        Ok(())
    }
    fn route(&self, verb: &str, p: &str) -> Result<(), String> {
        let family = if p.contains(':') { "-inet6" } else { "-inet" };
        run(
            &self.bins.route,
            &["-n", verb, family, "-net", p, "-interface", &self.interface],
        )
        .map(|_| ())
    }
}
impl HostNetwork for Network {
    fn supports_subnet_routing(&self) -> bool {
        false
    }
    fn configure_link(&self, mtu: usize, addrs: &[(IpAddr, u8)]) -> Result<(), String> {
        run(
            &self.bins.ifconfig,
            &[&self.interface, "mtu", &mtu.to_string(), "up"],
        )?;
        let current = run(&self.bins.ifconfig, &[&self.interface])?;
        for line in String::from_utf8_lossy(&current).lines() {
            let words: Vec<_> = line.split_whitespace().collect();
            if words.len() >= 2
                && matches!(words[0], "inet" | "inet6")
                && words[1]
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| !matches!(ip, IpAddr::V6(v) if v.is_unicast_link_local()))
            {
                run(
                    &self.bins.ifconfig,
                    &[&self.interface, words[0], words[1], "-alias"],
                )?;
            }
        }
        // utun es punto a punto; repetir la dirección evita inventar un gateway remoto.
        for (addr, len) in addrs {
            let addr = addr.to_string();
            let len = len.to_string();
            if addr.contains(':') {
                run(&self.bins.ifconfig, &[&self.interface, "inet6", "-dad"])?;
                run(
                    &self.bins.ifconfig,
                    &[&self.interface, "inet6", &addr, "prefixlen", &len, "alias"],
                )?;
            } else {
                let bits: u32 = len.parse().map_err(|_| "prefijo inválido")?;
                let mask_bits = u32::MAX
                    .checked_shl(32u32.checked_sub(bits).ok_or("prefijo inválido")?)
                    .unwrap_or(0);
                let mask = std::net::Ipv4Addr::from(mask_bits).to_string();
                run(
                    &self.bins.ifconfig,
                    &[
                        &self.interface,
                        "inet",
                        &addr,
                        &addr,
                        "netmask",
                        &mask,
                        "alias",
                    ],
                )?;
                let ip: std::net::Ipv4Addr = addr.parse().map_err(|_| "IPv4 inválida")?;
                let network = format!(
                    "{}/{}",
                    std::net::Ipv4Addr::from(u32::from(ip) & mask_bits),
                    bits
                );
                if let Err(error) = self.route("add", &network) {
                    let details = run(&self.bins.route, &["-n", "get", "-inet", "-net", &network])?;
                    if !String::from_utf8_lossy(&details)
                        .lines()
                        .any(|line| line.split_whitespace().eq(["interface:", &self.interface]))
                    {
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
    fn existing_routes(&self) -> Result<Vec<Prefix>, String> {
        let mut out = Vec::new();
        for family in ["inet", "inet6"] {
            out.extend(parse_routes(
                &String::from_utf8_lossy(&run(&self.bins.netstat, &["-rn", "-f", family])?),
                &self.interface,
            )?);
        }
        Ok(out)
    }
    fn add_route(&self, p: &str) -> Result<(), String> {
        self.route("add", p)
    }
    fn remove_route(&self, p: &str) -> Result<(), String> {
        self.route("delete", p)
    }
    fn dns_domain_conflict(&self, dns: &DnsConfig) -> Result<Option<String>, String> {
        if dns.domains.iter().any(|d| !domain_ok(d)) {
            return Err("dominio DNS de enrutamiento inválido".into());
        }
        let entries = match std::fs::read_dir(&self.bins.resolver_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            if let Some(domain) = dns.domains.iter().find(|domain| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(domain))
            }) && !owned(&entry.path(), &self.resolver_marker)?
            {
                return Ok(Some(format!(
                    "dominio DNS {domain} reclamado por otro resolutor"
                )));
            }
        }
        Ok(None)
    }
    fn clear_dns(&self) -> Result<(), String> {
        let entries = match std::fs::read_dir(&self.bins.resolver_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if owned(&path, &self.resolver_marker)? {
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    fn set_dns(&self, dns: &DnsConfig) -> Result<(), String> {
        if let Some(detail) = self.dns_domain_conflict(dns)? {
            return Err(detail);
        }
        std::fs::create_dir_all(&self.bins.resolver_dir).map_err(|e| e.to_string())?;
        self.clear_dns()?;
        let domains: std::collections::BTreeSet<_> = dns
            .domains
            .iter()
            .map(|domain| domain.to_ascii_lowercase())
            .collect();
        for domain in domains {
            let result = (|| {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o644)
                    .open(self.bins.resolver_dir.join(domain))
                    .map_err(|e| e.to_string())?;
                file.write_all(self.resolver_marker.as_bytes())
                    .map_err(|e| e.to_string())?;
                for server in &dns.servers {
                    writeln!(file, "nameserver {server}").map_err(|e| e.to_string())?;
                }
                file.sync_all().map_err(|e| e.to_string())
            })();
            if result.is_err() {
                let _ = self.clear_dns();
                return result;
            }
        }
        std::fs::File::open(&self.bins.resolver_dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
    fn set_snat(&self, _: &[AdvertisedRoute], _: bool) -> Result<(), String> {
        Err("macOS no admite servir subredes ni SNAT".into())
    }
    fn cleanup(&self) -> Result<(), String> {
        self.clear_dns()
    }
    fn forwarding(&self, _: bool) -> bool {
        false
    }
    async fn local_addrs(&self, port: u16) -> Vec<SocketAddr> {
        match tokio::process::Command::new(&self.bins.ifconfig)
            .output()
            .await
        {
            Ok(o) if o.status.success() => {
                parse_addrs(&String::from_utf8_lossy(&o.stdout), &self.interface, port)
            }
            _ => Vec::new(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn darwin_routes_and_addresses() {
        let routes = "Routing tables\n\nInternet:\nDestination Gateway Flags Netif Expire\ndefault 192.0.2.1 UGScg en0\n10.20/16 link#4 UCS en0 !\n10.77.0.1 10.77.0.1 UH utun9\n192.0.2.7 aa:bb:cc:00:00:00 UHLWI en0 100\n";
        let p = parse_routes(routes, "utun9").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].addr.to_string(), "10.20.0.0");
        assert_eq!(p[0].len, 16);
        assert_eq!(p[1].len, 32);
        let v6 = "Internet6:\nDestination Gateway Flags Netif Expire\ndefault fe80::1%en0 UGcIg en0\n::1 ::1 UHL lo0\nfe80::%en0/64 link#4 UCI en0\nfd77::/64 link#9 Uc utun9\n";
        assert_eq!(parse_routes(v6, "utun9").unwrap().len(), 2);
        assert!(parse_routes("bad", "utun9").is_err());
        let addrs = "en0: flags=8863<UP,BROADCAST,RUNNING> mtu 1500\n\tinet 192.0.2.7 netmask 0xffffff00 broadcast 192.0.2.255\n\tinet6 fe80::123%en0 prefixlen 64 secured scopeid 0x4\n\tinet6 2001:db8::7 prefixlen 64 autoconf secured\nutun9: flags=8051<UP,POINTOPOINT,RUNNING> mtu 1280\n\tinet 10.77.0.2 --> 10.77.0.2 netmask 0xffffff00\nlo0: flags=8049<UP,LOOPBACK,RUNNING> mtu 16384\n\tinet 127.0.0.1 netmask 0xff000000\n";
        let a = parse_addrs(addrs, "utun9", 51820);
        assert!(!a.iter().any(|a| a.ip().to_string() == "10.77.0.2"));
        assert!(!a.iter().any(|a| a.ip().is_loopback()));
    }
    #[test]
    fn resolver_ownership_and_validation() {
        let dir = std::env::temp_dir().join(format!("atenea-macos-dns-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let host = Network::new(
            "utun9".into(),
            Binaries {
                resolver_dir: dir.clone(),
                ..Default::default()
            },
        );
        let mut dns = DnsConfig {
            servers: vec!["10.77.0.1".parse().unwrap()],
            domains: vec!["lab.internal".into()],
        };
        host.set_dns(&dns).unwrap();
        assert!(
            std::fs::read_to_string(dir.join("lab.internal"))
                .unwrap()
                .contains("nameserver 10.77.0.1")
        );
        std::fs::write(dir.join("other.internal"), "nameserver 192.0.2.53\n").unwrap();
        // Un resolutor ajeno no UTF-8 no rompe la limpieza ni se borra.
        std::fs::write(dir.join("binary.internal"), [0xff, 0xfe, 0x00]).unwrap();
        std::fs::write(
            dir.join("stale.internal"),
            format!("{OWNER}nameserver 10.77.0.1\n"),
        )
        .unwrap();
        host.cleanup().unwrap();
        assert!(!dir.join("lab.internal").exists());
        assert!(!dir.join("stale.internal").exists());
        assert!(dir.join("binary.internal").exists());
        assert!(dir.join("other.internal").exists());
        dns.domains = vec!["other.internal".into()];
        assert!(host.set_dns(&dns).is_err());
        for d in ["", ".", "../bad", "a/b", "a\nb", "a..b"] {
            dns.domains = vec![d.into()];
            assert!(host.set_dns(&dns).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_y_dos_redes_aislan_resolutores_y_conflictos() {
        let dir = std::env::temp_dir().join(format!("atenea-dns-multired-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let host = |owner: Option<&str>| {
            Network::new(
                "utun9".into(),
                Binaries {
                    resolver_dir: dir.clone(),
                    ..Default::default()
                },
            )
            .with_resolver_owner(owner.map(str::to_owned))
        };
        let legacy = host(None);
        let a = host(Some("0000000a"));
        let b = host(Some("0000000b"));
        let dns = |domain: &str| DnsConfig {
            servers: vec!["10.77.0.1".parse().unwrap()],
            domains: vec![domain.into()],
        };
        legacy.set_dns(&dns("legacy.internal")).unwrap();
        a.set_dns(&dns("red-a.internal")).unwrap();
        b.set_dns(&dns("red-b.internal")).unwrap();
        std::fs::write(dir.join("ajeno.internal"), "nameserver 192.0.2.53\n").unwrap();
        let before = std::fs::read(dir.join("red-b.internal")).unwrap();
        for other in [&legacy, &a] {
            assert!(
                other
                    .dns_domain_conflict(&dns("RED-B.INTERNAL"))
                    .unwrap()
                    .is_some()
            );
            assert!(other.set_dns(&dns("red-b.internal")).is_err());
            assert_eq!(std::fs::read(dir.join("red-b.internal")).unwrap(), before);
        }
        assert_eq!(a.dns_domain_conflict(&dns("red-a.internal")).unwrap(), None);
        assert!(
            a.dns_domain_conflict(&dns("ajeno.internal"))
                .unwrap()
                .is_some()
        );
        a.cleanup().unwrap();
        assert!(!dir.join("red-a.internal").exists());
        assert!(dir.join("legacy.internal").exists());
        assert!(dir.join("red-b.internal").exists());
        assert!(dir.join("ajeno.internal").exists());
        legacy.cleanup().unwrap();
        assert!(dir.join("red-b.internal").exists());
        b.cleanup().unwrap();
        assert!(dir.join("ajeno.internal").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preflight_falla_cerrado_si_no_puede_leer_resolutores() {
        let dir = std::env::temp_dir().join(format!("atenea-dns-error-{}", std::process::id()));
        std::fs::write(&dir, "no es directorio").unwrap();
        let host = Network::new(
            "utun9".into(),
            Binaries {
                resolver_dir: dir.clone(),
                ..Default::default()
            },
        );
        let dns = DnsConfig {
            servers: vec!["10.77.0.1".parse().unwrap()],
            domains: vec!["red.internal".into()],
        };
        assert!(host.dns_domain_conflict(&dns).is_err());
        assert!(host.set_dns(&dns).is_err());
        std::fs::remove_file(dir).unwrap();
    }
}
#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn commands_use_injected_binaries() {
        let dir =
            std::env::temp_dir().join(format!("atenea-macos-commands-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let bin = dir.join("fake");
        let log = dir.join("args");
        crate::testutil::fake_binary(
            &bin,
            &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        );
        let host = Network::new(
            "utun9".into(),
            Binaries {
                ifconfig: bin.clone(),
                route: bin.clone(),
                netstat: bin,
                resolver_dir: dir.join("resolver"),
            },
        );
        host.configure_link(
            1280,
            &[
                ("10.77.0.2".parse().unwrap(), 24),
                ("fd77::2".parse().unwrap(), 64),
            ],
        )
        .unwrap();
        host.add_route("10.88.0.0/24").unwrap();
        host.remove_route("fd88::/64").unwrap();
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("utun9 mtu 1280 up"));
        assert!(calls.contains("utun9 inet6 -dad"));
        assert!(calls.contains("utun9 inet6 fd77::2 prefixlen 64 alias"));
        assert!(calls.contains("-n add -inet -net 10.88.0.0/24 -interface utun9"));
        assert!(calls.contains("-n delete -inet6 -net fd88::/64 -interface utun9"));
        assert!(!host.forwarding(false));
        assert!(host.set_snat(&[], false).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
