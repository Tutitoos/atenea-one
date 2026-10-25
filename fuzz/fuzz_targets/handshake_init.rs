//! `read_init` con clave de respondedor fija sobre bytes arbitrarios: nunca pánico.
#![no_main]
use atenea_crypto::{StaticKeypair, read_init};
use libfuzzer_sys::fuzz_target;

const NET: [u8; 16] = [7; 16];

fuzz_target!(|data: &[u8]| {
    // Primeros 4 bytes: índice externo; el resto, mensaje 1.
    let Some((idx, msg1)) = data.split_first_chunk::<4>() else { return };
    let local = StaticKeypair::from_private(&[0x42; 32]);
    let _ = read_init(&local, &NET, msg1, u32::from_be_bytes(*idx));
});
