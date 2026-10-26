//! Formato de datagrama v2 (wire-v2, ADR-003). Big-endian.
//!
//! Init:  `version type sender_index(4) noise_msg1(108) mac1(16) mac2(16)`
//! Resp:  `version type sender_index(4) receiver_index(4) noise_msg2`
//! Datos: `version type receiver_index(4) counter(8) ciphertext+tag(16)`

/// Versión del protocolo; versión o tipo desconocidos se descartan sin responder.
pub const VERSION: u8 = 2;
pub const TYPE_INIT: u8 = 1;
pub const TYPE_RESP: u8 = 2;
pub const TYPE_DATA: u8 = 3;
pub const TYPE_COOKIE: u8 = 4;
pub const MAC_LEN: usize = 16;
pub const INIT_MAC1_OFFSET: usize = 114;
pub const INIT_MAC2_OFFSET: usize = 130;

/// Etiqueta AEAD de ChaChaPoly.
pub const TAG_LEN: usize = 16;
/// Cabecera de datos: versión, tipo, receiver_index y contador.
pub const DATA_HEADER_LEN: usize = 1 + 1 + 4 + 8;
/// Sobrecarga de un datagrama de datos (ADR-003: 30 bytes).
pub const DATA_OVERHEAD: usize = DATA_HEADER_LEN + TAG_LEN;

pub const DEFAULT_MTU: usize = 1400;
pub const MIN_MTU: usize = 1280;
pub const MAX_MTU: usize = 1500;
/// Tamaño máximo de un datagrama aceptado.
pub const MAX_DATAGRAM: usize = MAX_MTU + DATA_OVERHEAD;

/// Payload del mensaje 1: `sender_index(4) timestamp(8)`.
pub const INIT_PAYLOAD_LEN: usize = 12;
/// Payload del mensaje 2: `sender_index(4)`.
pub const RESP_PAYLOAD_LEN: usize = 4;
/// Noise IK mensaje 1: e(32) + s cifrada(32+16) + payload cifrado(12+16).
pub const NOISE_MSG1_LEN: usize = 32 + 32 + TAG_LEN + INIT_PAYLOAD_LEN + TAG_LEN;
/// Noise IK mensaje 2: e(32) + payload cifrado(4+16).
pub const NOISE_MSG2_LEN: usize = 32 + RESP_PAYLOAD_LEN + TAG_LEN;

pub const INIT_LEN: usize = 146;
pub const RESP_LEN: usize = 2 + 4 + 4 + NOISE_MSG2_LEN;
pub const COOKIE_REPLY_LEN: usize = 62;

#[derive(Debug, PartialEq, Eq)]
pub enum Packet<'a> {
    Init {
        sender_index: u32,
        noise: &'a [u8],
        mac1: &'a [u8; 16],
        mac2: &'a [u8; 16],
    },
    Resp {
        sender_index: u32,
        receiver_index: u32,
        noise: &'a [u8],
    },
    Data {
        receiver_index: u32,
        counter: u64,
        ciphertext: &'a [u8],
    },
    CookieReply {
        receiver_index: u32,
        nonce: &'a [u8; 24],
        encrypted_cookie: &'a [u8; 32],
    },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ParseError {
    TooShort,
    TooLarge,
    UnknownVersion,
    UnknownType,
    BadLength,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Parsea un datagrama sin copiar. Estricto: longitudes de handshake exactas.
pub fn parse(buf: &[u8]) -> Result<Packet<'_>, ParseError> {
    if buf.len() > MAX_DATAGRAM {
        return Err(ParseError::TooLarge);
    }
    if buf.len() < 2 {
        return Err(ParseError::TooShort);
    }
    if buf[0] != VERSION {
        return Err(ParseError::UnknownVersion);
    }
    match buf[1] {
        TYPE_INIT => {
            if buf.len() != INIT_LEN {
                return Err(ParseError::BadLength);
            }
            Ok(Packet::Init {
                sender_index: be32(&buf[2..6]),
                noise: &buf[6..INIT_MAC1_OFFSET],
                mac1: buf[INIT_MAC1_OFFSET..INIT_MAC2_OFFSET]
                    .try_into()
                    .expect("longitud comprobada"),
                mac2: buf[INIT_MAC2_OFFSET..]
                    .try_into()
                    .expect("longitud comprobada"),
            })
        }
        TYPE_RESP => {
            if buf.len() != RESP_LEN {
                return Err(ParseError::BadLength);
            }
            Ok(Packet::Resp {
                sender_index: be32(&buf[2..6]),
                receiver_index: be32(&buf[6..10]),
                noise: &buf[10..],
            })
        }
        TYPE_DATA => {
            if buf.len() < DATA_OVERHEAD {
                return Err(ParseError::TooShort);
            }
            let mut c = [0u8; 8];
            c.copy_from_slice(&buf[6..14]);
            Ok(Packet::Data {
                receiver_index: be32(&buf[2..6]),
                counter: u64::from_be_bytes(c),
                ciphertext: &buf[DATA_HEADER_LEN..],
            })
        }
        TYPE_COOKIE => {
            if buf.len() != COOKIE_REPLY_LEN {
                return Err(ParseError::BadLength);
            }
            Ok(Packet::CookieReply {
                receiver_index: be32(&buf[2..6]),
                nonce: buf[6..30].try_into().expect("longitud comprobada"),
                encrypted_cookie: buf[30..62].try_into().expect("longitud comprobada"),
            })
        }
        _ => Err(ParseError::UnknownType),
    }
}

