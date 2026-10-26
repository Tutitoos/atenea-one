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
