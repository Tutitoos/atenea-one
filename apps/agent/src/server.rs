// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Información de presentación del servidor de esta red, sin efectos sobre autorización ni rutas.

use atenea_identity::{DeviceId, SigningKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Service {
    pub version: String,
    pub commit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Info {
    pub control_plane: Service,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<Service>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<Service>,
    pub server_time_unix: i64,
    pub issuer_cert_not_after_unix: Option<i64>,
}

#[derive(Clone, Serialize)]
pub struct Status {
    #[serde(flatten)]
    pub info: Info,
    pub clock_skew_secs: i64,
    pub fetched_unix: u64,
    pub last_error: Option<&'static str>,
}

pub fn fetch(
    cfg: &crate::config::SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
) -> Result<Option<Info>, &'static str> {
    crate::sync::get_metadata(cfg, key, device, ts, "/api/v1/server/info")?
        .map(|body| serde_json::from_slice(&body).map_err(|_| "información del servidor inválida"))
        .transpose()
}

pub fn record(status: &mut Option<Status>, result: Result<Option<Info>, &'static str>, unix: u64) {
    match result {
        Ok(Some(info)) => {
            // Estimación al recibir; no compensa el RTT de la petición.
            let local = i64::try_from(unix).unwrap_or(i64::MAX);
            *status = Some(Status {
                clock_skew_secs: info.server_time_unix.saturating_sub(local),
                info,
                fetched_unix: unix,
                last_error: None,
            });
        }
        Ok(None) => *status = None,
        Err(error) => {
            if let Some(status) = status {
                status.last_error = Some(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> Info {
        serde_json::from_value(serde_json::json!({
            "control_plane": {"version": "0.1.6", "commit": "abcd1234"},
            "server_time_unix": 120,
            "issuer_cert_not_after_unix": 500
        }))
        .unwrap()
    }

    #[test]
    fn server_status_preserves_last_good_read_on_error_and_clears_on_404() {
        let mut status = None;
        record(&mut status, Ok(Some(info())), 125);
        let first = status.clone().unwrap();
        assert_eq!(first.clock_skew_secs, -5);
        assert_eq!(first.fetched_unix, 125);
        assert_eq!(first.info.issuer_cert_not_after_unix, Some(500));
        record(&mut status, Err("error de red"), 130);
        let failed = status.as_ref().unwrap();
        assert_eq!(failed.info.control_plane.commit, "abcd1234");
        assert_eq!(failed.fetched_unix, 125);
        assert_eq!(failed.last_error, Some("error de red"));
        record(&mut status, Ok(Some(info())), 115);
        assert_eq!(status.as_ref().unwrap().clock_skew_secs, 5);
        assert!(status.as_ref().unwrap().last_error.is_none());
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["control_plane"]["version"], "0.1.6");
        assert!(json.get("coordinator").is_none());
        assert!(json.get("relay").is_none());
        record(&mut status, Ok(None), 140);
        assert!(status.is_none());
        record(&mut status, Err("error de red"), 150);
        assert!(
            status.is_none(),
            "sin lectura buena no se inventan versiones"
        );
    }

    #[test]
    #[cfg(unix)]
    fn server_http_is_signed_bounded_and_backward_compatible() {
        use atenea_identity::api_auth::{parse_authorization, verify_request};
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;
        let key = atenea_identity::signing_key([1; 32]);
        let public = key.verifying_key().to_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cfg = crate::config::SyncConfig {
            url: format!("http://{}/prefix", listener.local_addr().unwrap()),
            interval_secs: 60,
            insecure_http: true,
        };
        let body = serde_json::json!({
            "control_plane": {"version": "0.1.6", "commit": "abcd"},
            "coordinator": {"version": "0.1.6", "commit": "bcde"},
            "relay": {"version": "0.1.6", "commit": "cdef"},
            "server_time_unix": 120,
            "issuer_cert_not_after_unix": null
        })
        .to_string();
        let responses = vec![
            (200, body),
            (404, String::new()),
            (403, String::new()),
            (500, String::new()),
            (200, "{}".into()),
            (200, " ".repeat(crate::sync::METADATA_LIMIT + 1)),
        ];
        let server = std::thread::spawn(move || {
            let mut nonces = std::collections::HashSet::new();
            for (code, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.starts_with("GET /prefix/api/v1/server/info HTTP/1.1\r\n"));
                let auth = request
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("authorization")
                            .then_some(value.trim())
                    })
                    .unwrap();
                let signed = parse_authorization(auth).unwrap();
                assert_eq!(signed.device_id, [2; 16]);
                assert_eq!(signed.ts, 123);
                assert!(nonces.insert(signed.nonce));
                verify_request(&signed, &public, "GET", "/prefix/api/v1/server/info", &[]).unwrap();
                write!(stream, "HTTP/1.1 {code} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let info = fetch(&cfg, &key, [2; 16], 123).unwrap().unwrap();
        assert!(info.issuer_cert_not_after_unix.is_none());
        assert_eq!(info.coordinator.unwrap().commit, "bcde");
        assert_eq!(info.relay.unwrap().commit, "cdef");
        assert!(fetch(&cfg, &key, [2; 16], 123).unwrap().is_none());
        assert_eq!(
            fetch(&cfg, &key, [2; 16], 123).unwrap_err(),
            "acceso denegado"
        );
        assert_eq!(
            fetch(&cfg, &key, [2; 16], 123).unwrap_err(),
            "error HTTP del control plane"
        );
        assert_eq!(
            fetch(&cfg, &key, [2; 16], 123).unwrap_err(),
            "información del servidor inválida"
        );
        assert_eq!(
            fetch(&cfg, &key, [2; 16], 123).unwrap_err(),
            "respuesta demasiado grande"
        );
        server.join().unwrap();
    }
}
