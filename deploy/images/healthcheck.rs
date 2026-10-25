use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn healthy_status(status: &[u8; 12]) -> bool {
    (status.starts_with(b"HTTP/1.1 ") || status.starts_with(b"HTTP/1.0 "))
        && &status[9..] == b"200"
}

fn probe(mode: &str, target: &str) -> Result<(), Box<dyn std::error::Error>> {
    match mode {
        "http" => {
            let address: SocketAddr = target.parse()?;
            let timeout = Duration::from_secs(2);
            let mut stream = TcpStream::connect_timeout(&address, timeout)?;
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
            let mut status = [0; 12];
            stream.read_exact(&mut status)?;
            if !healthy_status(&status) {
                return Err("healthz no devuelve HTTP 200".into());
            }
        }
        "unix" => {
            UnixStream::connect(target)?;
        }
        _ => return Err("modo esperado: http o unix".into()),
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let result = match (args.next(), args.next(), args.next()) {
        (Some(mode), Some(target), None) => probe(&mode, &target),
        _ => Err("uso: healthcheck http DIRECCION:PUERTO | unix RUTA".into()),
    };
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
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap();
        });
        probe("http", &address.to_string()).unwrap();
        server.join().unwrap();
        let path = std::env::temp_dir().join(format!("atenea-healthcheck-{}.sock", std::process::id()));
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        probe("unix", path.to_str().unwrap()).unwrap();
        drop(listener);
        std::fs::remove_file(path).unwrap();
        assert!(probe("unknown", "unused").is_err());
    }
}
