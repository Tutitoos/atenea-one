//! Pipes locales con DACL desde la creación; ninguna identidad Unix sintética.
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::Mutex;

pub type Stream = NamedPipeServer;
/// Un nombre ocupado (ERROR_ACCESS_DENIED / ERROR_PIPE_BUSY) suele ser un reinicio rápido: se reintenta poco tiempo.
const BIND_ATTEMPTS: u32 = 10;
const BIND_RETRY: Duration = Duration::from_millis(200);

pub struct Listener {
    path: PathBuf,
    group: Option<u32>,
    /// `None` si no hubo instancia libre tras la última conexión: se recrea en el siguiente `accept`.
    pending: Mutex<Option<NamedPipeServer>>,
}
pub struct LocalIpc;
impl Listener {
    /// La conexión pendiente sobrevive a la cancelación de select!.
    pub async fn accept(&self) -> std::io::Result<(Stream, ())> {
        let mut pending = self.pending.lock().await;
        if pending.is_none() {
            *pending = Some(atenea_windows::create_pipe(&self.path, self.group, false)?);
        }
        let server = pending.as_ref().expect("instancia creada arriba");
        server.connect().await?;
        // Con las instancias agotadas se entrega igualmente al cliente ya conectado.
        let next = atenea_windows::create_pipe(&self.path, self.group, false).ok();
        Ok((
            std::mem::replace(&mut *pending, next).expect("conectada"),
            (),
        ))
    }
}
impl LocalIpc {
    pub fn bind(path: &Path, status_gid: Option<u32>, read_only: bool) -> Result<Listener, String> {
        let group = if read_only { status_gid } else { None };
        let mut attempt = 1;
        let pending = loop {
            match atenea_windows::create_pipe(path, group, true) {
                Ok(pipe) => break pipe,
                Err(e) if attempt < BIND_ATTEMPTS => {
                    tracing::warn!(
                        "IPC {} ocupado ({e}); reintento {attempt}/{BIND_ATTEMPTS}",
                        path.display()
                    );
                    attempt += 1;
                    std::thread::sleep(BIND_RETRY);
                }
                Err(e) => {
                    return Err(format!(
                        "IPC {}: {e}; otro proceso ocupa el nombre del pipe",
                        path.display()
                    ));
                }
            }
        };
        Ok(Listener {
            path: path.into(),
            group,
            pending: Mutex::new(Some(pending)),
        })
    }
    pub fn authorized(stream: &Stream) -> bool {
        atenea_windows::authorize_pipe_client(stream).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bind_retries_while_the_name_is_busy_and_then_gives_up() {
        let path = PathBuf::from(format!(r"\\.\pipe\atenea-test-{}", std::process::id()));
        let busy = atenea_windows::create_pipe(&path, None, true).unwrap();
        let freed = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            drop(busy);
        });
        assert!(LocalIpc::bind(&path, None, false).is_ok());
        freed.join().unwrap();
        let _held = atenea_windows::create_pipe(&path, None, true);
        assert!(LocalIpc::bind(&path, None, false).is_err());
    }
}
