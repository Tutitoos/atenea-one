//! Control local autenticado y socket separado de solo lectura.
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Stop,
    Reload,
    Cleanup,
}

pub fn bind(path: &Path, status_gid: Option<u32>, read_only: bool) -> Result<UnixListener, String> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(format!("{}: ya hay un agente activo", path.display()));
        }
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(path).map_err(|e| format!("IPC {}: {e}", path.display()))?;
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if read_only { 0o660 } else { 0o600 }),
    )
    .map_err(|e| e.to_string())?;
    if read_only && let Some(gid) = status_gid {
        std::os::unix::fs::chown(path, None, Some(gid)).map_err(|e| e.to_string())?;
    }
    Ok(listener)
}
fn allowed(uid: u32, owner: u32) -> bool {
    uid == 0 || uid == owner
}
fn action(command: &str, read_only: bool) -> Result<Action, &'static str> {
    match command {
        "status" => Ok(Action::None),
        _ if read_only => Err("socket de estado: solo se permite status"),
        "stop" => Ok(Action::Stop),
        "reload" => Ok(Action::Reload),
        "cleanup" => Ok(Action::Cleanup),
        _ => Err("orden desconocida"),
    }
}
pub async fn handle(
    stream: UnixStream,
    socket: &Path,
    read_only: bool,
    status: serde_json::Value,
) -> Action {
    if !read_only {
        let authorized = stream
            .peer_cred()
            .ok()
            .zip(std::fs::metadata(socket).ok())
            .is_some_and(|(cred, meta)| allowed(cred.uid(), meta.uid()));
        if !authorized {
            return Action::None;
        }
    }
    let mut reader = BufReader::new(stream);
    // Límite por byte: ningún cliente local puede hacer crecer la línea sin cota.
    let mut line = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let bytes = reader.fill_buf().await?;
            if bytes.is_empty() {
                return Ok::<(), std::io::Error>(());
            }
            let byte = bytes[0];
            reader.consume(1);
            if byte == b'\n' {
                return Ok(());
            }
            line.push(byte);
            if line.len() > 32 {
                return Err(std::io::Error::other("orden demasiado larga"));
            }
        }
    })
    .await;
    if !matches!(read, Ok(Ok(()))) {
        return Action::None;
    }
    let command = String::from_utf8_lossy(&line);
    let (reply, result) = match action(command.trim(), read_only) {
        Ok(Action::None) => (status, Action::None),
        Ok(result) => (serde_json::json!({"ok":true}), result),
        Err(error) => (serde_json::json!({"error":error}), Action::None),
    };
    let _ = reader
        .get_mut()
        .write_all(format!("{reply}\n").as_bytes())
        .await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_uid_cannot_control_and_status_cannot_stop() {
        assert!(allowed(0, 100));
        assert!(allowed(100, 100));
        assert!(!allowed(101, 100));
        assert!(action("stop", true).is_err());
        assert!(action("cleanup", true).is_err());
        assert!(action("reload", true).is_err());
        assert_eq!(action("status", true), Ok(Action::None));
        assert_eq!(action("stop", false), Ok(Action::Stop));
    }
}
