//! Relay: `parse`, `RelayToken::parse` y `FrameDecoder` nunca entran en pánico con entradas
//! arbitrarias; un token aceptado se re-codifica idéntico y un decodificador no acumula más de lo debido.
#![no_main]
use atenea_relay_core::{FrameDecoder, MAX_FRAME, RelayToken, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse(data);
    if let Ok(t) = RelayToken::parse(data) {
        assert_eq!(t.encode().as_slice(), data);
    }
    // El primer byte decide el tamaño de los trozos con que se alimenta el flujo TCP.
    let Some((&chunk, rest)) = data.split_first() else {
        return;
    };
    let mut d = FrameDecoder::new();
    for part in rest.chunks(usize::from(chunk) + 1) {
        d.push(part);
        loop {
            match d.next_frame() {
                Ok(Some(f)) => assert!(!f.is_empty() && f.len() <= MAX_FRAME),
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
});
