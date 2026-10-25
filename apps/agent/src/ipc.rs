//! Control local autenticado y socket separado de solo lectura.
use crate::platform::LocalIpc;
use crate::platform::local_ipc::{Listener, Stream};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Stop,
    Reload,
    Cleanup,
}

pub fn bind(path: &Path, status_gid: Option<u32>, read_only: bool) -> Result<Listener, String> {
    LocalIpc::bind(path, status_gid, read_only)
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
    stream: Stream,
    socket: &Path,
    read_only: bool,
    status: serde_json::Value,
) -> Action {
    if !read_only {
        let authorized = LocalIpc::peer_uid(&stream)
            .ok()
            .zip(LocalIpc::socket_owner(socket).ok())
            .is_some_and(|(uid, owner)| allowed(uid, owner));
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
