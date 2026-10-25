//! Transporte Unix e identidad del par; la política de control vive en `ipc`.
#[cfg(windows)]
#[path = "windows_ipc.rs"]
mod windows;
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    pub type Stream = tokio::net::UnixStream;
    pub type Listener = tokio::net::UnixListener;

    pub struct LocalIpc;

    impl LocalIpc {
        pub fn bind(
            path: &Path,
            status_gid: Option<u32>,
            read_only: bool,
        ) -> Result<Listener, String> {
            if path.exists() {
                if Self::connect(path).is_ok() {
                    return Err(format!("{}: ya hay un agente activo", path.display()));
                }
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
            }
            let listener =
                Listener::bind(path).map_err(|e| format!("IPC {}: {e}", path.display()))?;
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

        pub fn connect(path: &Path) -> Result<std::os::unix::net::UnixStream, String> {
            std::os::unix::net::UnixStream::connect(path)
                .map_err(|e| format!("IPC {}: {e}", path.display()))
        }

        pub fn peer_uid(stream: &Stream) -> Result<u32, String> {
            stream
                .peer_cred()
                .map(|cred| cred.uid())
                .map_err(|e| e.to_string())
        }

        pub fn socket_owner(path: &Path) -> Result<u32, String> {
            std::fs::metadata(path)
                .map(|meta| meta.uid())
                .map_err(|e| e.to_string())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn sockets_keep_permissions_and_peer_identity() {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("atenea-ipc-{}-{unique}", std::process::id()));
            let listener = LocalIpc::bind(&path, None, false).unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let client = LocalIpc::connect(&path).unwrap();
            let (server, _) = listener.accept().await.unwrap();
            assert_eq!(
                LocalIpc::peer_uid(&server).unwrap(),
                LocalIpc::socket_owner(&path).unwrap()
            );
            assert!(LocalIpc::bind(&path, None, false).is_err());
            drop((client, server, listener));
            std::fs::remove_file(&path).unwrap();
            let listener = LocalIpc::bind(&path, None, true).unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o660
            );
            drop(listener);
            std::fs::remove_file(path).unwrap();
        }
    }
}
