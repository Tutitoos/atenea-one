// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn healthy_status(status: &[u8; 12]) -> bool {
    (status.starts_with(b"HTTP/1.1 ") || status.starts_with(b"HTTP/1.0 ")) && &status[9..] == b"200"
}

fn probe(mode: &str, target: &str) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    if std::fs::read_link("/proc/self/ns/net")? != std::fs::read_link("/proc/1/ns/net")? {
        return Err("namespace de red distinto del proceso principal".into());
    }
    match mode {
        "tcp" => {
            let address = target
                .to_socket_addrs()?
                .next()
                .ok_or("destino TCP vacío")?;
            TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
        }
        "http" => {
            let address: SocketAddr = target.parse()?;
            let timeout = Duration::from_secs(2);
            let mut stream = TcpStream::connect_timeout(&address, timeout)?;
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.write_all(
                b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )?;
            let mut status = [0; 12];
            stream.read_exact(&mut status)?;
            if !healthy_status(&status) {
                return Err("healthz no devuelve HTTP 200".into());
            }
        }
        "unix" => {
            UnixStream::connect(target)?;
        }
        _ => return Err("modo esperado: http, unix o tcp".into()),
    }
    Ok(())
}

fn check_args(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let mode = args.next().ok_or("falta modo")?;
    let target = args.next().ok_or("falta destino")?;
    probe(&mode, &target)?;
    if let Some(tcp) = args.next() {
        probe("tcp", &tcp)?;
    }
    if args.next().is_some() {
        return Err("uso: healthcheck MODO DESTINO [DESTINO_TCP]".into());
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let result = check_args(std::env::args().skip(1));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("healthcheck: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_http_success() {
        assert!(healthy_status(b"HTTP/1.1 200"));
        assert!(healthy_status(b"HTTP/1.0 200"));
        assert!(!healthy_status(b"HTTP/1.1 503"));
        assert!(!healthy_status(b"NOT-HTTP 200"));
    }

    #[test]
    fn checks_http_and_unix_listeners() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 128];
            let count = stream.read(&mut request).unwrap();
            assert!(request[..count].starts_with(b"GET /healthz HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        probe("http", &address.to_string()).unwrap();
        server.join().unwrap();
        let path =
            std::env::temp_dir().join(format!("atenea-healthcheck-{}.sock", std::process::id()));
        let unix_listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        probe("unix", path.to_str().unwrap()).unwrap();
        assert!(probe("unknown", "unused").is_err());
        assert!(check_args(std::iter::empty()).is_err());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        probe("tcp", &address).unwrap();
        check_args(
            ["unix", path.to_str().unwrap(), &address]
                .map(String::from)
                .into_iter(),
        )
        .unwrap();
        drop(unix_listener);
        std::fs::remove_file(path).unwrap();
        drop(listener);
        assert!(probe("tcp", &address).is_err());
    }
}
