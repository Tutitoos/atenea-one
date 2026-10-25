//! Solicitud de parada del host; la limpieza pertenece al bucle compartido.

use std::io;
#[cfg(unix)]
use tokio::signal::unix::{Signal, SignalKind, signal};

#[cfg(unix)]
pub struct Lifecycle {
    term: Signal,
    int: Signal,
}

#[cfg(windows)]
pub struct Lifecycle {
    interrupt: tokio::signal::windows::CtrlC,
    close: tokio::signal::windows::CtrlClose,
    shutdown: tokio::signal::windows::CtrlShutdown,
    stop: tokio::sync::watch::Receiver<bool>,
}

#[cfg(not(any(unix, windows)))]
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
        #[cfg(windows)]
        {
            Ok(Self {
                interrupt: tokio::signal::windows::ctrl_c()?,
                close: tokio::signal::windows::ctrl_close()?,
                shutdown: tokio::signal::windows::ctrl_shutdown()?,
                stop: super::service::stop_receiver(),
            })
        }
        #[cfg(not(any(unix, windows)))]
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
        #[cfg(windows)]
        {
            if *self.stop.borrow_and_update() {
                return;
            }
            tokio::select! {
                _ = self.interrupt.recv() => {},
                _ = self.close.recv() => {},
                _ = self.shutdown.recv() => {},
                _ = self.stop.changed() => {},
            }
        }
        #[cfg(not(any(unix, windows)))]
        match *self {}
    }
}

#[cfg(all(test, not(any(unix, windows))))]
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
