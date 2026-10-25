//! Datagrama DATA v2 arbitrario contra sesión establecida; antirreplay sin cambios.
//! Nunca pánico; un fallo no avanza la ventana (un paquete válido posterior sigue aceptándose).
#![no_main]
use atenea_crypto::{Error, Session, StaticKeypair, initiate, read_init};
use atenea_protocol::{Packet, parse};
use libfuzzer_sys::fuzz_target;
use std::time::Instant;

const NET: [u8; 16] = [7; 16];

fn pair(now: Instant) -> (Session, Session) {
    let a = StaticKeypair::from_private(&[0x11; 32]);
    let b = StaticKeypair::from_private(&[0x22; 32]);
    let (mut pending, msg1) = initiate(&a, &b.public, &NET, 1, 1000).unwrap();
    let (sb, msg2) = read_init(&b, &NET, &msg1, 1).unwrap().accept(2, now).unwrap();
    (pending.finish(&msg2, 2, now).unwrap(), sb)
}

fn valid(a: &mut Session, now: Instant, payload: &[u8]) -> (u64, Vec<u8>) {
    match parse(&a.encrypt(payload, now).unwrap()) {
        Ok(Packet::Data { counter, ciphertext, .. }) => (counter, ciphertext.to_vec()),
        _ => unreachable!(),
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(Packet::Data {counter, ciphertext: ct, ..}) = parse(data) else { return };
    let now = Instant::now();
    let (mut a, mut b) = pair(now);
    // Tres válidos: 0, 1, 2 (el 0 también sirve de objetivo de colisión con el contador arbitrario).
    let ok: Vec<_> = (0u8..3).map(|i| valid(&mut a, now, &[i; 5])).collect();

    match b.decrypt(counter, ct, now) {
        Ok(_) => return, // forjar un AEAD válido es inviable; si ocurre, no hay invariante que comprobar
        Err(Error::AuthFailed | Error::Replay | Error::Expired | Error::TooLarge) => {}
        Err(e) => panic!("error inesperado: {e}"),
    }
    // El fallo no debe haber avanzado la ventana ni confirmado la sesión.
    assert!(!b.is_confirmed(), "un fallo confirmó la sesión");
    for (i, (c, ct)) in ok.iter().enumerate() {
        assert_eq!(b.decrypt(*c, ct, now).as_deref(), Ok(&[i as u8; 5][..]), "válido {c} rechazado tras fallo");
    }
    assert!(b.is_confirmed());
});
