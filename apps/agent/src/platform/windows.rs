// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Cliente Windows: NetTCPIP y NRPT por dominio, sin DNS global ni rutas predeterminadas.
use super::HostNetwork;
#[cfg(all(test, unix))]
use crate::testutil::output;
use atenea_identity::{AdvertisedRoute, DnsConfig, Prefix};
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
pub struct Binaries {
    pub powershell: PathBuf,
    pub pnputil: PathBuf,
}
impl Default for Binaries {
    fn default() -> Self {
        Self {
            powershell: PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            pnputil: PathBuf::from(r"C:\Windows\System32\pnputil.exe"),
        }
    }
}
pub struct Network {
    interface: String,
    bins: Binaries,
}
const PRELUDE: &str = "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false); ";
#[cfg(target_os = "windows")]
const COMMANDS: &str = "Get-NetIPInterface,Set-NetIPInterface,Get-NetRoute,New-NetRoute,Remove-NetRoute,Get-NetIPAddress,New-NetIPAddress,Remove-NetIPAddress,Get-DnsClientNrptRule,Add-DnsClientNrptRule,Remove-DnsClientNrptRule";
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn command(bin: &Path, script: &str) -> Result<Command, String> {
    if !bin.is_absolute() {
        return Err("PowerShell requiere una ruta absoluta".into());
    }
    let mut cmd = Command::new(bin);
    // Windows PowerShell no debe cargar módulos heredados de pwsh (PowerShell 7).
    cmd.env_remove("PSModulePath")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-OutputFormat",
            "Text",
            "-Command",
        ])
        .arg(format!("{PRELUDE}{script}"));
    Ok(cmd)
}
#[cfg(not(all(test, unix)))]
fn output(command: &mut Command) -> std::io::Result<std::process::Output> {
    command.output()
}
fn run(bin: &Path, script: &str) -> Result<Vec<u8>, String> {
    let out = output(&mut command(bin, script)?).map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{}: {}",
            bin.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}
