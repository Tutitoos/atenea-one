// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `DeviceConfig::parse`: nunca pánico y lo aceptado se re-codifica con los mismos bytes
//! (codificación única, contrato §1).
#![no_main]
use atenea_identity::DeviceConfig;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(cfg) = DeviceConfig::parse(data) {
        assert_eq!(cfg.encode().expect("lo parseado es válido"), data);
    }
});
