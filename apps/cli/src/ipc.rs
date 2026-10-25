//! Conexión IPC local; el protocolo y los tiempos de espera viven en el cliente.
use std::path::Path;

pub struct LocalIpc;

impl LocalIpc {
    #[cfg(unix)]
    pub fn connect(path: &Path) -> Result<std::os::unix::net::UnixStream, String> {
        std::os::unix::net::UnixStream::connect(path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    #[cfg(windows)]
    pub fn connect(path: &Path) -> Result<std::fs::File, String> {
        atenea_windows::connect_pipe(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    #[cfg(not(any(unix, windows)))]
    pub fn unsupported() -> String {
        "IPC local no soportado: backend de canalizaciones con nombre pendiente".into()
    }
}
