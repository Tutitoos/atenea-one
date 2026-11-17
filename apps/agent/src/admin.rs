// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Administración delegada (ADR-026, fase 6): lecturas de la concesión de este dispositivo y de lo que le permite
//! ver. Son datos de presentación para la aplicación de escritorio, que no tiene la clave de identidad: el agente
//! solo hace GET firmados y no ejecuta ninguna escritura delegada. Nada de esto autoriza ni enruta.
use atenea_identity::{DeviceId, SigningKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Scope {
    pub kind: String,
    #[serde(default)]
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Grant {
    pub scopes: Vec<String>,
    pub scope: Scope,
    pub granted_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
    pub fingerprint: String,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub enrolled_by_device: Option<String>,
    pub in_scope: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Policy {
    pub id: String,
    pub src: serde_json::Value,
    pub dst: serde_json::Value,
    pub proto: u16,
    pub port_min: u16,
    pub port_max: u16,
    pub created_by_device: Option<String>,
}

/// Lo último leído. `grant` nulo: este dispositivo no tiene concesión (la aplicación no muestra «Administración»).
/// `devices` y `policies` solo se leen si la concesión incluye `read:devices` y `read:policies`.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Status {
    pub grant: Option<Grant>,
    pub devices: Option<Vec<Device>>,
    pub policies: Option<Vec<Policy>>,
    pub fetched_unix: u64,
    pub last_error: Option<&'static str>,
}

fn get<T: serde::de::DeserializeOwned>(
    cfg: &crate::config::SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
    path: &str,
) -> Result<Option<T>, &'static str> {
    crate::sync::get_metadata(
        cfg,
        key,
        device,
        ts,
        &format!("/api/v1/device/admin/{path}"),
    )?
    .map(|body| serde_json::from_slice(&body).map_err(|_| "respuesta de administración inválida"))
    .transpose()
}

/// `Ok(None)`: el servidor no tiene estas rutas (anterior a ADR-026). Sin concesión (`403`) es un estado, no un error.
/// Si falla la lectura de devices o policies se conserva lo que sí se leyó y se informa el error.
pub fn fetch(
    cfg: &crate::config::SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
) -> Result<Option<Status>, &'static str> {
    let grant = match get::<Grant>(cfg, key, device, ts, "grant") {
        Ok(Some(grant)) => grant,
        Ok(None) => return Ok(None),
        Err("acceso denegado") => {
            return Ok(Some(Status {
                fetched_unix: ts,
                ..Status::default()
            }));
        }
        Err(error) => return Err(error),
    };
    let allowed = |scope: &str| grant.scopes.iter().any(|s| s == scope);
    let mut status = Status {
        fetched_unix: ts,
        ..Status::default()
    };
    if allowed("read:devices") {
        match get(cfg, key, device, ts, "devices") {
            Ok(list) => status.devices = list,
            Err(error) => status.last_error = Some(error),
        }
    }
    if allowed("read:policies") {
        match get(cfg, key, device, ts, "policies") {
            Ok(list) => status.policies = list,
            Err(error) => status.last_error = Some(error),
        }
    }
    status.grant = Some(grant);
    Ok(Some(status))
}

