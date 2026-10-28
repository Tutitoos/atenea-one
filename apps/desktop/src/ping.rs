// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Ping acotado a las direcciones de túnel publicadas por los pares.

use serde_json::{Value, json};
use std::net::IpAddr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn validate(address: &str, status: &Value) -> Result<IpAddr, String> {
    let ip: IpAddr = address.parse().map_err(|_| "dirección IP no válida")?;
    let found = status["peers"].as_array().is_some_and(|peers| {
        peers.iter().any(|peer| {
            peer["addrs"].as_array().is_some_and(|addrs| {
                addrs
                    .iter()
                    .any(|a| a.as_str().and_then(|a| a.parse::<IpAddr>().ok()) == Some(ip))
            })
        })
    });
    if found {
        Ok(ip)
    } else {
        Err("la dirección no pertenece a un par de esta red".into())
    }
}

pub fn parse(text: &str) -> Result<Value, String> {
    let bad = || "respuesta de ping no válida".to_string();
    let summary = text
        .lines()
        .find(|l| l.contains("packets transmitted,"))
        .ok_or_else(bad)?;
    let fields: Vec<&str> = summary.split(',').collect();
    let count = |field: &str| {
        field
            .split_whitespace()
            .next()
            .ok_or_else(bad)?
            .parse::<u32>()
            .map_err(|_| bad())
    };
    let sent = count(fields[0])?;
    let received = count(fields.get(1).ok_or_else(bad)?)?;
    let loss = fields
        .iter()
        .find(|field| field.contains("packet loss"))
        .and_then(|f| f.split('%').next())
        .ok_or_else(bad)?
        .trim()
        .parse::<f64>()
        .map_err(|_| bad())?;
    if sent != 5
        || received > sent
        || !loss.is_finite()
        || !(0.0..=100.0).contains(&loss)
        || (loss - (100.0 * f64::from(sent - received) / f64::from(sent))).abs() > 0.1
    {
        return Err(bad());
    }
    let mut times = None;
    if received > 0 {
        let line = text
            .lines()
            .find(|l| l.starts_with("round-trip ") || l.starts_with("rtt "))
            .ok_or_else(bad)?;
        let values = line
            .split_once('=')
            .ok_or_else(bad)?
            .1
            .split_whitespace()
            .next()
            .ok_or_else(bad)?;
        let values: Vec<f64> = values
            .split('/')
            .map(str::parse::<f64>)
            .collect::<Result<_, _>>()
            .map_err(|_| bad())?;
        if values.len() < 3
            || values.iter().any(|v| !v.is_finite() || *v < 0.0)
            || values[0] > values[1]
            || values[1] > values[2]
        {
            return Err(bad());
        }
        times = Some(values);
    }
    Ok(json!({"sent":sent,"received":received,"loss_pct":loss,
        "min_ms":times.as_ref().map(|t| t[0]),"avg_ms":times.as_ref().map(|t| t[1]),"max_ms":times.as_ref().map(|t| t[2])}))
}

pub fn run(ip: IpAddr) -> Result<String, String> {
    if cfg!(windows) {
        return Err("no soportado en Windows".into());
    }
    let program = if cfg!(target_os = "macos") {
        if ip.is_ipv6() {
            "/sbin/ping6"
        } else {
            "/sbin/ping"
        }
    } else {
        "ping"
    };
    let mut command = Command::new(program);
    if !cfg!(target_os = "macos") && ip.is_ipv6() {
        command.arg("-6");
    }
    command.args(["-n", "-c", "5", "-i", "0.2"]);
    if cfg!(target_os = "macos") {
        command.args(["-W", "1000"]);
    } else {
        command.args(["-w", "8", "-W", "1"]);
    }
    let mut child = command
        .arg(ip.to_string())
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("no se pudo ejecutar ping: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(9) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => e.to_string(),
                    _ => "ping agotó el plazo de 9 s".into(),
                });
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    parse(&String::from_utf8_lossy(&out.stdout)).map(|v| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summaries_macos_linux_ipv6_and_loss() {
        for summary in [
            "5 packets transmitted, 5 packets received, 0.0% packet loss\nround-trip min/avg/max/stddev = 0.1/0.2/0.3/0.1 ms",
            "5 packets transmitted, 5 received, 0% packet loss, time 801ms\nrtt min/avg/max/mdev = 0.1/0.2/0.3/0.1 ms",
            "PING6(56=40+8+8 bytes) fd00::1 --> fd00::2\n8 bytes from fd00::2, icmp_seq=0 hlim=64 time=0.2 ms\n5 packets transmitted, 5 packets received, 0.0% packet loss\nround-trip min/avg/max/std-dev = 0.1/0.2/0.3/0.1 ms",
        ] {
            let result = parse(summary).unwrap();
            assert_eq!(result["received"], 5);
            assert_eq!(result["avg_ms"], 0.2);
        }
        let partial = parse(
            "5 packets transmitted, 3 received, 40% packet loss\nrtt min/avg/max/mdev = 1/2/3/0 ms",
        )
        .unwrap();
        assert_eq!(partial["loss_pct"], 40.0);
        let lost = parse("5 packets transmitted, 0 packets received, 100.0% packet loss").unwrap();
        assert!(lost["avg_ms"].is_null());
        assert!(parse("5 packets transmitted, 0 received, +5 errors, 100% packet loss").unwrap()["min_ms"].is_null());
        for corrupt in [
            "",
            "PING ::1",
            "5 packets transmitted, six received, 0% packet loss",
            "5 packets transmitted, 6 received, 0% packet loss",
            "5 packets transmitted, 1 received, 80% packet loss",
        ] {
            assert!(parse(corrupt).is_err(), "{corrupt}");
        }
    }

    #[test]
    fn validates_only_real_peer_addresses() {
        let status = json!({"peers":[{"addrs":["10.50.0.2", "fd00::2"]}], "addrs":["10.50.0.1"]});
        assert_eq!(validate("fd00::2", &status).unwrap().to_string(), "fd00::2");
        assert!(validate("10.50.0.2", &status).is_ok());
        for address in [
            "10.50.0.1",
            "10.60.0.2",
            "example.test",
            "10.50.0.2;id",
            "-c5",
            "fd00::2%en0",
        ] {
            assert!(validate(address, &status).is_err());
        }
    }
}
