// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! STUN: `parse` y `looks_like_stun` nunca entran en pánico; lo que se acepta como éxito se
//! re-codifica a un mensaje equivalente; los candidatos saneados cumplen siempre las reglas.
#![no_main]
use atenea_nat_traversal::{
    Message, candidate_ok, encode_binding_success, looks_like_stun, parse, sanitize_candidates,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = looks_like_stun(data);
    if let Ok(Message::BindingSuccess { tx, mapped }) = parse(data) {
        let again = encode_binding_success(&tx, mapped);
        assert_eq!(parse(&again), Ok(Message::BindingSuccess { tx, mapped }));
        for c in sanitize_candidates([mapped], 8) {
            assert!(candidate_ok(&c));
        }
    }
});
