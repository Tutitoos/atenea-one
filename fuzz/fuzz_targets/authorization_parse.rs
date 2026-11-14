// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `parse_authorization` (cabecera `Authorization: AtDevice v1 …`, control-plane-v1 §3.3) con texto UTF-8
//! arbitrario: nunca pánico; lo aceptado tiene una única representación canónica (campos exactos y en orden,
//! `ts` sin ceros a la izquierda ni signo, hex de longitud exacta) y, salvo las mayúsculas del hex, el texto de
//! entrada ES esa representación. Lo canónico vuelve a parsearse al mismo valor.
#![no_main]
use atenea_identity::api_auth::parse_authorization;
use libfuzzer_sys::fuzz_target;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(request) = parse_authorization(text) else {
        return;
    };
    let canonical = format!(
        "AtDevice v1 device={}, ts={}, nonce={}, sig={}",
        hex(&request.device_id),
        request.ts,
        hex(&request.nonce),
        hex(&request.sig)
    );
    assert_eq!(text.to_ascii_lowercase(), canonical.to_ascii_lowercase(), "dos textos distintos para la misma petición");
    assert_eq!(parse_authorization(&canonical).expect("canónico"), request);
});
