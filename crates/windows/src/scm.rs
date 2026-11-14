// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Consulta de servicios con la API del SCM (sin ejecutar `sc.exe`, ADR-017 §1). Solo lectura.
use std::io;
use windows_service::{
    Error,
    service::{ServiceAccess, ServiceState},
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;

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

/// Estado del servicio `name`; `Missing` si no está registrado.
pub fn service_status(name: &str) -> io::Result<ServiceStatus> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(io_error)?;
    let service = match manager.open_service(name, ServiceAccess::QUERY_STATUS) {
        Ok(service) => service,
        Err(Error::Winapi(e)) if e.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST) => {
            return Ok(ServiceStatus::Missing);
        }
        Err(e) => return Err(io_error(e)),
    };
    let state = service.query_status().map_err(io_error)?.current_state;
    Ok(match state {
        ServiceState::Stopped => ServiceStatus::Stopped,
        ServiceState::Running => ServiceStatus::Running,
        _ => ServiceStatus::Transition,
    })
}
