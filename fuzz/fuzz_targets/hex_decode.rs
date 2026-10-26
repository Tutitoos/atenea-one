// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! `hex::decode` contra un modelo: acepta ⇔ (tras `trim`) longitud 2N y solo dígitos hex; y ida y vuelta.
#![no_main]
use atenea_crypto::hex::{decode, encode};
use libfuzzer_sys::fuzz_target;

fn check<const N: usize>(s: &str) {
    let t = s.trim();
    let valid = t.len() == N * 2 && t.bytes().all(|b| b.is_ascii_hexdigit());
    match decode::<N>(s) {
        Some(b) => {
            assert!(valid, "aceptó entrada inválida {s:?}");
            assert_eq!(encode(&b), t.to_ascii_lowercase());
        }
        None => assert!(!valid, "rechazó entrada válida {s:?}"),
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    check::<0>(s);
    check::<1>(s);
    check::<4>(s);
    check::<8>(s);
    check::<16>(s);
    check::<32>(s);
    check::<64>(s);
});
