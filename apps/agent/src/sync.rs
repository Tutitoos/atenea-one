//! Descarga acotada: solo `poll_config` decide si la configuración es de confianza.
use crate::config::SyncConfig;
use atenea_identity::{DeviceId, MAX_BODY_LEN, SigningKey, api_auth::sign_request};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

pub const RESPONSE_LIMIT: usize = MAX_BODY_LEN + 512;

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub url: String,
    pub last_ok_unix: Option<u64>,
    pub last_error: Option<&'static str>,
    pub sync_ok: u64,
    pub sync_denied: u64,
    pub sync_errors: u64,
}

impl Status {
    pub fn record(&mut self, result: Result<bool, &'static str>, unix: u64) {
        match result {
            Ok(_) => {
                self.sync_ok += 1;
                self.last_ok_unix = Some(unix);
                self.last_error = None;
            }
            Err(code) => {
                if code == "acceso denegado" {
                    self.sync_denied += 1;
                } else {
                    self.sync_errors += 1;
                }
                self.last_error = Some(code);
                tracing::warn!(code, "sincronización fallida");
            }
        }
    }
}

fn request(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
    nonce: [u8; 16],
) -> Result<(String, String), &'static str> {
    let url = cfg.endpoint().map_err(|_| "URL inválida")?;
    let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
    let path = uri.path_and_query().ok_or("URL inválida")?.as_str();
    let authorization = sign_request(key, device, "GET", path, &[], ts, nonce);
    Ok((url, authorization))
}

fn validate_body(body: &[u8]) -> Result<(), &'static str> {
    if body.len() > RESPONSE_LIMIT {
        return Err("respuesta demasiado grande");
    }
    if !body.starts_with(b"AOCF") {
        return Err("respuesta no es SignedConfig");
    }
    Ok(())
}

fn write_changed(path: &Path, body: &[u8]) -> Result<bool, &'static str> {
    validate_body(body)?;
    match fs::read(path) {
        Ok(current) if current == body => return Ok(false),
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err("no se pudo leer config_file"),
    }
    let suffix = atenea_crypto::hex::encode(&atenea_identity::generate_seed()[..16]);
    let tmp = path.with_file_name(format!(".atenea-sync-{suffix}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|_| "no se pudo crear el temporal")?;
    let result = (|| {
        file.write_all(body)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        std::fs::File::open(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|_| "no se pudo guardar config_file")?;
    Ok(true)
}

pub fn fetch(
    cfg: &SyncConfig,
    key: &SigningKey,
    device: DeviceId,
    path: &Path,
    ts: u64,
) -> Result<bool, &'static str> {
    let mut nonce = [0; 16];
    nonce.copy_from_slice(&atenea_identity::generate_seed()[..16]);
    let (url, authorization) = request(cfg, key, device, ts, nonce)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("Authorization", &authorization)
        .call()
        .map_err(|_| "error de red")?;
    match response.status().as_u16() {
        200 => (),
        401 | 403 => return Err("acceso denegado"),
        _ => return Err("error HTTP del control plane"),
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "respuesta ilegible")?;
    write_changed(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    fn cfg(url: &str) -> SyncConfig {
        SyncConfig {
            url: url.into(),
            interval_secs: 60,
        }
    }
    fn temp() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "atenea-sync-{}",
            atenea_crypto::hex::encode(&atenea_identity::generate_seed())
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn signed_request_and_body_limits() {
        let key = atenea_identity::signing_key([1; 32]);
        let (url, auth) = request(
            &cfg("https://example.test/prefix/"),
            &key,
            [2; 16],
            123,
            [3; 16],
        )
        .unwrap();
        assert_eq!(url, "https://example.test/prefix/api/v1/device/config");
        assert_eq!(
            auth,
            sign_request(
                &key,
                [2; 16],
                "GET",
                "/prefix/api/v1/device/config",
                &[],
                123,
                [3; 16]
            )
        );
        assert!(validate_body(b"bad").is_err());
        assert!(validate_body(&vec![0; RESPONSE_LIMIT + 1]).is_err());
        assert!(validate_body(b"AOCF").is_ok());
        assert!(cfg("https://user:secret@example.test").endpoint().is_err());
    }
    #[test]
    fn atomic_write_and_noop() {
        let dir = temp();
        let path = dir.join("config.bin");
        assert_eq!(write_changed(&path, b"AOCFfirst"), Ok(true));
        let metadata = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(write_changed(&path, b"AOCFfirst"), Ok(false));
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), metadata);
        assert!(write_changed(&path, b"invalid").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"AOCFfirst");
        assert_eq!(write_changed(&path, b"AOCFsecond"), Ok(true));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn local_http_success_denied_invalid() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (status, body) in [(200, "AOCFtest"), (403, "denied"), (200, "invalid")] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("GET /api/v1/device/config HTTP/1.1\r\n"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: atdevice v1 ")
                );
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let dir = temp();
        let path = dir.join("config.bin");
        let key = atenea_identity::signing_key([1; 32]);
        let mut status = Status::default();
        for _ in 0..3 {
            status.record(fetch(&cfg(&url), &key, [2; 16], &path, 123), 123);
        }
        assert_eq!(
            (status.sync_ok, status.sync_denied, status.sync_errors),
            (1, 1, 1)
        );
        assert_eq!(status.last_ok_unix, Some(123));
        assert_eq!(fs::read(&path).unwrap(), b"AOCFtest");
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
