// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Parsers de `IssuerCert`, `DeviceBundle` y `AgentState`: nunca pánico; lo aceptado se re-codifica idéntico.
#![no_main]
use atenea_identity::{AgentState, DeviceBundle, IssuerCert};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(c) = IssuerCert::parse(data) {
        assert_eq!(c.encode().as_slice(), data);
    }
    if let Ok(b) = DeviceBundle::parse(data) {
        assert_eq!(b.encode().as_slice(), data);
        let _ = b.verify();
        let _ = b.fingerprint();
    }
    if let Ok(s) = AgentState::from_bytes(data) {
        assert_eq!(s.to_bytes(), data);
    }
});
