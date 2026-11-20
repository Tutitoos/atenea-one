// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Disparador del SCM «llega la primera dirección IP» para los servicios de red (ADR-033). `windows-service` no
//! expone disparadores: una sola llamada FFI a `ChangeServiceConfig2W`.
use std::io;
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, NETWORK_MANAGER_FIRST_IP_ADDRESS_ARRIVAL_GUID, SC_HANDLE,
    SERVICE_CONFIG_TRIGGER_INFO, SERVICE_TRIGGER, SERVICE_TRIGGER_ACTION_SERVICE_START,
    SERVICE_TRIGGER_INFO, SERVICE_TRIGGER_TYPE_IP_ADDRESS_AVAILABILITY,
};

/// Sustituye los disparadores del servicio por uno solo: iniciarlo cuando llega la primera dirección IP tras el
/// arranque del equipo. `service` debe ser un manejador abierto con `SERVICE_CHANGE_CONFIG`.
pub fn start_on_first_ip(service: SC_HANDLE) -> io::Result<()> {
    let mut subtype = NETWORK_MANAGER_FIRST_IP_ADDRESS_ARRIVAL_GUID;
    let mut trigger = SERVICE_TRIGGER {
        dwTriggerType: SERVICE_TRIGGER_TYPE_IP_ADDRESS_AVAILABILITY,
        dwAction: SERVICE_TRIGGER_ACTION_SERVICE_START,
        pTriggerSubtype: &mut subtype,
        cDataItems: 0,
        pDataItems: std::ptr::null_mut(),
    };
    let info = SERVICE_TRIGGER_INFO {
        cTriggers: 1,
        pTriggers: &mut trigger,
        pReserved: std::ptr::null_mut(),
    };
    // SAFETY: `info`, `trigger` y `subtype` son locales y siguen vivos durante la llamada, que solo los lee y no
    // guarda punteros; `service` es un manejador abierto del SCM que el llamador conserva.
    let ok = unsafe {
        ChangeServiceConfig2W(
            service,
            SERVICE_CONFIG_TRIGGER_INFO,
            (&raw const info).cast(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
