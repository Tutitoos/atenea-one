// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Configuración v2 y dominios DNS: parseo estricto y representación única.
#![no_main]
use atenea_identity::{DeviceConfig, valid_domain};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(config) = DeviceConfig::parse(data) {
        assert_eq!(config.encode().unwrap(), data);
        assert!(config.dns.domains.iter().all(|s| valid_domain(s)));
    }
    if let Ok(domain) = std::str::from_utf8(data) {
        if valid_domain(domain) {
            assert!(domain.is_ascii());
            assert!(!domain.ends_with('.'));
            assert!(!domain.contains(".."));
        }
    }
});
