// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `JoinCode::decode` (join-code-v1): nunca pánico con texto UTF-8 arbitrario; lo aceptado cumple las
//! reglas de longitud, se re-codifica y vuelve a decodificar al mismo valor (codificación canónica).
#![no_main]
use atenea_identity::join_code::JoinCode;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(code) = JoinCode::decode(text) {
        assert!(text.len() <= 512);
        let canonical = code.encode().expect("lo decodificado es válido");
        assert_eq!(JoinCode::decode(&canonical).expect("canónico"), code);
    }
});
