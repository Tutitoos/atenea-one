// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Adaptador SCM: el bucle compartido conserva la responsabilidad de limpiar la red.
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
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
fn service_status(state: ServiceState, exit_code: ServiceExitCode) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code,
        checkpoint: u32::from(state == ServiceState::StopPending),
        wait_hint: if state == ServiceState::StopPending {
            Duration::from_secs(30)
        } else {
            Duration::ZERO
        },
        process_id: None,
    }
}
fn serve() -> Result<(), String> {
    // Serializa STOP con Running/Stopped para no publicar estados fuera de orden.
    let shared = Arc::new(Mutex::new(None::<ServiceStatusHandle>));
    let handler_status = Arc::clone(&shared);
    let status = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            tracing::info!(?control, "SCM solicita parada");
            let slot = match handler_status.lock() {
                Ok(slot) => slot,
                Err(error) => {
                    tracing::error!("SCM estado bloqueado: {error}");
                    return ServiceControlHandlerResult::Other(1);
                }
            };
            if let Some(status) = *slot
                && let Err(error) = status.set_service_status(service_status(
                    ServiceState::StopPending,
                    ServiceExitCode::Win32(0),
                ))
            {
                tracing::error!("SCM StopPending: {error}");
            }
            STOP.send_replace(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(|e| format!("SCM handler: {e}"))?;
    let result = (|| {
        let mut slot = shared.lock().map_err(|_| "estado SCM bloqueado")?;
        *slot = Some(status);
        status
            .set_service_status(service_status(
                ServiceState::Running,
                ServiceExitCode::Win32(0),
            ))
            .map_err(|e| format!("SCM Running: {e}"))?;
        drop(slot);
        tracing::info!("SCM Running; iniciando el agente");
        let config = CONFIG.get().ok_or("configuración SCM ausente")?.clone();
        RUN.get().ok_or("callback SCM ausente")?(config)
    })();
    if let Err(error) = &result {
        tracing::error!("servicio terminado con error: {error}");
    }
    let mut slot = shared.lock().map_err(|_| "estado SCM bloqueado")?;
    status
        .set_service_status(service_status(
            ServiceState::Stopped,
            ServiceExitCode::Win32(if result.is_ok() { 0 } else { 1 }),
        ))
        .map_err(|e| format!("SCM Stopped: {e}"))?;
    *slot = None;
    tracing::info!(
        success = result.is_ok(),
        "SCM Stopped; runtime y limpieza terminados"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_stop_has_bounded_hint_and_no_accepted_controls() {
        let status = service_status(ServiceState::StopPending, ServiceExitCode::Win32(0));
        assert_eq!(status.checkpoint, 1);
        assert_eq!(status.wait_hint, Duration::from_secs(30));
        assert!(status.controls_accepted.is_empty());
        let stopped = service_status(ServiceState::Stopped, ServiceExitCode::Win32(1));
        assert_eq!(stopped.checkpoint, 0);
        assert_eq!(stopped.wait_hint, Duration::ZERO);
    }

    #[tokio::test]
    async fn stop_before_subscription_is_retained() {
        let (tx, _) = tokio::sync::watch::channel(false);
        tx.send_replace(true);
        let rx = tx.subscribe();
        assert!(*rx.borrow());
    }
}
