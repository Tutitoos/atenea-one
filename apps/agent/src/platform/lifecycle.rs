//! Solicitud de parada del host; la limpieza pertenece al bucle compartido.

use std::io;
#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};

#[cfg(unix)]
pub struct Lifecycle {
    term: Signal,
    int: Signal,
}

#[cfg(not(unix))]
pub enum Lifecycle {}

impl Lifecycle {
    /// Registra las señales antes de entrar en el bucle de E/S.
    pub fn new() -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                term: signal(SignalKind::terminate())?,
                int: signal(SignalKind::interrupt())?,
            })
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "ciclo de vida: backend de {} pendiente de implementación",
                    std::env::consts::OS,
                ),
            ))
        }
    }

    /// Espera SIGTERM o SIGINT en Unix; se puede cancelar dentro de `select!`.
    pub async fn wait(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.term.recv() => {},
            _ = self.int.recv() => {},
        }
        #[cfg(not(unix))]
        match *self {}
    }
}

#[cfg(all(test, not(unix)))]
mod tests {
    use super::Lifecycle;
    use std::io;

    #[test]
    fn unsupported_platform_has_no_lifecycle() {
        let err = Lifecycle::new()
            .err()
            .expect("sin backend de ciclo de vida");
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert!(err.to_string().contains(std::env::consts::OS));
    }
}