pub fn record(status: &mut Option<Status>, result: Result<Option<Status>, &'static str>) {
    match result {
        Ok(read) => *status = read,
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

    fn read(scopes: &[&str]) -> Status {
        Status {
            grant: Some(Grant {
                scopes: scopes.iter().map(|s| (*s).into()).collect(),
                scope: Scope {
                    kind: "enrolled".into(),
                    devices: vec![],
                },
                granted_at: "2026-11-17T10:00:00Z".into(),
            }),
            fetched_unix: 100,
            ..Status::default()
        }
    }

    #[test]
    fn a_failed_read_keeps_the_last_good_one_and_a_missing_route_clears_it() {
        let mut status = None;
        record(&mut status, Err("error de red"));
        assert!(
            status.is_none(),
            "sin lectura buena no se inventa una concesión"
        );
        record(&mut status, Ok(Some(read(&["read:devices"]))));
        record(&mut status, Err("error de red"));
        let kept = status.as_ref().unwrap();
        assert!(kept.grant.is_some());
        assert_eq!(kept.fetched_unix, 100);
        assert_eq!(kept.last_error, Some("error de red"));
        record(&mut status, Ok(Some(read(&["read:devices"]))));
        assert!(status.as_ref().unwrap().last_error.is_none());
        // Un servidor anterior (404) o la pérdida de la concesión (403) dejan de mostrar «Administración».
        record(
            &mut status,
            Ok(Some(Status {
                fetched_unix: 200,
                ..Status::default()
            })),
        );
        assert!(status.as_ref().unwrap().grant.is_none());
        record(&mut status, Ok(None));
        assert!(status.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn reads_only_what_the_grant_allows_with_signed_gets() {
        use atenea_identity::api_auth::{parse_authorization, verify_request};
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;
        let key = atenea_identity::signing_key([1; 32]);
        let public = key.verifying_key().to_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cfg = crate::config::SyncConfig {
            url: format!("http://{}/p", listener.local_addr().unwrap()),
            interval_secs: 60,
            insecure_http: true,
        };
        let grant = |scopes: &[&str]| {
            serde_json::json!({"device_id":"d","network_id":"n","scopes":scopes,"scope":{"kind":"enrolled","devices":[]},"granted_at":"2026-11-17T10:00:00Z"}).to_string()
        };
        let device = serde_json::json!([{"id":"a","name":"web","role":"device","status":"approved","fingerprint":"f","ipv4":"10.0.0.2","ipv6":null,"enrolled_at":"x","enrolled_by_device":null,"in_scope":true}]).to_string();
        // (ruta esperada, estado, cuerpo)
        let script = vec![
            ("grant", 200, grant(&["read:devices"])),
            ("devices", 200, device),
            ("grant", 200, grant(&["write:policies"])),
            ("grant", 403, String::new()),
            ("grant", 404, String::new()),
            ("grant", 200, grant(&["read:devices", "read:policies"])),
            ("devices", 200, "[]".into()),
            ("policies", 500, String::new()),
            ("grant", 200, "{\"scopes\":1}".into()),
        ];
        let server = std::thread::spawn(move || {
            for (path, code, body) in script {
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
                let want = format!("/p/api/v1/device/admin/{path}");
                assert!(
                    request.starts_with(&format!("GET {want} HTTP/1.1\r\n")),
                    "{request}"
                );
                let auth = request
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                            .map(|(_, v)| v.trim())
                    })
                    .unwrap();
                let signed = parse_authorization(auth).unwrap();
                verify_request(&signed, &public, "GET", &want, &[]).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let f = || fetch(&cfg, &key, [2; 16], 123);
        let one = f().unwrap().unwrap();
        assert_eq!(one.devices.as_ref().unwrap()[0].name, "web");
        assert!(one.policies.is_none(), "sin read:policies no se pide");
        // Solo escritura: ni inventario ni políticas, y no se piden.
        let two = f().unwrap().unwrap();
        assert!(two.grant.is_some() && two.devices.is_none() && two.policies.is_none());
        // 403 = sin concesión (estado); 404 = servidor anterior.
        let none = f().unwrap().unwrap();
        assert!(none.grant.is_none() && none.last_error.is_none());
        assert!(f().unwrap().is_none());
        // Un fallo al leer las políticas conserva lo leído e informa el error.
        let partial = f().unwrap().unwrap();
        assert_eq!(partial.devices, Some(vec![]));
        assert!(partial.policies.is_none());
        assert_eq!(partial.last_error, Some("error HTTP del control plane"));
        assert_eq!(f().unwrap_err(), "respuesta de administración inválida");
        server.join().unwrap();
    }
}
