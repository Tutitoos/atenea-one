//! Ventana antirreplay deslizante de 2048 contadores (ADR-003).
//!
//! Uso obligatorio: `check` (solo lectura) → descifrar y autenticar → `update`.
//! Un paquete que no autentica nunca debe llegar a `update`.

pub const WINDOW: u64 = 2048;
const WORDS: usize = (WINDOW / 64) as usize;

#[derive(Debug, Default)]
pub struct ReplayWindow {
    top: u64,
    any: bool,
    bits: [u64; WORDS],
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, c: u64) -> bool {
        let i = (c % WINDOW) as usize;
        self.bits[i / 64] >> (i % 64) & 1 == 1
    }

    fn set(&mut self, c: u64, v: bool) {
        let i = (c % WINDOW) as usize;
        if v {
            self.bits[i / 64] |= 1 << (i % 64);
        } else {
            self.bits[i / 64] &= !(1 << (i % 64));
        }
    }

    /// `true` si el contador podría aceptarse (ni visto ni fuera de ventana).
    pub fn check(&self, c: u64) -> bool {
        if !self.any || c > self.top {
            return true;
        }
        self.top - c < WINDOW && !self.get(c)
    }

    /// Marca el contador como visto. Devuelve `false` si es replay o está fuera de ventana.
    pub fn update(&mut self, c: u64) -> bool {
        if !self.check(c) {
            return false;
        }
        if !self.any {
            self.any = true;
            self.top = c;
            self.bits = [0; WORDS];
        } else if c > self.top {
            if c - self.top >= WINDOW {
                self.bits = [0; WORDS];
            } else {
                for x in self.top + 1..=c {
                    self.set(x, false);
                }
            }
            self.top = c;
        }
        self.set(c, true);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    #[test]
    fn basic_replay_and_window() {
        let mut w = ReplayWindow::new();
        assert!(w.update(0));
        assert!(!w.update(0));
        assert!(w.update(5));
        assert!(w.update(3)); // desorden dentro de ventana
        assert!(!w.update(3));
        assert!(w.update(5000));
        assert!(!w.update(5)); // fuera de ventana
        assert!(!w.update(5000 - WINDOW)); // justo fuera
        assert!(w.update(5000 - WINDOW + 1)); // borde dentro
    }

    #[test]
    fn check_does_not_mutate() {
        let mut w = ReplayWindow::new();
        assert!(w.check(10));
        assert!(w.check(10));
        assert!(w.update(10));
        assert!(!w.check(10));
    }

    proptest! {
        // Contra un modelo exacto: aceptado ⇔ no visto y dentro de ventana.
        #[test]
        fn matches_reference_model(ops in proptest::collection::vec(0u64..6000, 1..400)) {
            let mut w = ReplayWindow::new();
            let mut seen = BTreeSet::new();
            let mut max: Option<u64> = None;
            for c in ops {
                let expect = match max {
                    None => true,
                    Some(m) => c > m || (m - c < WINDOW && !seen.contains(&c)),
                };
                prop_assert_eq!(w.check(c), expect);
                prop_assert_eq!(w.update(c), expect);
                if expect {
                    seen.insert(c);
                    max = Some(max.map_or(c, |m| m.max(c)));
                }
            }
        }
    }
}
