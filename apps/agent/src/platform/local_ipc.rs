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
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::Path;

    pub type Stream = tokio::net::UnixStream;
    pub type Listener = tokio::net::UnixListener;

    pub struct LocalIpc;

    impl LocalIpc {
        /// Crea el socket en un directorio privado recién creado junto al destino (sin `umask` ni
        /// `unsafe`), fija modo/grupo allí y lo renombra atómicamente; luego verifica el resultado.
        /// El directorio padre debe ser de root o del proceso y no escribible por grupo/otros.
        pub fn bind(
            path: &Path,
            status_gid: Option<u32>,
            read_only: bool,
        ) -> Result<Listener, String> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let scratch = parent.join(format!(".ipc{:x}{nanos:x}", std::process::id()));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&scratch)
                .map_err(|e| format!("IPC {}: {e}", parent.display()))?;
            let result = Self::bind_in(parent, &scratch, path, status_gid, read_only);
            let _ = std::fs::remove_dir(&scratch);
            result
        }

        fn bind_in(
            parent: &Path,
            scratch: &Path,
            path: &Path,
            status_gid: Option<u32>,
            read_only: bool,
        ) -> Result<Listener, String> {
            let me = std::fs::symlink_metadata(scratch)
                .map_err(|e| e.to_string())?
                .uid();
            let dir = std::fs::metadata(parent).map_err(|e| e.to_string())?;
            if !dir.is_dir() || !(dir.uid() == 0 || dir.uid() == me) || dir.mode() & 0o022 != 0 {
                return Err(format!(
                    "IPC {}: directorio con propietario o permisos inseguros",
                    parent.display()
                ));
            }
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_socket() => {
                    if Self::connect(path).is_ok() {
                        return Err(format!("{}: ya hay un agente activo", path.display()));
                    }
                }
                Ok(_) => {
                    return Err(format!("{}: existe y no es un socket", path.display()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
            }
            let tmp = scratch.join("s");
            let mode = if read_only { 0o660 } else { 0o600 };
            let listener =
                Listener::bind(&tmp).map_err(|e| format!("IPC {}: {e}", path.display()))?;
            let prepared = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))
                .and_then(|()| match status_gid {
                    Some(gid) if read_only => std::os::unix::fs::chown(&tmp, None, Some(gid)),
                    _ => Ok(()),
                })
                .and_then(|()| std::fs::rename(&tmp, path));
            if let Err(e) = prepared {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("IPC {}: {e}", path.display()));
            }
            let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            let gid_ok = !read_only || status_gid.is_none_or(|gid| meta.gid() == gid);
            if !meta.file_type().is_socket()
                || meta.uid() != me
                || meta.mode() & 0o777 != mode
                || !gid_ok
            {
                let _ = std::fs::remove_file(path);
                return Err(format!(
                    "IPC {}: el socket creado no es el esperado",
                    path.display()
                ));
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
        use std::path::PathBuf;

        fn private_dir(tag: &str) -> PathBuf {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = PathBuf::from("/tmp")
                .join(format!("atenea-ipc-{tag}-{}-{unique}", std::process::id()));
            std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
            dir
        }

        #[tokio::test]
        async fn sockets_keep_permissions_and_peer_identity() {
            let dir = private_dir("ok");
            let path = dir.join("agent.sock");
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
            // Un fork paralelo puede heredar el fd y mantener el socket vivo hasta exec,
            // cuando CLOEXEC lo cierra; después se puede reemplazar el socket huérfano.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let listener = loop {
                match LocalIpc::bind(&path, None, true) {
                    Err(error)
                        if error.contains("ya hay un agente activo")
                            && std::time::Instant::now() < deadline =>
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    result => break result.unwrap(),
                }
            };
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o660
            );
            drop(listener);
            // No quedan directorios temporales.
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[tokio::test]
        async fn refuses_symlinks_files_and_shared_parents() {
            let dir = private_dir("bad");
            let victim = dir.join("victim");
            std::fs::write(&victim, b"intacto").unwrap();
            std::os::unix::fs::symlink(&victim, dir.join("link.sock")).unwrap();
            assert!(LocalIpc::bind(&dir.join("link.sock"), None, false).is_err());
            assert!(LocalIpc::bind(&victim, None, false).is_err());
            assert_eq!(std::fs::read(&victim).unwrap(), b"intacto");
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o770)).unwrap();
            assert!(LocalIpc::bind(&dir.join("agent.sock"), None, false).is_err());
            assert!(!dir.join("agent.sock").exists());
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
