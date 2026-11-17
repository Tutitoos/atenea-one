// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Administración delegada desde la CLI (ADR-026, fase 4): órdenes bajo `atenea network` que ejercen, con la
//! identidad de este dispositivo, los permisos que un administrador le concedió en una red. Solo hablan con
//! `/api/v1/device/admin/`; el servidor decide permisos y alcance, aquí solo se traducen sus respuestas.
use crate::enroll::signed;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const BASE: &str = "/api/v1/device/admin";

/// Opciones comunes: la red (id completo, prefijo único o nombre) y la salida JSON del servidor.
#[derive(Args)]
pub struct Common {
    /// Id completo, prefijo único o nombre de la red.
    #[arg(long)]
    pub network: String,
    /// Imprime la respuesta del servidor tal cual, sin formatear.
    #[arg(long)]
    pub json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Proto {
    Any,
    Icmp,
    Tcp,
    Udp,
    Icmpv6,
}

impl Proto {
    fn number(self) -> u8 {
        match self {
            Proto::Any => 0,
            Proto::Icmp => 1,
            Proto::Tcp => 6,
            Proto::Udp => 17,
            Proto::Icmpv6 => 58,
        }
    }
}

#[derive(Subcommand)]
pub enum PolicyCmd {
    /// Lista las políticas de la red (permiso `read:policies`).
    List(#[command(flatten)] Common),
    /// Crea una política entre dos dispositivos del alcance (permiso `write:policies`). Como toda política, también
    /// permite el sentido inverso de las respuestas y, sin puertos, el tráfico de ida y vuelta (ADR-016).
    Add {
        #[command(flatten)]
        common: Common,
        /// Origen: id o nombre del dispositivo.
        #[arg(long)]
        from: String,
        /// Destino: id o nombre del dispositivo.
        #[arg(long)]
        to: String,
        #[arg(long, value_enum, default_value = "any")]
        proto: Proto,
        /// Puerto o rango (`22`, `8000-8100`); solo con tcp o udp. Sin él, todos.
        #[arg(long)]
        ports: Option<String>,
    },
    /// Borra una política creada por un dispositivo delegado y de su alcance (permiso `write:policies`).
    Remove {
        /// Id de la política.
        id: String,
        #[command(flatten)]
        common: Common,
        /// No pregunta: confirma el borrado (obligatorio sin terminal interactiva).
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum AdminCmd {
    /// Muestra los permisos y el alcance que este dispositivo tiene en la red.
    Permissions(#[command(flatten)] Common),
    /// Lista los dispositivos de la red (permiso `read:devices`).
    Devices(#[command(flatten)] Common),
    /// Genera un código de unión de un solo uso (permiso `write:join-codes`); se imprime una sola vez y no se guarda.
    JoinCode {
        #[command(flatten)]
        common: Common,
        /// Caducidad en segundos, de 60 a 3600.
        #[arg(long, default_value_t = 600)]
        ttl: u64,
    },
    /// Aprueba un dispositivo pendiente de este alcance, indicando su huella completa (permiso `write:approve-devices`).
    Approve {
        /// Huella completa, tal como la muestra el equipo que se une (`xxxx-xxxx-…`, 8 grupos).
        fingerprint: String,
        #[command(flatten)]
        common: Common,
    },
    /// Revoca un dispositivo de este alcance (permiso `write:revoke-devices`).
    Revoke {
        /// Id o nombre del dispositivo.
        device: String,
        #[command(flatten)]
        common: Common,
        /// No pregunta: confirma la revocación (obligatorio sin terminal interactiva).
        #[arg(long)]
        yes: bool,
    },
    /// Políticas de la red.
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },
}

impl AdminCmd {
    pub fn network(&self) -> &str {
        match self {
            AdminCmd::Permissions(c) | AdminCmd::Devices(c) => &c.network,
            AdminCmd::JoinCode { common, .. }
            | AdminCmd::Approve { common, .. }
            | AdminCmd::Revoke { common, .. } => &common.network,
            AdminCmd::Policy { cmd } => match cmd {
                PolicyCmd::List(c) => &c.network,
                PolicyCmd::Add { common, .. } | PolicyCmd::Remove { common, .. } => &common.network,
            },
        }
    }
}

/// La red y la identidad con la que se firma.
pub struct Target {
    pub url: String,
    pub device_id: String,
    pub dir: PathBuf,
}

/// Traduce la respuesta de error del servidor; ninguna lleva datos del cuerpo salvo el código estable.
fn message(status: u16, body: &str) -> String {
    let code = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error")?.get("code")?.as_str().map(str::to_owned));
    let text = match (status, code.as_deref()) {
        (403, Some("grant_missing")) => {
            "este dispositivo no tiene ese permiso en la red: pídeselo a un administrador"
        }
        (403, Some("out_of_scope")) => {
            "fuera del alcance que un administrador concedió a este dispositivo"
        }
        (403, Some("device_not_approved")) => "este dispositivo no está aprobado en la red",
        (401, _) => {
            "firma rechazada: comprueba el reloj del equipo y que el dispositivo no esté revocado"
        }
        (404, Some("not_found")) => "no encontrado en esta red",
        (404, _) => "el servidor no admite administración delegada (actualiza el control plane)",
        (409, Some("fingerprint_mismatch")) => "la huella no coincide con la del dispositivo",
        (409, Some("device_not_pending")) => "el dispositivo ya no está pendiente",
        (409, Some("join_code_limit")) => {
            "hay demasiados códigos vivos de este dispositivo: espera a que caduquen o se usen"
        }
        (409, Some("policy_overflow")) => "se alcanzó el máximo de políticas",
        (400, Some("invalid_ttl")) => "la caducidad debe estar entre 60 y 3600 segundos",
        (400, Some("invalid_selector")) => {
            "un dispositivo de la política no existe en la red o está revocado"
        }
        (400, Some("invalid_ports")) => "puertos o protocolo no válidos",
        (429, _) => "demasiadas peticiones; inténtalo más tarde",
        (500..=599, _) => "control plane no disponible",
        _ => return format!("respuesta inesperada del servidor (HTTP {status})"),
    };
    text.into()
}

fn call(t: &Target, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let bytes = body.map(Value::to_string).unwrap_or_default();
    let reply = signed(
        &t.url,
        &t.device_id,
        &t.dir,
        method,
        &format!("{BASE}{path}"),
        bytes.as_bytes(),
    )?;
    if !(200..300).contains(&reply.status) {
        return Err(message(reply.status, &reply.body));
    }
    Ok(serde_json::from_str(&reply.body).unwrap_or(Value::Null))
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("-")
}

fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Una huella completa: 8 grupos de 4 hex en minúsculas separados por guiones.
fn valid_fingerprint(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 8
        && groups
            .iter()
            .all(|g| g.len() == 4 && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

fn ports(proto: Proto, text: Option<&str>) -> Result<(u16, u16), String> {
    let Some(text) = text else { return Ok((0, 0)) };
    if !matches!(proto, Proto::Tcp | Proto::Udp) {
        return Err("--ports solo existe con --proto tcp o udp".into());
    }
    let port = |s: &str| {
        s.parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| format!("puerto no válido: {s}"))
    };
    let (min, max) = match text.split_once('-') {
        Some((a, b)) => (port(a)?, port(b)?),
        None => {
            let p = port(text)?;
            (p, p)
        }
    };
    if min > max {
        return Err("el rango de puertos va de menor a mayor".into());
    }
    Ok((min, max))
}

fn devices(t: &Target) -> Result<Vec<Value>, String> {
    match call(t, "GET", "/devices", None)? {
        Value::Array(list) => Ok(list),
        _ => Err("respuesta inesperada del servidor".into()),
    }
}

/// Id de un dispositivo dado por id o por nombre; un nombre debe ser único entre los no revocados.
fn device_id(t: &Target, spec: &str) -> Result<String, String> {
    if looks_like_uuid(spec) {
        return Ok(spec.to_lowercase());
    }
    pick(&devices(t)?, spec)
}

fn pick(list: &[Value], name: &str) -> Result<String, String> {
    let found: Vec<&Value> = list
        .iter()
        .filter(|d| text(d, "name") == name && text(d, "status") != "revoked")
        .collect();
    match found.as_slice() {
        [one] => Ok(text(one, "id").to_owned()),
        [] => Err(format!("ningún dispositivo se llama «{name}»; usa su id")),
        _ => Err(format!(
            "hay varios dispositivos llamados «{name}»; usa su id"
        )),
    }
}

fn name_of(list: &[Value], id: &str) -> String {
    list.iter()
        .find(|d| text(d, "id") == id)
        .map_or_else(|| id.to_owned(), |d| text(d, "name").to_owned())
}

fn selector(list: &[Value], s: &Value) -> String {
    match text(s, "kind") {
        "device" => name_of(list, text(s, "device_id")),
        "prefix" => text(s, "prefix").to_owned(),
        _ => "cualquiera".into(),
    }
}

fn show(out: &mut dyn Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{v}").map_err(|e| e.to_string())
}

fn line(out: &mut dyn Write, s: String) -> Result<(), String> {
    writeln!(out, "{s}").map_err(|e| e.to_string())
}

pub fn run(
    t: &Target,
    cmd: AdminCmd,
    input: &mut dyn BufRead,
    interactive: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    match cmd {
        AdminCmd::Permissions(c) => {
            let g = call(t, "GET", "/grant", None)?;
            if c.json {
                return show(out, &g);
            }
            let scopes: Vec<&str> = g["scopes"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            line(out, format!("Permisos: {}", scopes.join(", ")))?;
            let alcance = match text(&g["scope"], "kind") {
                "network" => "toda la red".to_owned(),
                "devices" => format!(
                    "{} dispositivo(s) concretos",
                    g["scope"]["devices"].as_array().map_or(0, Vec::len)
                ),
                _ => "los dispositivos que dio de alta y él mismo".into(),
            };
            line(out, format!("Alcance de las escrituras: {alcance}"))
        }
        AdminCmd::Devices(c) => {
            let list = call(t, "GET", "/devices", None)?;
            if c.json {
                return show(out, &list);
            }
            for d in list.as_array().into_iter().flatten() {
                line(
                    out,
                    format!(
                        "{} {:<10} {:<24} {:<15} {} {}",
                        if d["in_scope"] == true { "*" } else { " " },
                        text(d, "status"),
                        text(d, "name"),
                        text(d, "ipv4"),
                        text(d, "fingerprint"),
                        text(d, "id"),
                    ),
                )?;
            }
            line(out, "(* = dentro del alcance de este dispositivo)".into())
        }
        AdminCmd::JoinCode { common, ttl } => {
            let r = call(t, "POST", "/join-codes", Some(&json!({"ttl_secs": ttl})))?;
            if common.json {
                return show(out, &r);
            }
            let code = r["code"]
                .as_str()
                .ok_or("el servidor no pudo componer el código de unión")?;
            line(
                out,
                format!(
                    "Código de unión (un solo uso, caduca {}; no se guarda en ningún sitio):",
                    text(&r, "expires_at")
                ),
            )?;
            line(out, code.to_owned())?;
            line(
                out,
                "El equipo entrará pendiente: apruébalo con `atenea network approve <huella>`."
                    .into(),
            )
        }
        AdminCmd::Approve {
            fingerprint,
            common,
        } => {
            let fp = fingerprint.trim().to_lowercase();
            if !valid_fingerprint(&fp) {
                return Err(
                    "se exige la huella completa: 8 grupos de 4 hex separados por guiones".into(),
                );
            }
            let list = devices(t)?;
            let d = list
                .iter()
                .find(|d| text(d, "fingerprint") == fp)
                .ok_or("ningún dispositivo de la red tiene esa huella")?;
            if text(d, "status") != "pending" {
                return Err(format!(
                    "el dispositivo está {}, no pendiente",
                    text(d, "status")
                ));
            }
            let id = text(d, "id").to_owned();
            let r = call(
                t,
                "POST",
                &format!("/devices/{id}/approve"),
                Some(&json!({"fingerprint": fp})),
            )?;
            if common.json {
                return show(out, &r);
            }
            line(out, format!("Aprobado «{}» ({id}).", text(d, "name")))
        }
        AdminCmd::Revoke {
            device,
            common,
            yes,
        } => {
            let id = device_id(t, &device)?;
            crate::network_common::confirm(
                &format!("¿Revocar el dispositivo {id}? Pierde el acceso a la red"),
                yes,
                interactive,
                input,
            )?;
            let r = call(t, "POST", &format!("/devices/{id}/revoke"), None)?;
            if common.json {
                return show(out, &r);
            }
            line(out, format!("Revocado {id}."))
        }
        AdminCmd::Policy { cmd } => policy(t, cmd, input, interactive, out),
    }
}

fn policy(
    t: &Target,
    cmd: PolicyCmd,
    input: &mut dyn BufRead,
    interactive: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    match cmd {
        PolicyCmd::List(c) => {
            let list = call(t, "GET", "/policies", None)?;
            if c.json {
                return show(out, &list);
            }
            // Los nombres son un añadido: sin `read:devices` se muestran los ids.
            let names = devices(t).unwrap_or_default();
            for p in list.as_array().into_iter().flatten() {
                let (min, max) = (p["port_min"].as_u64(), p["port_max"].as_u64());
                let proto = match p["proto"].as_u64() {
                    Some(1) => "icmp",
                    Some(6) => "tcp",
                    Some(17) => "udp",
                    Some(58) => "icmpv6",
                    _ => "any",
                };
                let puertos = match (min, max) {
                    (Some(a), Some(b)) if a == b && a != 0 => format!(":{a}"),
                    (Some(a), Some(b)) if a != 0 || b != 0 => format!(":{a}-{b}"),
                    _ => String::new(),
                };
                line(
                    out,
                    format!(
                        "{} {} -> {} {proto}{puertos}{}",
                        text(p, "id"),
                        selector(&names, &p["src"]),
                        selector(&names, &p["dst"]),
                        if p["created_by_device"].is_null() {
                            "  (de un administrador)"
                        } else {
                            ""
                        }
                    ),
                )?;
            }
            Ok(())
        }
        PolicyCmd::Add {
            common,
            from,
            to,
            proto,
            ports: range,
        } => {
            let (min, max) = ports(proto, range.as_deref())?;
            let (src, dst) = (device_id(t, &from)?, device_id(t, &to)?);
            let r = call(
                t,
                "POST",
                "/policies",
                Some(&json!({
                    "src": {"kind": "device", "device_id": src},
                    "dst": {"kind": "device", "device_id": dst},
                    "proto": proto.number(),
                    "port_min": min,
                    "port_max": max,
                })),
            )?;
            if common.json {
                return show(out, &r);
            }
            line(out, format!("Política creada: {}.", text(&r, "id")))
        }
        PolicyCmd::Remove { id, common, yes } => {
            if !looks_like_uuid(&id) {
                return Err("se espera el id de la política".into());
            }
            crate::network_common::confirm(
                &format!("¿Borrar la política {id}?"),
                yes,
                interactive,
                input,
            )?;
            call(t, "DELETE", &format!("/policies/{id}"), None)?;
            if common.json {
                return show(out, &json!({"id": id, "deleted": true}));
            }
            line(out, format!("Política {id} borrada."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        cmd: AdminCmd,
    }

    fn parse(args: &[&str]) -> Result<AdminCmd, clap::Error> {
        Cli::try_parse_from(["atenea"].iter().chain(args)).map(|c| c.cmd)
    }

    const FP: &str = "1234-5678-9abc-def0-1234-5678-9abc-def0";

    #[test]
    fn the_commands_need_a_network_and_take_the_documented_arguments() {
        assert!(parse(&["devices"]).is_err());
        let c = parse(&["approve", FP, "--network", "casa"]).unwrap();
        assert_eq!(c.network(), "casa");
        assert!(parse(&["policy", "add", "--network", "n", "--from", "a"]).is_err());
        assert!(
            parse(&[
                "policy",
                "add",
                "--network",
                "n",
                "--from",
                "a",
                "--to",
                "b",
                "--proto",
                "sctp"
            ])
            .is_err()
        );
        let c = parse(&["policy", "remove", "x", "--network", "n", "--yes"]).unwrap();
        assert_eq!(c.network(), "n");
        let c = parse(&["join-code", "--network", "n", "--ttl", "90", "--json"]).unwrap();
        let AdminCmd::JoinCode { ttl, common } = c else {
            panic!()
        };
        assert_eq!((ttl, common.json), (90, true));
    }

    #[test]
    fn ports_are_validated_before_any_request() {
        assert_eq!(ports(Proto::Tcp, None), Ok((0, 0)));
        assert_eq!(ports(Proto::Tcp, Some("22")), Ok((22, 22)));
        assert_eq!(ports(Proto::Udp, Some("8000-8100")), Ok((8000, 8100)));
        for bad in ["0", "70000", "9-3", "a", "1-", "-1", "1-2-3"] {
            assert!(ports(Proto::Tcp, Some(bad)).is_err(), "{bad}");
        }
        // Sin puertos en protocolos que no los tienen.
        assert!(ports(Proto::Any, Some("22")).is_err());
        assert!(ports(Proto::Icmp, Some("22")).is_err());
    }

    #[test]
    fn only_a_complete_fingerprint_is_accepted() {
        assert!(valid_fingerprint(FP));
        for bad in [
            "1234-5678",
            "1234-5678-9ABC-DEF0-1234-5678-9ABC-DEF0",
            "1234-5678-9abc-def0-1234-5678-9abc-def",
            "1234-5678-9abc-def0-1234-5678-9abc-def0-0000",
            "zzzz-dc70-8d48-49f1-7280-52c8-f5af-2e46",
            "",
        ] {
            assert!(!valid_fingerprint(bad), "{bad}");
        }
    }

    #[test]
    fn a_name_resolves_only_when_it_is_unique_among_the_living() {
        let d =
            |id: &str, name: &str, status: &str| json!({"id": id, "name": name, "status": status});
        let list = vec![
            d("a", "web", "approved"),
            d("b", "web", "revoked"),
            d("c", "db", "approved"),
            d("d", "db", "pending"),
        ];
        assert_eq!(pick(&list, "web"), Ok("a".into()));
        assert!(pick(&list, "db").unwrap_err().contains("varios"));
        assert!(pick(&list, "nada").unwrap_err().contains("ningún"));
        assert!(looks_like_uuid("01a1ea9d-0a5e-7000-8000-000000000001"));
        assert!(!looks_like_uuid("01a1ea9d0a5e70008000000000000001"));
    }

    #[test]
    fn server_errors_become_actionable_messages() {
        let body = |code: &str| format!(r#"{{"error":{{"code":"{code}","message":"{code}"}}}}"#);
        assert!(message(403, &body("grant_missing")).contains("permiso"));
        assert!(message(403, &body("out_of_scope")).contains("alcance"));
        assert!(message(409, &body("fingerprint_mismatch")).contains("huella"));
        assert!(message(404, &body("not_found")).contains("no encontrado"));
        // Un servidor anterior no tiene las rutas: 404 sin el código de la API.
        assert!(message(404, &body("invalid_request")).contains("no admite"));
        assert!(message(404, "").contains("no admite"));
        assert!(message(502, "<html>").contains("no disponible"));
        assert!(message(418, "").contains("418"));
    }

    #[cfg(unix)]
    mod http {
        use super::*;
        use std::io::{Cursor, Read};
        use std::net::TcpListener;

        /// Atiende una petición por respuesta, en orden, y devuelve las peticiones recibidas.
        fn serve(responses: Vec<(u16, String)>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let h = std::thread::spawn(move || {
                let mut seen = Vec::new();
                for (status, body) in responses {
                    let (mut s, _) = l.accept().unwrap();
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = s.read(&mut chunk).unwrap();
                        buf.extend_from_slice(&chunk[..n]);
                        let t = String::from_utf8_lossy(&buf).into_owned();
                        if let Some(i) = t.find("\r\n\r\n") {
                            let len = t[..i]
                                .to_lowercase()
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if buf.len() >= i + 4 + len {
                                break;
                            }
                        }
                        if n == 0 {
                            break;
                        }
                    }
                    write!(
                        s,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                    seen.push(String::from_utf8_lossy(&buf).into_owned());
                }
                seen
            });
            (port, h)
        }

        fn target(port: u16, tag: &str) -> Target {
            let dir =
                std::env::temp_dir().join(format!("atenea-admin-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            crate::issue::write_secret(
                &dir.join("identity.key"),
                &atenea_crypto::hex::encode(&[42; 32]),
            )
            .unwrap();
            std::fs::write(dir.join("agent.toml"), "[sync]\ninsecure_http = true\n").unwrap();
            Target {
                url: format!("http://127.0.0.1:{port}"),
                device_id: "01a1ea9d-0a5e-7000-8000-000000000001".into(),
                dir,
            }
        }

        fn exec(t: &Target, cmd: AdminCmd, yes_input: &str) -> (Result<(), String>, String) {
            let mut out = Vec::new();
            let r = run(
                t,
                cmd,
                &mut Cursor::new(yes_input.to_owned()),
                false,
                &mut out,
            );
            (r, String::from_utf8(out).unwrap())
        }

        fn verify(request: &str, method: &str, path: &str, body: &str) {
            assert!(
                request.starts_with(&format!("{method} {path} HTTP/1.1")),
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
            let signed = atenea_identity::api_auth::parse_authorization(auth).unwrap();
            let key = atenea_identity::signing_key([42; 32]);
            atenea_identity::api_auth::verify_request(
                &signed,
                &key.verifying_key().to_bytes(),
                method,
                path,
                body.as_bytes(),
            )
            .unwrap();
            assert!(request.ends_with(body), "{request}");
        }

        #[test]
        fn approve_finds_the_pending_device_by_fingerprint_and_signs_both_requests() {
            let devices = json!([
                {"id":"d1","name":"otro","status":"pending","fingerprint":"0000-0000-0000-0000-0000-0000-0000-0001"},
                {"id":"d2","name":"nuevo","status":"pending","fingerprint":FP}
            ]);
            let (port, h) = serve(vec![
                (200, devices.to_string()),
                (200, r#"{"device_id":"d2","status":"approved"}"#.into()),
            ]);
            let t = target(port, "aprobar");
            let c = parse(&["approve", FP, "--network", "n"]).unwrap();
            let (r, out) = exec(&t, c, "");
            r.unwrap();
            assert!(out.contains("Aprobado «nuevo»"), "{out}");
            let seen = h.join().unwrap();
            verify(&seen[0], "GET", "/api/v1/device/admin/devices", "");
            verify(
                &seen[1],
                "POST",
                "/api/v1/device/admin/devices/d2/approve",
                &json!({"fingerprint": FP}).to_string(),
            );
            std::fs::remove_dir_all(&t.dir).unwrap();
        }

        #[test]
        fn approve_never_posts_for_an_unknown_or_already_approved_fingerprint() {
            for status in ["approved", "revoked"] {
                let devices = json!([{"id":"d2","name":"x","status":status,"fingerprint":FP}]);
                let (port, h) = serve(vec![(200, devices.to_string())]);
                let t = target(port, "aprobar-no");
                let (r, _) = exec(&t, parse(&["approve", FP, "--network", "n"]).unwrap(), "");
                assert!(r.unwrap_err().contains("no pendiente"));
                assert_eq!(h.join().unwrap().len(), 1);
                std::fs::remove_dir_all(&t.dir).unwrap();
            }
            let (port, h) = serve(vec![(200, "[]".into())]);
            let t = target(port, "aprobar-ninguno");
            let (r, _) = exec(&t, parse(&["approve", FP, "--network", "n"]).unwrap(), "");
            assert!(r.unwrap_err().contains("ningún dispositivo"));
            assert_eq!(h.join().unwrap().len(), 1);
            std::fs::remove_dir_all(&t.dir).unwrap();
        }

        #[test]
        fn join_code_prints_the_code_once_and_errors_are_translated() {
            let (port, h) = serve(vec![(
                201,
                r#"{"token":"t","expires_at":"2026-11-17T10:00:00Z","code":"ATENEA1-ABC"}"#.into(),
            )]);
            let t = target(port, "codigo");
            let c = parse(&["join-code", "--network", "n", "--ttl", "120"]).unwrap();
            let (r, out) = exec(&t, c, "");
            r.unwrap();
            assert_eq!(out.matches("ATENEA1-ABC").count(), 1, "{out}");
            let seen = h.join().unwrap();
            verify(
                &seen[0],
                "POST",
                "/api/v1/device/admin/join-codes",
                r#"{"ttl_secs":120}"#,
            );
            // Nada de lo recibido queda en el directorio del dispositivo.
            let mut names: Vec<_> = std::fs::read_dir(&t.dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            assert_eq!(names, ["agent.toml", "identity.key"]);
            std::fs::remove_dir_all(&t.dir).unwrap();

            let (port, h) = serve(vec![(
                403,
                r#"{"error":{"code":"grant_missing","message":"grant_missing"}}"#.into(),
            )]);
            let t = target(port, "codigo-403");
            let (r, out) = exec(&t, parse(&["join-code", "--network", "n"]).unwrap(), "");
            assert!(r.unwrap_err().contains("pídeselo a un administrador"));
            assert!(out.is_empty());
            h.join().unwrap();
            std::fs::remove_dir_all(&t.dir).unwrap();
        }

        #[test]
        fn policy_add_resolves_names_and_sends_only_device_selectors() {
            let devices = json!([
                {"id":"01a1ea9d-0a5e-7000-8000-00000000000a","name":"dev","status":"approved"},
                {"id":"01a1ea9d-0a5e-7000-8000-00000000000b","name":"k3s","status":"approved"}
            ]);
            let (port, h) = serve(vec![
                (200, devices.to_string()),
                (200, devices.to_string()),
                (201, r#"{"id":"p1"}"#.into()),
            ]);
            let t = target(port, "politica");
            let c = parse(&[
                "policy",
                "add",
                "--network",
                "n",
                "--from",
                "dev",
                "--to",
                "k3s",
                "--proto",
                "tcp",
                "--ports",
                "22",
            ])
            .unwrap();
            let (r, out) = exec(&t, c, "");
            r.unwrap();
            assert!(out.contains("p1"));
            let seen = h.join().unwrap();
            verify(
                &seen[2],
                "POST",
                "/api/v1/device/admin/policies",
                &json!({
                    "src":{"kind":"device","device_id":"01a1ea9d-0a5e-7000-8000-00000000000a"},
                    "dst":{"kind":"device","device_id":"01a1ea9d-0a5e-7000-8000-00000000000b"},
                    "proto":6,"port_min":22,"port_max":22
                })
                .to_string(),
            );
            std::fs::remove_dir_all(&t.dir).unwrap();
        }

        #[test]
        fn revoke_and_policy_remove_ask_first_and_send_nothing_without_confirmation() {
            // Sin `--yes` y sin terminal no se envía nada: el servidor ni se contacta.
            let t = target(1, "confirmar");
            let id = "01a1ea9d-0a5e-7000-8000-00000000000a";
            for args in [
                vec!["revoke", id, "--network", "n"],
                vec!["policy", "remove", id, "--network", "n"],
            ] {
                let (r, _) = exec(&t, parse(&args).unwrap(), "");
                assert!(r.is_err());
            }
            std::fs::remove_dir_all(&t.dir).unwrap();

            let (port, h) = serve(vec![
                (200, r#"{"device_id":"x","status":"revoked"}"#.into()),
                (204, String::new()),
            ]);
            let t = target(port, "confirmado");
            let (r, _) = exec(
                &t,
                parse(&["revoke", id, "--network", "n", "--yes"]).unwrap(),
                "",
            );
            r.unwrap();
            let (r, out) = exec(
                &t,
                parse(&["policy", "remove", id, "--network", "n", "--yes"]).unwrap(),
                "",
            );
            r.unwrap();
            assert!(out.contains("borrada"));
            let seen = h.join().unwrap();
            verify(
                &seen[0],
                "POST",
                &format!("/api/v1/device/admin/devices/{id}/revoke"),
                "",
            );
            verify(
                &seen[1],
                "DELETE",
                &format!("/api/v1/device/admin/policies/{id}"),
                "",
            );
            std::fs::remove_dir_all(&t.dir).unwrap();
        }
    }
}
