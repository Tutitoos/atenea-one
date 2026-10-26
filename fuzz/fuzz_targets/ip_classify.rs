// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `classify` y `Policy::allows` sobre paquetes arbitrarios: nunca pánico; una política sin reglas deniega.
#![no_main]
use atenea_identity::{DeviceConfig, Prefix};
use atenea_policies::{Policy, classify};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(flow) = classify(data) else { return };
    let cfg = DeviceConfig {
        network_id: [0; 16],
        device_id: [1; 16],
        issuer_serial: 1,
        config_version: 1,
        not_before: 0,
        not_after: 1,
        noise_pubkey: [0; 32],
        identity_pubkey: [0; 32],
        mtu: 1400,
        tunnel_addrs: vec!["10.0.0.1".parse().unwrap()],
        network_prefixes: vec![Prefix { addr: "10.0.0.0".parse().unwrap(), len: 24 }],
        peers: vec![],
        rules: vec![],
        advertised: vec![],
        dns: Default::default(),
    };
    assert!(!Policy::new(&cfg).allows(&flow));
});
