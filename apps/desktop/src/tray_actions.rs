// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Acciones de bandeja: etiquetas puras y portapapeles sin plugins.

use serde_json::Value;
use std::io::Write;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};

pub fn entries(net: &Value, status: Option<&Value>, clipboard: bool) -> Vec<(String, String)> {
    let id = net["network_id"].as_str().unwrap_or("");
    let name = net["name"].as_str().unwrap_or("Red");
    let up = matches!(
        net["state"].as_str(),
        Some("detenida" | "desconocida") | None
    );
    let mut entries = vec![(
        format!("{}:{id}", if up { "up" } else { "down" }),
        format!("{} {name}", if up { "Conectar" } else { "Desconectar" }),
    )];
    if clipboard
        && let Some(ip) = status
            .and_then(|s| s["addrs"].as_array())
            .and_then(|addrs| {
                addrs
                    .iter()
                    .filter_map(|a| a.as_str()?.parse::<Ipv4Addr>().ok())
                    .next()
            })
    {
        entries.push((format!("copy:{ip}"), format!("Copiar mi IP · {name}")));
    }
    entries
}

pub fn clipboard_program() -> Option<std::path::PathBuf> {
    let names: &[&str] = if cfg!(target_os = "macos") {
        &["pbcopy"]
    } else if cfg!(target_os = "linux") {
        &["wl-copy", "xclip"]
    } else {
        &[]
    };
    names.iter().find_map(|name| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

pub fn copy(address: &str) -> Result<(), String> {
    let ip: Ipv4Addr = address.parse().map_err(|_| "IP no válida")?;
    let program = clipboard_program().ok_or("portapapeles no disponible")?;
    let mut command = Command::new(&program);
    if program.file_name().is_some_and(|name| name == "xclip") {
        command.args(["-selection", "clipboard"]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("portapapeles sin entrada")?
        .write_all(ip.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    if child.wait().map_err(|e| e.to_string())?.success() {
        Ok(())
    } else {
        Err("no se pudo copiar la IP".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn labels_follow_network_state_and_ipv4() {
        let net = |state| json!({"network_id":"id", "name":"Red de prueba", "state":state});
        let status = json!({"addrs":["fd00::1", "10.50.0.1"]});
        assert_eq!(
            entries(&net("detenida"), Some(&status), true),
            vec![
                ("up:id".into(), "Conectar Red de prueba".into()),
                (
                    "copy:10.50.0.1".into(),
                    "Copiar mi IP · Red de prueba".into()
                )
            ]
        );
        for state in ["conectada", "conflicto", "iniciada_sin_configuracion"] {
            assert_eq!(
                entries(&net(state), None, false),
                vec![("down:id".into(), "Desconectar Red de prueba".into())]
            );
        }
        assert_eq!(entries(&net("conectada"), Some(&status), false).len(), 1);
    }
}
