//! Adaptador SCM: el bucle compartido conserva la responsabilidad de limpiar la red.
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

pub const NAME: &str = "AteneaAgent";
static CONFIG: OnceLock<PathBuf> = OnceLock::new();
static RUN: OnceLock<fn(PathBuf) -> Result<(), String>> = OnceLock::new();
static RESULT: Mutex<Option<Result<(), String>>> = Mutex::new(None);
static STOP: LazyLock<tokio::sync::watch::Sender<bool>> =
    LazyLock::new(|| tokio::sync::watch::channel(false).0);
pub fn service_stop_receiver() -> tokio::sync::watch::Receiver<bool> {
    STOP.subscribe()
}
define_windows_service!(ffi_service_main, service_main);

/// Debe invocarse antes de crear el runtime, desde `run --service --config ...`.
pub fn dispatch_service(
    config: PathBuf,
    run: fn(PathBuf) -> Result<(), String>,
) -> Result<(), String> {
    if !config.is_absolute() {
        return Err("el servicio exige una ruta de configuración absoluta".into());
    }
    CONFIG
        .set(config)
        .map_err(|_| "dispatcher ya inicializado")?;
    RUN.set(run).map_err(|_| "callback SCM ya inicializado")?;
    service_dispatcher::start(NAME, ffi_service_main).map_err(|e| format!("SCM: {e}"))?;
    RESULT
        .lock()
        .map_err(|_| "resultado SCM bloqueado")?
        .take()
        .ok_or("SCM terminó sin ejecutar el servicio")?
}
fn service_main(_arguments: Vec<OsString>) {
    let result = serve();
    if let Ok(mut slot) = RESULT.lock() {
        *slot = Some(result);
    }
}
fn serve() -> Result<(), String> {
    let stop = STOP.clone();
    let status = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop.send_replace(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(|e| format!("SCM handler: {e}"))?;
    let report = |state, exit_code| {
        status
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: if state == ServiceState::Running {
                    ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
                } else {
                    ServiceControlAccept::empty()
                },
                exit_code,
                checkpoint: 0,
                wait_hint: Duration::ZERO,
                process_id: None,
            })
            .map_err(|e| format!("SCM estado: {e}"))
    };
    let result = (|| {
        report(ServiceState::Running, ServiceExitCode::Win32(0))?;
        let config = CONFIG.get().ok_or("configuración SCM ausente")?.clone();
        RUN.get().ok_or("callback SCM ausente")?(config)
    })();
    report(
        ServiceState::Stopped,
        ServiceExitCode::Win32(if result.is_ok() { 0 } else { 1 }),
    )?;
    result
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn stop_before_subscription_is_retained() {
        let (tx, _) = tokio::sync::watch::channel(false);
        tx.send_replace(true);
        let rx = tx.subscribe();
        assert!(*rx.borrow());
    }
}
