// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Hexadecimal mínimo para claves e identificadores.

pub fn encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Decodifica exactamente `N` bytes; `None` si la longitud o los dígitos no son válidos.
pub fn decode<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.trim();
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_rejects() {
        let b = [0u8, 1, 0xab, 0xff];
        assert_eq!(decode::<4>(&encode(&b)), Some(b));
        assert_eq!(decode::<4>("0001abf"), None);
        assert_eq!(decode::<4>("0001abzz"), None);
        assert_eq!(decode::<2>("0001abff"), None);
        assert_eq!(decode::<4>("00+1abff"), None);
    }
}
