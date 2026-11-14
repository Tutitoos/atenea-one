// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Servicios del SCM con la API de `windows-service` (sin ejecutar `sc.exe`, ADR-017 §1). Sin `unsafe` aquí.
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows_service::{
    Error,
    service::{
        Service, ServiceAccess, ServiceAction, ServiceActionType, ServiceErrorControl,
        ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType,
        ServiceState, ServiceType,
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const WAIT: Duration = Duration::from_secs(40);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceStatus {
    Missing,
    Stopped,
    Running,
    /// Arrancando, parándose o en pausa.
    Transition,
}

fn io_error(error: Error) -> io::Error {
    match error {
        Error::Winapi(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

fn does_not_exist(error: &Error) -> bool {
    matches!(error, Error::Winapi(e) if e.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST))
}

/// Estado del servicio `name`; `Missing` si no está registrado.
pub fn service_status(name: &str) -> io::Result<ServiceStatus> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(io_error)?;
    let service = match manager.open_service(name, ServiceAccess::QUERY_STATUS) {
        Ok(service) => service,
        Err(e) if does_not_exist(&e) => return Ok(ServiceStatus::Missing),
        Err(e) => return Err(io_error(e)),
    };
    Ok(
        match service.query_status().map_err(io_error)?.current_state {
            ServiceState::Stopped => ServiceStatus::Stopped,
            ServiceState::Running => ServiceStatus::Running,
            _ => ServiceStatus::Transition,
        },
    )
}

/// Un servicio de una red (`AteneaNetwork-<id8>`): LocalSystem, mismo binario que el heredado.
pub struct NetworkService {
    pub name: String,
    pub display_name: String,
    /// Ruta absoluta y ya comprobada como no sustituible por usuarios sin privilegios.
    pub executable: PathBuf,
    /// Ruta absoluta del `agent.toml` de la red.
    pub config: PathBuf,
}

fn quote(text: &str) -> String {
    if text.contains([' ', '\t']) {
        format!("\"{text}\"")
    } else {
        text.to_owned()
    }
}

impl NetworkService {
    fn validate(&self) -> io::Result<()> {
        let bad =
            |what: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("{what} inválida"));
        for (what, path) in [
            ("ruta del binario", &self.executable),
            ("ruta de configuración", &self.config),
        ] {
            let text = path.to_str().ok_or_else(|| bad(what))?;
            if !path.is_absolute() || text.contains(['"', '\0']) || text.ends_with('\\') {
                return Err(bad(what));
            }
        }
        Ok(())
    }

    /// Línea de órdenes registrada en el SCM: la misma que construye `windows-service` (CommandLineToArgvW).
    fn command_line(&self) -> String {
        format!(
            "{} run --service --config {}",
            quote(&self.executable.to_string_lossy()),
            quote(&self.config.to_string_lossy())
        )
    }

    fn info(&self, start_type: ServiceStartType) -> ServiceInfo {
        ServiceInfo {
            name: self.name.clone().into(),
            display_name: self.display_name.clone().into(),
            service_type: ServiceType::OWN_PROCESS,
            start_type,
            error_control: ServiceErrorControl::Normal,
            executable_path: self.executable.clone(),
            launch_arguments: vec![
                "run".into(),
                "--service".into(),
                "--config".into(),
                self.config.clone().into(),
            ],
            dependencies: Vec::new(),
            account_name: None,
            account_password: None,
        }
    }

    /// Nunca se reutiliza ni se modifica un servicio con ese nombre que apunte a otra cosa.
    fn verify(&self, service: &Service) -> io::Result<()> {
        let registered = service.query_config().map_err(io_error)?;
        let registered = registered.executable_path.to_string_lossy().into_owned();
        if !registered.eq_ignore_ascii_case(&self.command_line()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "el servicio {} ya existe con otra línea de órdenes ({registered}); no se modifica",
                    self.name
                ),
            ));
        }
        Ok(())
    }
}

const ACCESS: ServiceAccess = ServiceAccess::QUERY_CONFIG
    .union(ServiceAccess::CHANGE_CONFIG)
    .union(ServiceAccess::QUERY_STATUS)
    .union(ServiceAccess::START)
    .union(ServiceAccess::STOP);

fn wait_for(service: &Service, wanted: ServiceState, what: &str) -> io::Result<()> {
    let deadline = Instant::now() + WAIT;
    loop {
        let status = service.query_status().map_err(io_error)?;
        if status.current_state == wanted {
            return Ok(());
        }
        // Un servicio que se detiene mientras arranca no va a llegar a Running.
        if wanted == ServiceState::Running && status.current_state == ServiceState::Stopped {
            return Err(io::Error::other(format!(
                "el servicio se detuvo al arrancar (código {:?}); revisa agent.log de la red",
                status.exit_code
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("el servicio no llegó a {what} en {}s", WAIT.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `up`: registra el servicio si no existe, lo deja en arranque automático retrasado con reinicio tras fallo
/// (5, 15 y 60 s, también ante salidas con error) y lo inicia.
pub fn start_network_service(spec: &NetworkService) -> io::Result<()> {
    spec.validate()?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(io_error)?;
    let service = match manager.open_service(&spec.name, ACCESS) {
        Ok(service) => {
            spec.verify(&service)?;
            service
        }
        Err(e) if does_not_exist(&e) => manager
            .create_service(&spec.info(ServiceStartType::AutoStart), ACCESS)
            .map_err(io_error)?,
        Err(e) => return Err(io_error(e)),
    };
    // Reactiva un servicio que `down` dejó deshabilitado; en uno nuevo no cambia nada.
    service
        .change_config(&spec.info(ServiceStartType::AutoStart))
        .map_err(io_error)?;
    service.set_delayed_auto_start(true).map_err(io_error)?;
    service
        .set_description("Cliente Atenea: túnel cifrado, rutas firmadas y DNS dividido de una red.")
        .map_err(io_error)?;
    let restart = |seconds| ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(seconds),
    };
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![restart(5), restart(15), restart(60)]),
        })
        .map_err(io_error)?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(io_error)?;
    match service.query_status().map_err(io_error)?.current_state {
        ServiceState::Running => Ok(()),
        ServiceState::StartPending => wait_for(&service, ServiceState::Running, "estar en marcha"),
        _ => {
            service.start::<&str>(&[]).map_err(io_error)?;
            wait_for(&service, ServiceState::Running, "estar en marcha")
        }
    }
}

/// `down`: para el servicio, espera a que el agente limpie y lo deshabilita. `false` si no existe.
pub fn stop_network_service(spec: &NetworkService) -> io::Result<bool> {
    spec.validate()?;
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(io_error)?;
    let service = match manager.open_service(&spec.name, ACCESS) {
        Ok(service) => service,
        Err(e) if does_not_exist(&e) => return Ok(false),
        Err(e) => return Err(io_error(e)),
    };
    spec.verify(&service)?;
    // Antes de parar: una salida con error durante la retirada no debe provocar un reinicio que compita con la limpieza.
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(0)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![ServiceAction {
                action_type: ServiceActionType::None,
                delay: Duration::ZERO,
            }]),
        })
        .map_err(io_error)?;
    if service.query_status().map_err(io_error)?.current_state != ServiceState::Stopped {
        // Si ya se está parando, `stop` devuelve error y basta con esperar.
        let _ = service.stop();
        wait_for(&service, ServiceState::Stopped, "detenerse")?;
    }
    service
        .change_config(&spec.info(ServiceStartType::Disabled))
        .map_err(io_error)?;
    Ok(true)
}
