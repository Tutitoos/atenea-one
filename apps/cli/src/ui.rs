// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Panel local de solo lectura: sirve una página y reenvía `status` del agente.
//!
//! Solo escucha en loopback y rechaza cualquier `Host` que no sea el propio (contra DNS rebinding:
//! sin ello, una web cualquiera podría leer el estado). No envía CORS, no acepta métodos de
//! escritura y nunca ofrece órdenes de control: el socket que se le da debe ser el de estado.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

const PAGE: &str = include_str!("ui.html");
const MAX_HEAD: usize = 8192;

pub fn serve(socket: &Path, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .map_err(|e| format!("no se pudo abrir 127.0.0.1:{port}: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    println!("Panel en http://127.0.0.1:{port}/  (solo lectura, Ctrl+C para salir)");
    for conn in listener.incoming().flatten() {
        handle(conn, port, &|| crate::ask(socket, "status"));
    }
    Ok(())
}

fn handle(mut conn: TcpStream, port: u16, status: &dyn Fn() -> Result<String, String>) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = conn.set_write_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < MAX_HEAD {
        match conn.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let _ = conn.write_all(&respond(&head, port, status));
}

fn respond(head: &[u8], port: u16, status: &dyn Fn() -> Result<String, String>) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(head) else {
        return reply(400, "text/plain", "petición inválida");
    };
    let mut lines = text.split("\r\n");
    let mut request = lines.next().unwrap_or("").split(' ');
    let (method, path) = (request.next().unwrap_or(""), request.next().unwrap_or(""));
    let host = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.trim());
    if host != Some(&format!("127.0.0.1:{port}")) && host != Some(&format!("localhost:{port}")) {
        return reply(403, "text/plain", "host no permitido");
    }
    if method != "GET" {
        return reply(405, "text/plain", "solo GET");
    }
    match path {
        "/" => reply(200, "text/html; charset=utf-8", PAGE),
        "/status" => match status() {
            Ok(json) => reply(200, "application/json", &json),
            Err(e) => reply(502, "text/plain", &e),
        },
        _ => reply(404, "text/plain", "no existe"),
    }
}

fn reply(code: u16, kind: &str, body: &str) -> Vec<u8> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Bad Gateway",
    };
    format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(request: &str) -> String {
        let ok = || Ok(r#"{"peers":[]}"#.to_string());
        String::from_utf8(respond(request.as_bytes(), 8787, &ok)).unwrap()
    }

    #[test]
    fn serves_page_and_status_only_to_own_host() {
        assert!(get("GET / HTTP/1.1\r\nHost: 127.0.0.1:8787\r\n\r\n").starts_with("HTTP/1.1 200"));
        let json = get("GET /status HTTP/1.1\r\nhost: localhost:8787\r\n\r\n");
        assert!(json.starts_with("HTTP/1.1 200") && json.ends_with(r#"{"peers":[]}"#));
        // Rebinding: un nombre ajeno que resuelve a 127.0.0.1, o sin cabecera Host.
        assert!(
            get("GET /status HTTP/1.1\r\nHost: evil.example:8787\r\n\r\n")
                .starts_with("HTTP/1.1 403")
        );
        assert!(get("GET /status HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 403"));
    }

    #[test]
    fn rejects_writes_unknown_paths_and_reports_agent_errors() {
        let host = "Host: 127.0.0.1:8787\r\n\r\n";
        assert!(get(&format!("POST /status HTTP/1.1\r\n{host}")).starts_with("HTTP/1.1 405"));
        assert!(get(&format!("GET /stop HTTP/1.1\r\n{host}")).starts_with("HTTP/1.1 404"));
        let down = || Err("agente: no responde".to_string());
        let out = respond(
            format!("GET /status HTTP/1.1\r\n{host}").as_bytes(),
            8787,
            &down,
        );
        assert!(String::from_utf8(out).unwrap().starts_with("HTTP/1.1 502"));
    }
}
