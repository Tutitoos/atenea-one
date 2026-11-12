// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Órdenes Linux existentes, separadas de la política de rutas y estados.
use super::HostNetwork;
#[cfg(all(test, unix))]
use crate::testutil::{output, spawn};
use atenea_identity::{AdvertisedRoute, DnsConfig, Prefix};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(not(test))]
fn output(command: &mut Command) -> std::io::Result<std::process::Output> {
    command.output()
}

#[cfg(not(test))]
fn spawn(command: &mut Command) -> std::io::Result<std::process::Child> {
    command.spawn()
}
#[derive(Clone)]
pub struct Binaries {
    pub ip: PathBuf,
    pub nft: PathBuf,
    pub resolvectl: PathBuf,
    pub proc_sys: PathBuf,
}
impl Default for Binaries {
    /// Rutas fijas; el administrador puede sustituirlas con `ATENEA_IP`, `ATENEA_NFT` y
    /// `ATENEA_RESOLVECTL` (absolutas; `command` rechaza las que no son de root o del proceso).
    fn default() -> Self {
        let bin = |var: &str, fixed: &str| {
            std::env::var_os(var).map_or_else(|| fixed.into(), PathBuf::from)
        };
        Self {
            ip: bin("ATENEA_IP", "/usr/sbin/ip"),
            nft: bin("ATENEA_NFT", "/usr/sbin/nft"),
            resolvectl: bin("ATENEA_RESOLVECTL", "/usr/bin/resolvectl"),
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
        let out = output(command(&self.bins.ip)?.args(args)).map_err(|e| e.to_string())?;
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
    /// `disable_ipv6 = 1` en la interfaz (p. ej. `net.ipv6.conf.default.disable_ipv6=1` al crearla) o un
    /// núcleo sin IPv6, donde `/proc/sys/net/ipv6` no existe.
    fn ipv6_disabled(&self) -> bool {
        let conf = self.bins.proc_sys.join("net/ipv6/conf");
        match std::fs::read_to_string(conf.join(&self.interface).join("disable_ipv6")) {
            Ok(v) => v.trim() == "1",
            Err(_) => !conf.exists(),
        }
    }
}
/// Rutas no predeterminadas de `ip -j route show table all` fuera de la interfaz propia.
pub(crate) fn parse_routes(bytes: &[u8], interface: &str) -> Result<Vec<Prefix>, String> {
    parse_routes_by(bytes, |dev| dev != Some(interface))
}
/// Nombres de enlaces de tipo túnel (`tun`, `wireguard`) de `ip -j -d link show`.
pub(crate) fn parse_tunnel_links(bytes: &[u8]) -> Result<Vec<String>, String> {
    let links: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "respuesta ip inválida")?;
    Ok(links
        .as_array()
        .ok_or("respuesta ip inválida")?
        .iter()
        .filter(|l| {
            matches!(
                l["linkinfo"]["info_kind"].as_str(),
                Some("tun" | "wireguard")
            )
        })
        .filter_map(|l| l["ifname"].as_str().map(str::to_owned))
        .collect())
}
fn parse_routes_by(
    bytes: &[u8],
    keep: impl Fn(Option<&str>) -> bool,
) -> Result<Vec<Prefix>, String> {
    let entries: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "respuesta ip inválida")?;
    let mut routes = Vec::new();
    for entry in entries.as_array().ok_or("respuesta ip inválida")? {
        let dst = entry["dst"].as_str().ok_or("ruta sin destino")?;
        if dst == "default" || !keep(entry["dev"].as_str()) {
            continue;
        }
        let prefix = parse_prefix(dst).ok_or("prefijo del host inválido")?;
        if prefix.len != 0 {
            routes.push(prefix);
        }
    }
    Ok(routes)
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
/// Ejecutable absoluto, regular, de root o del propio proceso, sin escritura de grupo/otros ni en su
/// directorio; entorno vacío salvo un `PATH` fijo para los scripts auxiliares de las pruebas.
fn command(bin: &Path) -> Result<Command, String> {
    use std::os::unix::fs::MetadataExt;
    if !bin.is_absolute() {
        return Err("binario de red: se requiere ruta absoluta".into());
    }
    #[cfg(target_os = "linux")]
    let me = std::fs::metadata("/proc/self")
        .map_err(|_| "no se pudo identificar el proceso")?
        .uid();
    // El backend Linux también se prueba con binarios simulados en hosts Unix sin /proc.
    #[cfg(all(test, not(target_os = "linux")))]
    let me = std::fs::metadata(std::env::temp_dir())
        .map_err(|_| "no se pudo identificar el usuario de pruebas")?
        .uid();
    let trusted = |meta: &std::fs::Metadata| {
        (meta.uid() == 0 || meta.uid() == me) && meta.mode() & 0o022 == 0
    };
    let file = std::fs::metadata(bin).map_err(|_| "binario de red no disponible")?;
    let dir = bin
        .parent()
        .and_then(|d| std::fs::metadata(d).ok())
        .ok_or("binario de red no disponible")?;
    if !file.is_file() || !trusted(&file) || !dir.is_dir() || !trusted(&dir) {
        return Err("binario de red con propietario o permisos inseguros".into());
    }
    let mut cmd = Command::new(bin);
    cmd.env_clear().env("PATH", "/usr/bin:/bin");
    Ok(cmd)
}
fn run(bin: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = output(command(bin)?.args(args)).map_err(|_| "binario no disponible".to_string())?;
    if !out.status.success() {
        return Err("orden de red fallida".into());
    }
    Ok(out.stdout)
}
/// Sockets en escucha (estado `0A`) de `/proc/net/tcp` o `/proc/net/tcp6`. Las direcciones son
/// palabras de 32 bits en orden del host (little-endian en las plataformas Linux soportadas);
/// una línea ilegible se descarta sin afectar al resto.
pub(crate) fn parse_proc_net_tcp(text: &str, v6: bool) -> Vec<SocketAddr> {
    fn word(hex: &str) -> Option<[u8; 4]> {
        u32::from_str_radix(hex, 16).ok().map(u32::to_le_bytes)
    }
    text.lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let (_, local, _, state) = (f.next()?, f.next()?, f.next()?, f.next()?);
            if state != "0A" {
                return None;
            }
            let (host, port) = local.split_once(':')?;
            let port = u16::from_str_radix(port, 16).ok()?;
            let ip = match (v6, host.len()) {
                (false, 8) => IpAddr::from(word(host)?),
                (true, 32) => {
                    let mut b = [0u8; 16];
                    for (i, chunk) in b.chunks_mut(4).enumerate() {
                        chunk.copy_from_slice(&word(host.get(i * 8..i * 8 + 8)?)?);
                    }
                    IpAddr::from(b)
                }
                _ => return None,
            };
            Some(SocketAddr::new(ip, port))
        })
        .collect()
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
            match self.configure_ip(&args) {
                // Un CT o equipo con IPv6 desactivado sigue sirviendo por IPv4; cualquier otro fallo es real.
                Err(e) if addr.is_ipv6() && self.ipv6_disabled() => {
                    tracing::warn!(
                        "IPv6 desactivado en {}; la red funciona solo por IPv4 ({e})",
                        self.interface
                    );
                }
                result => result?,
            }
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
            routes.extend(parse_routes(&bytes, &self.interface)?);
        }
        Ok(routes)
    }
    fn tunnel_routes(&self) -> Result<Vec<Prefix>, String> {
        let links = parse_tunnel_links(&run(&self.bins.ip, &["-j", "-d", "link", "show"])?)?;
        let mut routes = Vec::new();
        for family in ["-4", "-6"] {
            let bytes = run(
                &self.bins.ip,
                &["-j", family, "route", "show", "table", "all"],
            )?;
            routes.extend(parse_routes_by(&bytes, |dev| {
                dev.is_some_and(|d| d != self.interface && links.iter().any(|l| l == d))
            })?);
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
        let mut child = spawn(
            command(&self.bins.nft)?
                .args(["-f", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
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
        let cmd = match command(&self.bins.ip) {
            Ok(cmd) => cmd,
            Err(_) => return Vec::new(),
        };
        match tokio::process::Command::from(cmd)
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
    async fn listening_tcp(&self) -> Result<Vec<SocketAddr>, String> {
        let v4 = tokio::fs::read_to_string("/proc/net/tcp")
            .await
            .map_err(|e| format!("/proc/net/tcp: {e}"))?;
        let mut out = parse_proc_net_tcp(&v4, false);
        // Sin IPv6 en el núcleo el fichero no existe: no es un error.
        if let Ok(v6) = tokio::fs::read_to_string("/proc/net/tcp6").await {
            out.extend(parse_proc_net_tcp(&v6, true));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP4: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11111 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 22222 1 0000000000000000 100 0 0 10 0\n   2: 0200630A:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 33333 1 0000000000000000 100 0 0 10 0\n   3: 0700000A:C350 0500000A:01BB 01 00000000:00000000 02:000A7B3C 00000000  1000        0 44444 2 0000000000000000 20 4 30 10 -1\n   4: zzzzzzzz:0016 00000000:0000 0A\n   5: 0100007F\n";
    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000000000000:0050 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 55555 1 0000000000000000 100 0 0 10 0\n   1: 00000000000000000000000001000000:1538 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000   999        0 66666 1 0000000000000000 100 0 0 10 0\n   2: 0000000000000000FFFF00000100007F:2382 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 77777 1 0000000000000000 100 0 0 10 0\n   3: 000000FD000000000000000002000000:2328 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 88888 1 0000000000000000 100 0 0 10 0\n   4: 000000FD0000000000000000020000:2328 00000000000000000000000000000000:0000 0A\n";
    #[test]
    fn proc_net_tcp_listening_sockets() {
        let s = |t: &str| t.parse::<SocketAddr>().unwrap();
        // Cabecera, ESTABLISHED (01) y líneas truncadas o con hexadecimal inválido se descartan.
        assert_eq!(
            parse_proc_net_tcp(TCP4, false),
            [s("0.0.0.0:22"), s("127.0.0.1:8080"), s("10.99.0.2:80")]
        );
        assert_eq!(
            parse_proc_net_tcp(TCP6, true),
            [
                s("[::]:80"),
                s("[::1]:5432"),
                s("[::ffff:127.0.0.1]:9090"),
                s("[fd00::2]:9000")
            ]
        );
        // Un fichero IPv6 leído como IPv4 (y al revés) no produce direcciones.
        assert!(parse_proc_net_tcp(TCP6, false).is_empty());
        assert!(parse_proc_net_tcp(TCP4, true).is_empty());
        assert!(parse_proc_net_tcp("", false).is_empty());
    }
    #[test]
    fn tunnel_routes_keep_only_other_tunnel_links() {
        let links = br#"[{"ifname":"lo"},{"ifname":"eth0","linkinfo":{"info_kind":"veth"}},{"ifname":"atenea0","linkinfo":{"info_kind":"tun"}},{"ifname":"atenea1","linkinfo":{"info_kind":"tun"}},{"ifname":"wg0","linkinfo":{"info_kind":"wireguard"}}]"#;
        assert_eq!(
            parse_tunnel_links(links).unwrap(),
            ["atenea0", "atenea1", "wg0"]
        );
        let routes = br#"[{"dst":"default","dev":"eth0"},{"dst":"192.0.2.0/24","dev":"eth0"},{"dst":"10.99.1.0/24","dev":"atenea1"},{"dst":"10.77.0.0/24","dev":"atenea0"},{"dst":"10.9.0.0/24","dev":"wg0"}]"#;
        let tunnels = ["atenea0", "atenea1", "wg0"];
        let found = parse_routes_by(routes, |dev| {
            dev.is_some_and(|d| d != "atenea0" && tunnels.contains(&d))
        })
        .unwrap();
        let found: Vec<_> = found
            .iter()
            .map(|p| format!("{}/{}", p.addr, p.len))
            .collect();
        assert_eq!(found, ["10.99.1.0/24", "10.9.0.0/24"]);
    }
}
