// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Decisiones de aviso con reloj explícito; la entrega no interviene en las pruebas.

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn status(path: &str, lease: bool, expiry: i64) -> Value {
        json!({"config":{"lease_active":lease},"peers":[{"public_key":"synthetic","name":"Par","path":path}],"server":{"issuer_cert_not_after_unix":expiry}})
    }
    #[test]
    fn first_sample_none_and_relay_debounce() {
        let mut state = State::default();
        assert!(
            state
                .sample("conectada", Some(&status("direct", true, 9_000_000)), 0)
                .is_empty()
        );
        assert!(
            state
                .sample("conectada", Some(&status("relay-udp", true, 9_000_000)), 10)
                .is_empty()
        );
        assert!(
            state
                .sample("conectada", Some(&status("relay-tls", true, 9_000_000)), 69)
                .is_empty()
        );
        assert_eq!(
            state
                .sample("conectada", Some(&status("relay-tls", true, 9_000_000)), 70)
                .len(),
            1
        );
        assert!(
            state
                .sample("conectada", Some(&status("relay-tls", true, 9_000_000)), 80)
                .is_empty()
        );
        assert!(
            state
                .sample("conectada", Some(&status("none", true, 9_000_000)), 90)
                .is_empty()
        );
        assert!(
            state
                .sample(
                    "conectada",
                    Some(&status("relay-udp", true, 9_000_000)),
                    200
                )
                .is_empty()
        );
    }
    #[test]
    fn connection_loss_lease_conflict_and_recovery() {
        for (network_state, status) in [
            ("conflicto", Some(status("direct", true, 9_000_000))),
            ("conectada", None),
            ("conectada", Some(status("direct", false, 9_000_000))),
        ] {
            let mut state = State::default();
            state.sample(
                "conectada",
                Some(&self::status("direct", true, 9_000_000)),
                0,
            );
            assert_eq!(state.sample(network_state, status.as_ref(), 1).len(), 1);
            assert!(state.sample(network_state, status.as_ref(), 2).is_empty());
            assert_eq!(
                state
                    .sample(
                        "conectada",
                        Some(&self::status("direct", true, 9_000_000)),
                        3
                    )
                    .len(),
                1
            );
        }
    }
    #[test]
    fn certificate_rate_and_missing_fields() {
        let mut state = State::default();
        let value = status("none", true, 100);
        assert!(state.sample("conectada", Some(&value), 0).is_empty());
        assert_eq!(state.sample("conectada", Some(&value), 1).len(), 1);
        assert!(state.sample("conectada", Some(&value), 86400).is_empty());
        assert_eq!(state.sample("conectada", Some(&value), 86401).len(), 1);
        let mut state = State::default();
        state.sample("detenida", None, 0);
        assert!(state.sample("detenida", None, 1).is_empty());
    }
    #[test]
    fn sanitizes_interpreter_text_and_bounds_length() {
        let text = sanitize("a\"b'c\\d\ne\u{202e}f");
        assert_eq!(text, "abcdef");
        assert_eq!(sanitize(&"ñ".repeat(300)).chars().count(), 180);
    }
    #[test]
    fn preference_defaults_and_round_trips() {
        let path = std::env::temp_dir().join(format!("atenea-notify-test-{}", std::process::id()));
        assert!(get(&path).unwrap());
        set(&path, false).unwrap();
        assert!(!get(&path).unwrap());
        std::fs::remove_file(path).unwrap();
    }
}

#[derive(Default)]
struct Peer {
    direct: bool,
    relay_since: Option<i64>,
    notified: bool,
}

#[derive(Default)]
pub struct State {
    initialized: bool,
    connected: bool,
    lost: bool,
    peers: HashMap<String, Peer>,
    certificate_at: Option<i64>,
}

impl State {
    pub fn sample(&mut self, network_state: &str, status: Option<&Value>, now: i64) -> Vec<String> {
        let connected = network_state != "conflicto"
            && status.is_some_and(|s| s["config"]["lease_active"].as_bool() == Some(true));
        let mut messages = Vec::new();
        if self.initialized {
            if self.connected && !connected && network_state != "detenida" {
                self.lost = true;
                messages.push("La red ha perdido la conexión".into());
            } else if connected && self.lost {
                self.lost = false;
                messages.push("La red vuelve a estar conectada".into());
            }
        }
        self.connected = connected;
        let peers = status.and_then(|s| s["peers"].as_array());
        self.peers.retain(|key, _| {
            peers.is_some_and(|p| p.iter().any(|p| p["public_key"].as_str() == Some(key)))
        });
        if let Some(peers) = peers {
            for peer in peers {
                let Some(key) = peer["public_key"].as_str() else {
                    continue;
                };
                let previous = self.peers.entry(key.into()).or_default();
                match peer["path"].as_str().unwrap_or("none") {
                    "direct" => {
                        previous.direct = true;
                        previous.relay_since = None;
                        previous.notified = false;
                    }
                    path if path.starts_with("relay-") && previous.direct => {
                        let since = *previous.relay_since.get_or_insert(now);
                        if self.initialized && now.saturating_sub(since) >= 60 && !previous.notified
                        {
                            previous.notified = true;
                            messages.push(format!(
                                "{} lleva un minuto por relay",
                                peer["name"].as_str().unwrap_or("Un dispositivo")
                            ));
                        }
                    }
                    _ => {
                        previous.direct = false;
                        previous.relay_since = None;
                        previous.notified = false;
                    }
                }
            }
        }
        if self.initialized
            && let Some(expiry) =
                status.and_then(|s| s["server"]["issuer_cert_not_after_unix"].as_i64())
            && expiry.saturating_sub(now) <= 14 * 86400
            && self
                .certificate_at
                .is_none_or(|last| now.saturating_sub(last) >= 86400)
        {
            self.certificate_at = Some(now);
            messages.push("El certificado de la red caduca en 14 días o menos".into());
        }
        self.initialized = true;
        messages
    }
}

pub fn get(path: &Path) -> Result<bool, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())?["notifications"]
                .as_bool()
                .ok_or_else(|| "preferencia de notificaciones no válida".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set(path: &Path, on: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        serde_json::json!({"notifications": on}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

pub fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() && !matches!(c, '\'' | '"' | '\\' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(180).collect()
}

pub fn deliver(text: &str) {
    let text = sanitize(text);
    let mut command = if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("/usr/bin/osascript");
        command.args([
            "-e",
            &format!("display notification \"{text}\" with title \"Atenea One\""),
        ]);
        command
    } else if cfg!(target_os = "linux") {
        let mut command = std::process::Command::new("notify-send");
        command.args(["--", "Atenea One", &text]);
        command
    } else {
        return;
    };
    let _ = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
