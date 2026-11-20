// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Primitivas Windows compartidas; el agente y la CLI no contienen FFI.
#![cfg(windows)]
// SAFETY: buffers ABI alineados y válidos durante cada llamada; Handle/Local liberan los recursos.
#[allow(unsafe_code)]
mod security;
pub use security::*;
// SAFETY: el callback SCM conserva configuración y parada en estado estático sincronizado.
#[allow(unsafe_code)]
mod service;
pub use service::{dispatch_service, service_stop_receiver};
// SAFETY: una sola llamada FFI con estructuras locales vivas durante la llamada (ADR-033).
#[allow(unsafe_code)]
mod trigger;
// La API del SCM de `windows-service` es segura: sin `unsafe` en este módulo.
mod scm;
pub use scm::{
    NetworkService, ServiceStatus, delete_network_service, service_status, start_network_service,
    stop_network_service,
};