pub fn encode_init(
    sender_index: u32,
    noise: &[u8],
    mac1: &[u8; 16],
    mac2: &[u8; 16],
) -> Result<Vec<u8>, ParseError> {
    if noise.len() != NOISE_MSG1_LEN {
        return Err(ParseError::BadLength);
    }
    let mut v = Vec::with_capacity(INIT_LEN);
    v.extend_from_slice(&[VERSION, TYPE_INIT]);
    v.extend_from_slice(&sender_index.to_be_bytes());
    v.extend_from_slice(noise);
    v.extend_from_slice(mac1);
    v.extend_from_slice(mac2);
    Ok(v)
}

pub fn encode_resp(
    sender_index: u32,
    receiver_index: u32,
    noise: &[u8],
) -> Result<Vec<u8>, ParseError> {
    if noise.len() != NOISE_MSG2_LEN {
        return Err(ParseError::BadLength);
    }
    let mut v = Vec::with_capacity(RESP_LEN);
    v.extend_from_slice(&[VERSION, TYPE_RESP]);
    v.extend_from_slice(&sender_index.to_be_bytes());
    v.extend_from_slice(&receiver_index.to_be_bytes());
    v.extend_from_slice(noise);
    Ok(v)
}

/// Construye la respuesta de cookie; el cifrado pertenece a `atenea-crypto`.
pub fn encode_cookie_reply(
    receiver_index: u32,
    nonce: &[u8; 24],
    encrypted_cookie: &[u8; 32],
) -> [u8; COOKIE_REPLY_LEN] {
    let mut packet = [0; COOKIE_REPLY_LEN];
    packet[..2].copy_from_slice(&[VERSION, TYPE_COOKIE]);
    packet[2..6].copy_from_slice(&receiver_index.to_be_bytes());
    packet[6..30].copy_from_slice(nonce);
    packet[30..].copy_from_slice(encrypted_cookie);
    packet
}

