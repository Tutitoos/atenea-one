//! Secuencia arbitraria de contadores contra un modelo exacto (aceptado ⇔ no visto y dentro de ventana).
#![no_main]
use arbitrary::Unstructured;
use atenea_crypto::replay::{ReplayWindow, WINDOW};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let mut w = ReplayWindow::new();
    let mut seen = BTreeSet::new();
    let mut max: Option<u64> = None;
    // Cada operación: modo + desplazamiento; cubre borde de ventana, saltos grandes y u64::MAX.
    // `arbitrary` devuelve ceros al agotarse la entrada: hay que parar explícitamente.
    while !u.is_empty() {
        let Ok((mode, off)) = u.arbitrary::<(u8, u16)>() else { break };
        let off = u64::from(off);
        let m = max.unwrap_or(0);
        let c = match mode % 6 {
            0 => m.saturating_add(off),
            1 => m.saturating_sub(off % (WINDOW + 8)),
            2 => off,
            3 => u64::MAX - off,
            4 => m.saturating_add(WINDOW - 4 + off % 8),
            _ => m.saturating_sub(WINDOW - 4 + off % 8),
        };
        let expect = match max {
            None => true,
            Some(m) => c > m || (m - c < WINDOW && !seen.contains(&c)),
        };
        assert_eq!(w.check(c), expect, "check({c}) con max={max:?}");
        assert_eq!(w.check(c), expect, "check debe ser idempotente");
        assert_eq!(w.update(c), expect, "update({c}) con max={max:?}");
        if expect {
            seen.insert(c);
            max = Some(max.map_or(c, |m| m.max(c)));
        }
        // Tras aceptar, el contador queda marcado como visto.
        assert!(!w.check(c));
    }
});
