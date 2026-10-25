//! Pipes locales con DACL desde la creación; ninguna identidad Unix sintética.
use std::path::{Path, PathBuf};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::Mutex;

pub type Stream = NamedPipeServer;
pub struct Listener {
    path: PathBuf,
    group: Option<u32>,
    pending: Mutex<NamedPipeServer>,
}
pub struct LocalIpc;
impl Listener {
    /// La conexión pendiente sobrevive a la cancelación de select!.
    pub async fn accept(&self) -> std::io::Result<(Stream, ())> {
        let mut pending = self.pending.lock().await;
        pending.connect().await?;
        let next = atenea_windows::create_pipe(&self.path, self.group, false)?;
        Ok((std::mem::replace(&mut *pending, next), ()))
    }
}
impl LocalIpc {
    pub fn bind(path: &Path, status_gid: Option<u32>, read_only: bool) -> Result<Listener, String> {
        let group = if read_only { status_gid } else { None };
        let pending = atenea_windows::create_pipe(path, group, true)
            .map_err(|e| format!("IPC {}: {e}", path.display()))?;
        Ok(Listener {
            path: path.into(),
            group,
            pending: Mutex::new(pending),
        })
    }
    pub fn authorized(stream: &Stream) -> bool {
        atenea_windows::authorize_pipe_client(stream).unwrap_or(false)
    }
}