/// Cabecera de datos; el cifrado se añade después.
pub fn data_header(receiver_index: u32, counter: u64) -> [u8; DATA_HEADER_LEN] {
    let mut h = [0u8; DATA_HEADER_LEN];
    h[0] = VERSION;
    h[1] = TYPE_DATA;
    h[2..6].copy_from_slice(&receiver_index.to_be_bytes());
    h[6..14].copy_from_slice(&counter.to_be_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn handshake_lengths_match_noise_ik() {
        assert_eq!(NOISE_MSG1_LEN, 108);
        assert_eq!(NOISE_MSG2_LEN, 52);
        assert_eq!(DATA_OVERHEAD, 30);
    }

    #[test]
    fn all_control_lengths_and_versions_are_strict() {
        assert_eq!(INIT_LEN, 146);
        assert_eq!(RESP_LEN, 62);
        assert_eq!(COOKIE_REPLY_LEN, 62);
        let init = encode_init(1, &[0; 108], &[1; 16], &[2; 16]).unwrap();
        let resp = encode_resp(2, 1, &[0; 52]).unwrap();
        let cookie = encode_cookie_reply(1, &[3; 24], &[4; 32]);
        assert_eq!(
            parse(&cookie),
            Ok(Packet::CookieReply {
                receiver_index: 1,
                nonce: &[3; 24],
                encrypted_cookie: &[4; 32]
            })
        );
        for packet in [&init[..], &resp[..], &cookie[..]] {
            for len in 0..packet.len() {
                assert!(parse(&packet[..len]).is_err());
            }
            let mut extra = packet.to_vec();
            extra.push(0);
            assert_eq!(parse(&extra), Err(ParseError::BadLength));
            for version in [0, 1, 3, 255] {
                let mut bad = packet.to_vec();
                bad[0] = version;
                assert_eq!(parse(&bad), Err(ParseError::UnknownVersion));
            }
        }
        assert_eq!(parse(&[2, 255]), Err(ParseError::UnknownType));
    }

    #[test]
    fn init_roundtrip() {
        let n = [7u8; NOISE_MSG1_LEN];
        let d = encode_init(0xdead_beef, &n, &[1; 16], &[2; 16]).unwrap();
        assert_eq!(
            parse(&d),
            Ok(Packet::Init {
                sender_index: 0xdead_beef,
                noise: &n,
                mac1: &[1; 16],
                mac2: &[2; 16],
            })
        );
    }

    #[test]
    fn resp_roundtrip() {
        let n = [9u8; NOISE_MSG2_LEN];
        let d = encode_resp(1, 2, &n).unwrap();
        assert_eq!(
            parse(&d),
            Ok(Packet::Resp {
                sender_index: 1,
                receiver_index: 2,
                noise: &n
            })
        );
    }

    #[test]
    fn data_roundtrip_and_minimum() {
        let mut d = data_header(5, 42).to_vec();
        d.extend_from_slice(&[0u8; TAG_LEN]);
        assert_eq!(
            parse(&d),
            Ok(Packet::Data {
                receiver_index: 5,
                counter: 42,
                ciphertext: &[0u8; TAG_LEN]
            })
        );
        d.pop();
        assert_eq!(parse(&d), Err(ParseError::TooShort));
    }

    #[test]
    fn rejects_version_type_length_and_oversize() {
        assert_eq!(parse(&[]), Err(ParseError::TooShort));
        assert_eq!(parse(&[1, 3]), Err(ParseError::UnknownVersion));
        assert_eq!(parse(&[2, 9]), Err(ParseError::UnknownType));
        let mut init = encode_init(1, &[0u8; NOISE_MSG1_LEN], &[0; 16], &[0; 16]).unwrap();
        init.push(0);
        assert_eq!(parse(&init), Err(ParseError::BadLength));
        assert_eq!(
            parse(&vec![1u8; MAX_DATAGRAM + 1]),
            Err(ParseError::TooLarge)
        );
        assert!(encode_init(1, &[0u8; 10], &[0; 16], &[0; 16]).is_err());
    }

    proptest! {
        // Entrada arbitraria: nunca entra en pánico y lo aceptado re-codifica igual.
        #[test]
        fn parse_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..1600)) {
            match parse(&buf) {
                Ok(Packet::Init { sender_index, noise, mac1, mac2 }) => {
                    prop_assert_eq!(encode_init(sender_index, noise, mac1, mac2).unwrap(), buf);
                }
                Ok(Packet::Resp { sender_index, receiver_index, noise }) => {
                    prop_assert_eq!(encode_resp(sender_index, receiver_index, noise).unwrap(), buf);
                }
                Ok(Packet::Data { receiver_index, counter, ciphertext }) => {
                    let mut e = data_header(receiver_index, counter).to_vec();
                    e.extend_from_slice(ciphertext);
                    prop_assert_eq!(e, buf);
                }
                Ok(Packet::CookieReply {receiver_index, nonce, encrypted_cookie}) => {
                    let encoded = encode_cookie_reply(receiver_index, nonce, encrypted_cookie);
                    prop_assert_eq!(encoded.as_slice(), buf.as_slice());
                }
                Err(_) => {}
            }
        }
    }
}
