//! Órdenes Linux existentes, separadas de la política de rutas y estados.
use super::HostNetwork;
use atenea_identity::{AdvertisedRoute, DnsConfig, Prefix};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
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
pub struct Network {
    interface: String,
    bins: Binaries,
}
impl Network {
    pub fn new(interface: String, bins: Binaries) -> Self {
        Self { interface, bins }
    }
    pub fn ensure_available() -> Result<(), String> {
        Ok(())
    }
    fn configure_ip(&self, args: &[&str]) -> Result<(), String> {
        let out = Command::new(&self.bins.ip)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "ip {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
    fn route(&self, verb: &str, prefix: &str) -> Result<(), String> {
        let family = if prefix.contains(':') { "-6" } else { "-4" };
        run(
            &self.bins.ip,
            &[family, "route", verb, prefix, "dev", &self.interface],
        )
        .map(|_| ())
    }
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
impl HostNetwork for Network {
    fn supports_subnet_routing(&self) -> bool {
        true
    }
    fn configure_link(&self, mtu: usize, addrs: &[(IpAddr, u8)]) -> Result<(), String> {
        self.configure_ip(&[
            "link",
            "set",
            "dev",
            &self.interface,
            "mtu",
            &mtu.to_string(),
            "up",
        ])?;
        self.configure_ip(&["addr", "flush", "dev", &self.interface])?;
        for (addr, prefix) in addrs {
            let cidr = format!("{addr}/{prefix}");
            let mut args = vec!["addr", "add", &cidr, "dev", &self.interface];
            if addr.is_ipv6() {
                args.push("nodad");
            }
            self.configure_ip(&args)?;
        }
        Ok(())
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
    fn add_route(&self, prefix: &str) -> Result<(), String> {
        self.route("add", prefix)
    }
    fn remove_route(&self, prefix: &str) -> Result<(), String> {
        self.route("del", prefix)
    }
    fn clear_dns(&self) -> Result<(), String> {
        run(&self.bins.resolvectl, &["revert", &self.interface]).map(|_| ())
    }
    fn set_dns(&self, dns: &DnsConfig) -> Result<(), String> {
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
        result.map(|_| ())
    }
    fn cleanup(&self) -> Result<(), String> {
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
        Ok(())
    }
    fn set_snat(&self, routes: &[AdvertisedRoute], replace: bool) -> Result<(), String> {
        let table = format!("atenea_{}", self.interface);
        let mut script = format!(
            "table inet {table} {{\n chain postrouting {{\n type nat hook postrouting priority srcnat; policy accept;\n"
        );
        for r in routes {
            let family = if r.prefix.addr.is_ipv4() { "ip" } else { "ip6" };
            script.push_str(&format!(
                "iifname \"{}\" oifname != \"{}\" {family} daddr {}/{} masquerade\n",
                self.interface, self.interface, r.prefix.addr, r.prefix.len
            ));
        }
        script.push_str(" }\n}\n");
        let transaction = if replace {
            format!("delete table inet {table}\n{script}")
        } else {
            script
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
        Ok(())
    }
    fn forwarding(&self, ipv6: bool) -> bool {
        let file = if ipv6 {
            "net/ipv6/conf/all/forwarding"
        } else {
            "net/ipv4/ip_forward"
        };
        std::fs::read_to_string(self.bins.proc_sys.join(file)).is_ok_and(|v| v.trim() == "1")
    }
    async fn local_addrs(&self, port: u16) -> Vec<SocketAddr> {
        match tokio::process::Command::new(&self.bins.ip)
            .args(["-o", "addr", "show", "scope", "global"])
            .output()
            .await
        {
            Ok(o) if o.status.success() => crate::net::parse_ip_addr(
                &String::from_utf8_lossy(&o.stdout),
                &self.interface,
                port,
            ),
            _ => Vec::new(),
        }
    }
}