fn prefix(text: &str) -> Result<Prefix, String> {
    let (addr, len) = text.split_once('/').ok_or("ruta sin máscara")?;
    let addr: IpAddr = addr.parse().map_err(|_| "dirección de ruta inválida")?;
    let len: u8 = len.parse().map_err(|_| "máscara de ruta inválida")?;
    if len > if addr.is_ipv4() { 32 } else { 128 } {
        return Err("máscara de ruta inválida".into());
    }
    Ok(Prefix { addr, len })
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Route {
    interface_index: u32,
    destination_prefix: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Address {
    interface_index: u32,
    #[serde(rename = "IPAddress")]
    ip_address: IpAddr,
    address_state: String,
}
#[derive(Deserialize)]
struct Snapshot<T> {
    #[serde(rename = "InterfaceIndex")]
    interface_index: u32,
    #[serde(rename = "Items")]
    items: Vec<T>,
}
fn parse_routes(bytes: &[u8]) -> Result<Vec<Prefix>, String> {
    let snapshot: Snapshot<Route> =
        serde_json::from_slice(bytes).map_err(|e| format!("Get-NetRoute: {e}"))?;
    let mut out = Vec::new();
    for route in snapshot.items {
        if route.interface_index == snapshot.interface_index {
            continue;
        }
        let p = prefix(&route.destination_prefix)?;
        if p.len != 0 && !out.contains(&p) {
            out.push(p);
        }
    }
    Ok(out)
}
fn parse_addrs(bytes: &[u8], port: u16) -> Result<Vec<SocketAddr>, String> {
    let snapshot: Snapshot<Address> =
        serde_json::from_slice(bytes).map_err(|e| format!("Get-NetIPAddress: {e}"))?;
    let mut out = Vec::new();
    for entry in snapshot.items {
        if entry.interface_index != snapshot.interface_index && entry.address_state == "Preferred" {
            let addr = SocketAddr::new(entry.ip_address, port);
            if atenea_nat_traversal::candidate_ok(&addr) && !out.contains(&addr) {
                out.push(addr);
            }
        }
    }
    Ok(out)
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
#[derive(Deserialize)]
struct Orphans {
    #[serde(rename = "Items")]
    items: Vec<String>,
}
fn parse_orphans(bytes: &[u8]) -> Result<Vec<String>, String> {
    let orphans: Orphans =
        serde_json::from_slice(bytes).map_err(|e| format!("Get-PnpDevice: {e}"))?;
    if orphans.items.iter().any(|id| {
        !id.to_ascii_lowercase().starts_with(r"swd\wintun\") || id.len() <= r"SWD\Wintun\".len()
    }) {
        return Err("identificador de dispositivo Wintun inválido".into());
    }
    Ok(orphans.items)
}
impl Network {
    pub fn new(interface: String, bins: Binaries) -> Self {
        Self { interface, bins }
    }
    #[cfg(target_os = "windows")]
    pub fn ensure_available() -> Result<(), String> {
        run(
            &Binaries::default().powershell,
            &format!("Get-Command -Name {COMMANDS} -ErrorAction Stop | Out-Null"),
        )
        .map(|_| ())
    }
    fn interface_script(&self) -> String {
        format!(
            "$indices = @(Get-NetIPInterface | Where-Object {{ $_.InterfaceAlias -ceq {} }} | Select-Object -ExpandProperty InterfaceIndex -Unique); if ($indices.Count -ne 1) {{ throw 'Interfaz Windows ambigua o ausente' }}; $i = $indices[0]; ",
            quote(&self.interface)
        )
    }
    fn execute(&self, script: &str) -> Result<Vec<u8>, String> {
        run(
            &self.bins.powershell,
            &format!("{}{script}", self.interface_script()),
        )
    }
    fn owner(&self) -> String {
        format!("Atenea One: NRPT administrado ({})", self.interface)
    }
    fn dns_cleanup_script(&self) -> String {
        format!(
            "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -ceq {} }} | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force }}; ",
            quote(&self.owner())
        )
    }
    fn route_cleanup_script(&self) -> String {
        format!(
            "$indices = @(Get-NetIPInterface | Where-Object {{ $_.InterfaceAlias -ceq {} }} | Select-Object -ExpandProperty InterfaceIndex -Unique); if ($indices.Count -gt 1) {{ throw 'Interfaz Windows ambigua' }}; foreach ($i in $indices) {{ Get-NetRoute -PolicyStore ActiveStore | Where-Object {{ $_.InterfaceIndex -eq $i -and $_.Protocol -eq 'NetMgmt' -and $_.DestinationPrefix -ne '0.0.0.0/0' -and $_.DestinationPrefix -ne '::/0' }} | Remove-NetRoute -Confirm:$false }}",
            quote(&self.interface)
        )
    }
    fn orphan_script(&self) -> String {
        // La propiedad privada de Wintun conserva el nombre incluso sin interfaz IP.
        format!(
            "$ids = @(Get-PnpDevice -Class Net | Where-Object {{ $_.FriendlyName -ceq 'Atenea Tunnel' -and $_.InstanceId -like 'SWD\\Wintun\\*' }} | ForEach-Object {{ $id = $_.InstanceId; $hardware = (Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_HardwareIds').Data; $name = (Get-PnpDeviceProperty -InstanceId $id -KeyName '{{3361c968-2f2e-4660-b47e-699cdc4c32b9}} 3').Data; if ($hardware -contains 'Wintun' -and $name -ceq {}) {{ $id }} }}); @{{ Items = $ids }} | ConvertTo-Json -Depth 3 -Compress",
            quote(&self.interface)
        )
    }
    /// Solo para limpieza explícita sin agente activo; no se llama al arrancar.
    pub fn remove_orphan_adapter(&self) -> Result<(), String> {
        if !self.bins.pnputil.is_absolute() {
            return Err("pnputil requiere una ruta absoluta".into());
        }
        for id in parse_orphans(&run(&self.bins.powershell, &self.orphan_script())?)? {
            let out = output(Command::new(&self.bins.pnputil).args(["/remove-device", &id]))
                .map_err(|e| format!("{}: {e}", self.bins.pnputil.display()))?;
            if !out.status.success() {
                return Err(format!(
                    "pnputil {}: {} {}",
                    out.status,
                    String::from_utf8_lossy(&out.stdout).trim(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        Ok(())
    }
    fn route(&self, text: &str, add: bool) -> Result<(), String> {
        let p = prefix(text)?;
        if p.len == 0 {
            return Err("Windows cliente no admite rutas predeterminadas".into());
        }
        let destination = quote(&format!("{}/{}", p.addr, p.len));
        let query = format!(
            "@(Get-NetRoute -InterfaceIndex $i -PolicyStore ActiveStore | Where-Object {{ $_.DestinationPrefix -eq {destination} }})"
        );
        let script = if add {
            let gateway = if p.addr.is_ipv4() { "0.0.0.0" } else { "::" };
            format!(
                "if (({query}).Count -eq 0) {{ New-NetRoute -InterfaceIndex $i -DestinationPrefix {destination} -NextHop '{gateway}' -PolicyStore ActiveStore | Out-Null }}"
            )
        } else {
            format!("{query} | Remove-NetRoute -Confirm:$false")
        };
        self.execute(&script).map(|_| ())
    }
}
impl HostNetwork for Network {
    fn supports_subnet_routing(&self) -> bool {
        false
    }
    fn configure_link(&self, mtu: usize, addrs: &[(IpAddr, u8)]) -> Result<(), String> {
        if !(1280..=65535).contains(&mtu)
            || addrs
                .iter()
                .any(|(ip, len)| *len == 0 || *len > if ip.is_ipv4() { 32 } else { 128 })
        {
            return Err("MTU o prefijo de interfaz inválido".into());
        }
        let mut script = format!(
            "foreach ($family in @('IPv4','IPv6')) {{ Set-NetIPInterface -InterfaceIndex $i -AddressFamily $family -NlMtuBytes {mtu} -Dhcp Disabled -Forwarding Disabled }}; Set-NetIPInterface -InterfaceIndex $i -AddressFamily IPv6 -RouterDiscovery Disabled; Get-NetIPAddress -InterfaceIndex $i | Where-Object {{ -not ([System.Net.IPAddress]::Parse($_.IPAddress)).IsIPv6LinkLocal }} | Remove-NetIPAddress -Confirm:$false; "
        );
        for (addr, len) in addrs {
            script.push_str(&format!("New-NetIPAddress -InterfaceIndex $i -IPAddress '{addr}' -PrefixLength {len} -PolicyStore ActiveStore | Out-Null; "));
        }
        self.execute(&script).map(|_| ())
    }
    fn existing_routes(&self) -> Result<Vec<Prefix>, String> {
        parse_routes(&self.execute("$items = @(Get-NetRoute -PolicyStore ActiveStore | Select-Object InterfaceIndex,DestinationPrefix); @{ InterfaceIndex = $i; Items = $items } | ConvertTo-Json -Depth 4 -Compress")?)
    }
    fn add_route(&self, p: &str) -> Result<(), String> {
        self.route(p, true)
    }
    fn remove_route(&self, p: &str) -> Result<(), String> {
        self.route(p, false)
    }
    fn set_dns(&self, dns: &DnsConfig) -> Result<(), String> {
        if dns.domains.iter().any(|d| !domain_ok(d))
            || (!dns.domains.is_empty() && dns.servers.is_empty())
        {
            return Err("dominio DNS de enrutamiento o servidores inválidos".into());
        }
        let mut preflight = "$rules = @(Get-DnsClientNrptRule); ".to_string();
        for domain in &dns.domains {
            preflight.push_str(&format!("if (@($rules | Where-Object {{ $_.Comment -cne {} -and $_.Namespace -contains {} }}).Count -ne 0) {{ throw 'Regla NRPT ajena: no se sobrescribe' }}; ", quote(&self.owner()), quote(&format!(".{domain}"))));
        }
        let mut script = self.dns_cleanup_script();
        let servers = dns
            .servers
            .iter()
            .map(|s| quote(&s.to_string()))
            .collect::<Vec<_>>()
            .join(",");
        for (index, domain) in dns.domains.iter().enumerate() {
            if dns.domains[..index]
                .iter()
                .any(|d| d.eq_ignore_ascii_case(domain))
            {
                continue;
            }
            script.push_str(&format!("Add-DnsClientNrptRule -Namespace {} -NameServers @({servers}) -Comment {} | Out-Null; ", quote(&format!(".{domain}")), quote(&self.owner())));
        }
        // Una instalación parcial no debe dejar políticas NRPT activas.
        let script = format!(
            "{preflight}try {{ {script} }} catch {{ {} throw }}",
            self.dns_cleanup_script()
        );
        run(&self.bins.powershell, &script).map(|_| ())
    }
    fn clear_dns(&self) -> Result<(), String> {
        run(&self.bins.powershell, &self.dns_cleanup_script()).map(|_| ())
    }
    fn set_snat(&self, _: &[AdvertisedRoute], _: bool) -> Result<(), String> {
        Err("Windows no admite servir subredes ni SNAT".into())
    }
    fn cleanup(&self) -> Result<(), String> {
        let dns = self.clear_dns();
        let routes = run(&self.bins.powershell, &self.route_cleanup_script()).map(|_| ());
        dns.and(routes)
    }
    fn forwarding(&self, _: bool) -> bool {
        false
    }
    async fn local_addrs(&self, port: u16) -> Vec<SocketAddr> {
        let script = format!(
            "{}$items = @(Get-NetIPAddress | Select-Object InterfaceIndex,IPAddress,@{{Name='AddressState';Expression={{ [string]$_.AddressState }}}}); @{{ InterfaceIndex = $i; Items = $items }} | ConvertTo-Json -Depth 4 -Compress",
            self.interface_script()
        );
        let Ok(cmd) = command(&self.bins.powershell, &script) else {
            return Vec::new();
        };
        match tokio::process::Command::from(cmd).output().await {
            Ok(out) if out.status.success() => parse_addrs(&out.stdout, port).unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_json_routes_and_addresses() {
        let routes = br#"{"InterfaceIndex":9,"Items":[{"InterfaceIndex":3,"DestinationPrefix":"0.0.0.0/0"},{"InterfaceIndex":3,"DestinationPrefix":"::/0"},{"InterfaceIndex":9,"DestinationPrefix":"10.77.0.0/24"},{"InterfaceIndex":3,"DestinationPrefix":"192.0.2.0/24"},{"InterfaceIndex":3,"DestinationPrefix":"2001:db8::/64"}]}"#;
        let result = parse_routes(routes).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].addr.to_string(), "192.0.2.0");
        assert_eq!(result[1].len, 64);
        assert!(
            parse_routes(br#"{"InterfaceIndex":9,"Items":[]}"#)
                .unwrap()
                .is_empty()
        );
        assert!(
            parse_routes(
                br#"{"InterfaceIndex":9,"Items":[{"InterfaceIndex":3,"DestinationPrefix":"bad"}]}"#
            )
            .is_err()
        );
        assert!(parse_routes(b"bad").is_err());
        let addrs = br#"{"InterfaceIndex":9,"Items":[{"InterfaceIndex":9,"IPAddress":"10.77.0.2","AddressState":"Preferred"},{"InterfaceIndex":3,"IPAddress":"192.0.2.7","AddressState":"Preferred"},{"InterfaceIndex":3,"IPAddress":"192.0.2.8","AddressState":"Tentative"},{"InterfaceIndex":3,"IPAddress":"127.0.0.1","AddressState":"Preferred"},{"InterfaceIndex":3,"IPAddress":"fe80::1","AddressState":"Preferred"}]}"#;
        assert_eq!(
            parse_addrs(addrs, 51820).unwrap(),
            vec!["192.0.2.7:51820".parse().unwrap()]
        );
    }
    #[test]
    fn rejects_unsafe_inputs_before_invoking_powershell() {
        let host = Network::new(
            "Atenea'One".into(),
            Binaries {
                powershell: "missing-relative-binary".into(),
                ..Default::default()
            },
        );
        assert_eq!(quote("Atenea'One"), "'Atenea''One'");
        assert!(host.interface_script().contains("'Atenea''One'"));
        for domain in [
            "",
            ".",
            "..",
            ".example.org",
            "example.org.",
            "a/b",
            "a\nb",
            "a..b",
            "-bad.org",
            "bad_.org",
        ] {
            assert!(!domain_ok(domain));
            assert!(
                host.set_dns(&DnsConfig {
                    servers: vec!["10.77.0.1".parse().unwrap()],
                    domains: vec![domain.into()]
                })
                .is_err()
            );
        }
        assert!(domain_ok("lab.internal"));
        for route in ["0.0.0.0/0", "::/0", "10.0.0.1/33", "::1/129", "bad"] {
            assert!(host.add_route(route).is_err());
            assert!(host.remove_route(route).is_err());
        }
        assert!(host.configure_link(1279, &[]).is_err());
        assert!(
            host.configure_link(1280, &[("10.77.0.2".parse().unwrap(), 0)])
                .is_err()
        );
        assert!(
            host.configure_link(1280, &[("fd77::2".parse().unwrap(), 129)])
                .is_err()
        );
        assert!(host.set_snat(&[], false).is_err());
        assert!(!host.supports_subnet_routing());
        assert!(!host.forwarding(false));
        assert!(command(Path::new("powershell"), "").is_err());
        assert!(
            command(&std::env::current_exe().unwrap(), "")
                .unwrap()
                .get_envs()
                .any(|(key, value)| key == "PSModulePath" && value.is_none())
        );
        assert!(
            host.dns_cleanup_script()
                .contains("$_.Comment -ceq 'Atenea One: NRPT administrado (Atenea''One)'")
        );
    }
    #[test]
    fn orphan_ids_and_cleanup_are_narrowly_scoped() {
        assert_eq!(
            parse_orphans(br#"{"Items":["SWD\\Wintun\\{abc}"]}"#).unwrap(),
            vec![r"SWD\Wintun\{abc}"]
        );
        assert!(parse_orphans(br#"{"Items":[]}"#).unwrap().is_empty());
        assert!(parse_orphans(br#"{"Items":["ROOT\\OTHER\\123"]}"#).is_err());
        assert!(parse_orphans(br#"{"Items":["SWD\\Wintun\\"]}"#).is_err());
        let host = Network::new("Atenea'One".into(), Binaries::default());
        let script = host.orphan_script();
        assert!(script.contains("FriendlyName -ceq 'Atenea Tunnel'"));
        assert!(script.contains("$hardware -contains 'Wintun'"));
        assert!(script.contains("$name -ceq 'Atenea''One'"));
        assert!(script.contains("{3361c968-2f2e-4660-b47e-699cdc4c32b9} 3"));
        let cleanup = host.route_cleanup_script();
        assert!(cleanup.contains("InterfaceAlias -ceq 'Atenea''One'"));
        assert!(cleanup.contains("foreach ($i in $indices)"));
        assert!(cleanup.contains("-PolicyStore ActiveStore"));
        assert!(cleanup.contains("$_.InterfaceIndex -eq $i -and $_.Protocol -eq 'NetMgmt'"));
        assert!(cleanup.contains("DestinationPrefix -ne '0.0.0.0/0'"));
        assert!(cleanup.contains("DestinationPrefix -ne '::/0'"));
        assert!(!cleanup.contains("remove-device"));
    }
}
#[cfg(all(test, unix))]
mod command_tests {
    use super::*;
    #[test]
    fn injected_binary_receives_native_scoped_commands() {
        let dir = std::env::temp_dir().join(format!("atenea-windows-host-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let bin = dir.join("powershell");
        let log = dir.join("args");
        crate::testutil::fake_binary(
            &bin,
            &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", log.display()),
        );
        let host = Network::new(
            "Atenea One".into(),
            Binaries {
                powershell: bin,
                ..Default::default()
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
        host.set_dns(&DnsConfig {
            servers: vec!["10.77.0.1".parse().unwrap()],
            domains: vec!["lab.internal".into()],
        })
        .unwrap();
        host.cleanup().unwrap();
        let calls = std::fs::read_to_string(log).unwrap();
        for expected in [
            "-NoProfile",
            "-NonInteractive",
            "-OutputFormat\nText",
            "Set-NetIPInterface -InterfaceIndex $i",
            "-NlMtuBytes 1280",
            "-RouterDiscovery Disabled",
            "New-NetIPAddress -InterfaceIndex $i -IPAddress '10.77.0.2' -PrefixLength 24",
            "New-NetIPAddress -InterfaceIndex $i -IPAddress 'fd77::2' -PrefixLength 64",
            "New-NetRoute -InterfaceIndex $i -DestinationPrefix '10.88.0.0/24' -NextHop '0.0.0.0' -PolicyStore ActiveStore",
            "DestinationPrefix -eq 'fd88::/64'",
            "Add-DnsClientNrptRule -Namespace '.lab.internal' -NameServers @('10.77.0.1')",
            "$_.Comment -ceq 'Atenea One: NRPT administrado (Atenea One)'",
        ] {
            assert!(calls.contains(expected), "falta {expected}");
        }
        assert!(!calls.contains("Set-DnsClientServerAddress"));
        assert!(!calls.contains("-DefaultGateway"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn orphan_removal_targets_one_instance_with_injected_pnputil() {
        let dir =
            std::env::temp_dir().join(format!("atenea-windows-orphan-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let powershell = dir.join("powershell");
        let pnputil = dir.join("pnputil");
        let log = dir.join("args");
        crate::testutil::fake_binary(
            &powershell,
            "#!/bin/sh\nprintf '%s\\n' '{\"Items\":[\"SWD\\\\Wintun\\\\{abc}\"]}'\n",
        );
        crate::testutil::fake_binary(
            &pnputil,
            &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", log.display()),
        );
        let host = Network::new(
            "Atenea One".into(),
            Binaries {
                powershell,
                pnputil,
            },
        );
        host.remove_orphan_adapter().unwrap();
        assert_eq!(
            std::fs::read_to_string(log).unwrap(),
            "/remove-device\nSWD\\Wintun\\{abc}\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
