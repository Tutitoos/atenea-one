//! Protocolo del relay v1 (docs/protocol/relay-v1.md). Sin E/S: solo formatos y comprobaciones.
//!
//! El relay reenvía datagramas de Atenea YA cifrados de extremo a extremo (ADR-001/003); nunca ve
//! claves ni texto en claro. Esta crate fija: mensajes UDP/TCP, el token de asignación que emite el
//! coordinator, la prueba de posesión del dispositivo, las cookies anti-spoofing y el tramado TCP.

use atenea_identity::{DeviceId, NetworkId, PublicKey};
use blake2::{Blake2s256, Digest};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use std::net::{IpAddr, SocketAddr};

/// Primer byte de todo mensaje del relay (distinto de la versión `1` de los datagramas de Atenea).
pub const MAGIC: u8 = 0xA1;
pub const COOKIE_LEN: usize = 16;
/// Tamaño exacto de `COOKIE_REQ`: la petición no es menor que la respuesta (sin amplificación).
pub const COOKIE_REQ_LEN: usize = 64;
/// Trama TCP máxima (cuerpo). Mayor o cero = cierre de la conexión (ADR-004).
pub const MAX_FRAME: usize = 2048;
pub const TOKEN_LEN: usize = 173;
pub const SIG_LEN: usize = 64;
/// Sobrecarga de `DATA` (magic, tipo e `alloc_id`).
pub const DATA_OVERHEAD: usize = 10;
/// Un token no puede declarar más validez que esto (el coordinator emite ≤ 300 s).
pub const MAX_TOKEN_TTL_SECS: u64 = 600;
/// Tolerancia de reloj para tokens y marcas de tiempo de la asignación (ADR-002).
pub const CLOCK_TOLERANCE_SECS: u64 = 30;
pub const ALPN: &[u8] = b"atenea-relay/1";

const TOKEN_MAGIC: &[u8; 4] = b"AORT";
const TOKEN_DOMAIN: &[u8] = b"atenea-one/relay-token/v1\0";
const ALLOC_DOMAIN: &[u8] = b"atenea-one/relay-alloc/v1\0";
const TOKEN_SIGNED_LEN: usize = TOKEN_LEN - SIG_LEN;

pub mod ty {
    pub const COOKIE_REQ: u8 = 0x01;
    pub const COOKIE: u8 = 0x02;
    pub const ALLOC: u8 = 0x03;
    pub const ALLOC_OK: u8 = 0x04;
    pub const ALLOC_ERR: u8 = 0x05;
    pub const DATA: u8 = 0x06;
    pub const PING: u8 = 0x08;
    pub const PONG: u8 = 0x09;
    pub const PEER_STATE: u8 = 0x0A;
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ErrCode {
    BadToken = 1,
    Expired = 2,
    RateLimited = 3,
    Full = 4,
    NotFound = 5,
    BadSignature = 6,
    BadCookie = 7,
    Unauthorized = 8,
}

impl ErrCode {
    pub fn from_u8(v: u8) -> Option<ErrCode> {
        Some(match v {
            1 => ErrCode::BadToken,
            2 => ErrCode::Expired,
            3 => ErrCode::RateLimited,
            4 => ErrCode::Full,
            5 => ErrCode::NotFound,
            6 => ErrCode::BadSignature,
            7 => ErrCode::BadCookie,
            8 => ErrCode::Unauthorized,
            _ => return None,
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ParseError {
    TooShort,
    TooLong,
    BadMagic,
    UnknownType,
    BadLength,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Message<'a> {
    CookieReq,
    Cookie {
        cookie: [u8; COOKIE_LEN],
    },
    Alloc {
        cookie: [u8; COOKIE_LEN],
        ts: u64,
        token: &'a [u8],
        sig: [u8; SIG_LEN],
    },
    AllocOk {
        alloc_id: u64,
        ttl_secs: u16,
        peer_ready: bool,
    },
    AllocErr {
        code: ErrCode,
    },
    Data {
        alloc_id: u64,
        datagram: &'a [u8],
    },
    Ping {
        alloc_id: u64,
        nonce: u64,
    },
    Pong {
        alloc_id: u64,
        nonce: u64,
    },
    PeerState {
        alloc_id: u64,
        ready: bool,
    },
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().expect("8 bytes"))
}

/// Parseo estricto de un mensaje (un datagrama UDP o el cuerpo de una trama TCP).
pub fn parse(buf: &[u8]) -> Result<Message<'_>, ParseError> {
    if buf.len() > MAX_FRAME {
        return Err(ParseError::TooLong);
    }
    if buf.len() < 2 {
        return Err(ParseError::TooShort);
    }
    if buf[0] != MAGIC {
        return Err(ParseError::BadMagic);
    }
    let body = &buf[2..];
    match buf[1] {
        ty::COOKIE_REQ => {
            if buf.len() != COOKIE_REQ_LEN {
                return Err(ParseError::BadLength);
            }
            Ok(Message::CookieReq)
        }
        ty::COOKIE => {
            let cookie: [u8; COOKIE_LEN] = body.try_into().map_err(|_| ParseError::BadLength)?;
            Ok(Message::Cookie { cookie })
        }
        ty::ALLOC => {
            if body.len() != COOKIE_LEN + 8 + 2 + TOKEN_LEN + SIG_LEN {
                return Err(ParseError::BadLength);
            }
            let token_len = usize::from(u16::from_be_bytes([body[24], body[25]]));
            if token_len != TOKEN_LEN {
                return Err(ParseError::BadLength);
            }
            Ok(Message::Alloc {
                cookie: body[..COOKIE_LEN].try_into().expect("16"),
                ts: be64(&body[COOKIE_LEN..]),
                token: &body[26..26 + TOKEN_LEN],
                sig: body[26 + TOKEN_LEN..].try_into().expect("64"),
            })
        }
        ty::ALLOC_OK => {
            if body.len() != 11 || body[10] > 1 {
                return Err(ParseError::BadLength);
            }
            Ok(Message::AllocOk {
                alloc_id: be64(body),
                ttl_secs: u16::from_be_bytes([body[8], body[9]]),
                peer_ready: body[10] == 1,
            })
        }
        ty::ALLOC_ERR => {
            if body.len() != 1 {
                return Err(ParseError::BadLength);
            }
            ErrCode::from_u8(body[0])
                .map(|code| Message::AllocErr { code })
                .ok_or(ParseError::BadLength)
        }
        ty::DATA => {
            if body.len() < 8 + 2 {
                return Err(ParseError::TooShort);
            }
            Ok(Message::Data {
                alloc_id: be64(body),
                datagram: &body[8..],
            })
        }
        ty::PING | ty::PONG => {
            if body.len() != 16 {
                return Err(ParseError::BadLength);
            }
            let (alloc_id, nonce) = (be64(body), be64(&body[8..]));
            Ok(if buf[1] == ty::PING {
                Message::Ping { alloc_id, nonce }
            } else {
                Message::Pong { alloc_id, nonce }
            })
        }
        ty::PEER_STATE => {
            if body.len() != 9 || body[8] > 1 {
                return Err(ParseError::BadLength);
            }
            Ok(Message::PeerState {
                alloc_id: be64(body),
                ready: body[8] == 1,
            })
        }
        _ => Err(ParseError::UnknownType),
    }
}

pub fn encode_cookie_req() -> Vec<u8> {
    let mut v = vec![0u8; COOKIE_REQ_LEN];
    v[0] = MAGIC;
    v[1] = ty::COOKIE_REQ;
    v
}

pub fn encode_cookie(cookie: &[u8; COOKIE_LEN]) -> Vec<u8> {
    let mut v = vec![MAGIC, ty::COOKIE];
    v.extend_from_slice(cookie);
    v
}

/// `token` debe medir exactamente `TOKEN_LEN` (lo comprueba `parse` en el otro extremo).
pub fn encode_alloc(
    cookie: &[u8; COOKIE_LEN],
    ts: u64,
    token: &[u8],
    sig: &[u8; SIG_LEN],
) -> Vec<u8> {
    let mut v = vec![MAGIC, ty::ALLOC];
    v.extend_from_slice(cookie);
    v.extend_from_slice(&ts.to_be_bytes());
    v.extend_from_slice(&(token.len() as u16).to_be_bytes());
    v.extend_from_slice(token);
    v.extend_from_slice(sig);
    v
}

pub fn encode_alloc_ok(alloc_id: u64, ttl_secs: u16, peer_ready: bool) -> Vec<u8> {
    let mut v = vec![MAGIC, ty::ALLOC_OK];
    v.extend_from_slice(&alloc_id.to_be_bytes());
    v.extend_from_slice(&ttl_secs.to_be_bytes());
    v.push(u8::from(peer_ready));
    v
}

pub fn encode_alloc_err(code: ErrCode) -> Vec<u8> {
    vec![MAGIC, ty::ALLOC_ERR, code as u8]
}

pub fn encode_data(alloc_id: u64, datagram: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(DATA_OVERHEAD + datagram.len());
    v.extend_from_slice(&[MAGIC, ty::DATA]);
    v.extend_from_slice(&alloc_id.to_be_bytes());
    v.extend_from_slice(datagram);
    v
}

pub fn encode_ping(alloc_id: u64, nonce: u64, pong: bool) -> Vec<u8> {
    let mut v = vec![MAGIC, if pong { ty::PONG } else { ty::PING }];
    v.extend_from_slice(&alloc_id.to_be_bytes());
    v.extend_from_slice(&nonce.to_be_bytes());
    v
}

pub fn encode_peer_state(alloc_id: u64, ready: bool) -> Vec<u8> {
    let mut v = vec![MAGIC, ty::PEER_STATE];
    v.extend_from_slice(&alloc_id.to_be_bytes());
    v.push(u8::from(ready));
    v
}

// ---------------------------------------------------------------------------------------------
// Token de asignación (lo firma el coordinator; el relay solo confía en sus claves públicas)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TokenError {
    Malformed,
    BadSignature,
    UnknownIssuer,
    Expired,
    NotYetValid,
    TooLong,
}

/// Autoriza al dispositivo `src` a enviar al dispositivo `dst` de `network` a través de un relay.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RelayToken {
    pub network_id: NetworkId,
    pub src_device: DeviceId,
    pub dst_device: DeviceId,
    /// Clave de identidad Ed25519 de `src`: el relay la usa para comprobar la prueba de posesión.
    pub src_identity_pub: PublicKey,
    pub not_before: u64,
    pub not_after: u64,
    pub token_id: [u8; 8],
    pub signature: [u8; SIG_LEN],
}

fn signed_message(domain: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let mut m = domain.to_vec();
    for p in parts {
        m.extend_from_slice(p);
    }
    m
}

impl RelayToken {
    fn body(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(TOKEN_SIGNED_LEN);
        b.extend_from_slice(TOKEN_MAGIC);
        b.push(1);
        b.extend_from_slice(&self.network_id);
        b.extend_from_slice(&self.src_device);
        b.extend_from_slice(&self.dst_device);
        b.extend_from_slice(&self.src_identity_pub);
        b.extend_from_slice(&self.not_before.to_be_bytes());
        b.extend_from_slice(&self.not_after.to_be_bytes());
        b.extend_from_slice(&self.token_id);
        b
    }

    /// Emite y firma un token con la clave del coordinator.
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        signer: &SigningKey,
        network_id: NetworkId,
        src_device: DeviceId,
        dst_device: DeviceId,
        src_identity_pub: PublicKey,
        not_before: u64,
        not_after: u64,
        token_id: [u8; 8],
    ) -> RelayToken {
        let mut t = RelayToken {
            network_id,
            src_device,
            dst_device,
            src_identity_pub,
            not_before,
            not_after,
            token_id,
            signature: [0; SIG_LEN],
        };
        t.signature = signer
            .sign(&signed_message(TOKEN_DOMAIN, &[&t.body()]))
            .to_bytes();
        t
    }

    pub fn encode(&self) -> [u8; TOKEN_LEN] {
        let mut out = [0u8; TOKEN_LEN];
        out[..TOKEN_SIGNED_LEN].copy_from_slice(&self.body());
        out[TOKEN_SIGNED_LEN..].copy_from_slice(&self.signature);
        out
    }

    pub fn parse(b: &[u8]) -> Result<RelayToken, TokenError> {
        if b.len() != TOKEN_LEN || &b[..4] != TOKEN_MAGIC || b[4] != 1 {
            return Err(TokenError::Malformed);
        }
        let arr = |from: usize, n: usize| &b[from..from + n];
        Ok(RelayToken {
            network_id: arr(5, 16).try_into().expect("16"),
            src_device: arr(21, 16).try_into().expect("16"),
            dst_device: arr(37, 16).try_into().expect("16"),
            src_identity_pub: arr(53, 32).try_into().expect("32"),
            not_before: be64(arr(85, 8)),
            not_after: be64(arr(93, 8)),
            token_id: arr(101, 8).try_into().expect("8"),
            signature: arr(TOKEN_SIGNED_LEN, SIG_LEN).try_into().expect("64"),
        })
    }

    /// Firma de alguna de las claves del coordinator, ventana de tiempo y vida máxima.
    pub fn verify(&self, issuers: &[PublicKey], now: u64) -> Result<(), TokenError> {
        if self.not_after <= self.not_before
            || self.not_after - self.not_before > MAX_TOKEN_TTL_SECS
        {
            return Err(TokenError::TooLong);
        }
        let msg = signed_message(TOKEN_DOMAIN, &[&self.body()]);
        let sig = Signature::from_bytes(&self.signature);
        let mut known = false;
        let mut valid = false;
        for k in issuers {
            if let Ok(vk) = VerifyingKey::from_bytes(k) {
                known = true;
                if vk.verify_strict(&msg, &sig).is_ok() {
                    valid = true;
                    break;
                }
            }
        }
        if !known {
            return Err(TokenError::UnknownIssuer);
        }
        if !valid {
            return Err(TokenError::BadSignature);
        }
        if now.saturating_add(CLOCK_TOLERANCE_SECS) < self.not_before {
            return Err(TokenError::NotYetValid);
        }
        if now >= self.not_after.saturating_add(CLOCK_TOLERANCE_SECS) {
            return Err(TokenError::Expired);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Prueba de posesión de la asignación (la firma el dispositivo con su clave de identidad)
// ---------------------------------------------------------------------------------------------

pub fn sign_alloc(
    identity: &SigningKey,
    cookie: &[u8; COOKIE_LEN],
    ts: u64,
    token: &[u8],
) -> [u8; SIG_LEN] {
    identity
        .sign(&signed_message(
            ALLOC_DOMAIN,
            &[cookie, &ts.to_be_bytes(), token],
        ))
        .to_bytes()
}

pub fn verify_alloc(
    identity_pub: &PublicKey,
    cookie: &[u8; COOKIE_LEN],
    ts: u64,
    token: &[u8],
    sig: &[u8; SIG_LEN],
) -> bool {
    VerifyingKey::from_bytes(identity_pub).is_ok_and(|vk| {
        vk.verify_strict(
            &signed_message(ALLOC_DOMAIN, &[cookie, &ts.to_be_bytes(), token]),
            &Signature::from_bytes(sig),
        )
        .is_ok()
    })
}

// ---------------------------------------------------------------------------------------------
// Cookies sin estado (anti-suplantación de origen UDP)
// ---------------------------------------------------------------------------------------------

pub struct CookieMaker {
    secret: [u8; 32],
}

impl CookieMaker {
    pub fn new(secret: [u8; 32]) -> CookieMaker {
        CookieMaker { secret }
    }

    fn at(&self, src: SocketAddr, bucket: u64) -> [u8; COOKIE_LEN] {
        let mut h = Blake2s256::new();
        h.update(self.secret);
        match src.ip() {
            IpAddr::V4(a) => h.update(a.octets()),
            IpAddr::V6(a) => h.update(a.octets()),
        }
        h.update(src.port().to_be_bytes());
        h.update(bucket.to_be_bytes());
        let d = h.finalize();
        d[..COOKIE_LEN].try_into().expect("16")
    }

    /// Cookie vigente (cubos de 60 s) ligada a la dirección de origen.
    pub fn cookie(&self, src: SocketAddr, now: u64) -> [u8; COOKIE_LEN] {
        self.at(src, now / 60)
    }

    /// Acepta el cubo actual y el anterior.
    pub fn verify(&self, src: SocketAddr, cookie: &[u8; COOKIE_LEN], now: u64) -> bool {
        let b = now / 60;
        &self.at(src, b) == cookie || (b > 0 && &self.at(src, b - 1) == cookie)
    }
}

// ---------------------------------------------------------------------------------------------
// Tramado TCP: `longitud (u16 BE)` + cuerpo, con lecturas parciales
// ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FrameError {
    /// Longitud cero o mayor que `MAX_FRAME`: la conexión debe cerrarse.
    BadLength,
}

pub fn encode_frame(body: &[u8]) -> Result<Vec<u8>, FrameError> {
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(FrameError::BadLength);
    }
    let mut v = Vec::with_capacity(2 + body.len());
    v.extend_from_slice(&(body.len() as u16).to_be_bytes());
    v.extend_from_slice(body);
    Ok(v)
}

/// Reensambla tramas desde un flujo con lecturas arbitrarias. Memoria acotada: nunca retiene más de
/// una trama incompleta (`2 + MAX_FRAME` bytes) más lo que se haya empujado desde la última lectura.
#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> FrameDecoder {
        FrameDecoder::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Siguiente trama completa; `Ok(None)` si falta flujo; `Err` obliga a cerrar.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        if self.buf.len() < 2 {
            return Ok(None);
        }
        let len = usize::from(u16::from_be_bytes([self.buf[0], self.buf[1]]));
        if len == 0 || len > MAX_FRAME {
            return Err(FrameError::BadLength);
        }
        if self.buf.len() < 2 + len {
            return Ok(None);
        }
        let frame = self.buf[2..2 + len].to_vec();
        self.buf.drain(..2 + len);
        Ok(Some(frame))
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
}

/// Un datagrama de Atenea cabe en un `DATA` y en una trama.
pub const MAX_RELAYED_DATAGRAM: usize = atenea_protocol::MAX_DATAGRAM;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sk(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    fn token(now: u64) -> (RelayToken, SigningKey, SigningKey) {
        let (coord, dev) = (sk(1), sk(2));
        let t = RelayToken::issue(
            &coord,
            [7; 16],
            [1; 16],
            [2; 16],
            dev.verifying_key().to_bytes(),
            now,
            now + 300,
            [9; 8],
        );
        (t, coord, dev)
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn message_roundtrips() {
        assert_eq!(parse(&encode_cookie_req()), Ok(Message::CookieReq));
        assert_eq!(
            parse(&encode_cookie(&[3; 16])),
            Ok(Message::Cookie { cookie: [3; 16] })
        );
        assert_eq!(
            parse(&encode_alloc_ok(5, 120, true)),
            Ok(Message::AllocOk {
                alloc_id: 5,
                ttl_secs: 120,
                peer_ready: true
            })
        );
        assert_eq!(
            parse(&encode_alloc_err(ErrCode::Full)),
            Ok(Message::AllocErr {
                code: ErrCode::Full
            })
        );
        assert_eq!(
            parse(&encode_data(9, b"hola")),
            Ok(Message::Data {
                alloc_id: 9,
                datagram: b"hola"
            })
        );
        assert_eq!(
            parse(&encode_ping(1, 2, false)),
            Ok(Message::Ping {
                alloc_id: 1,
                nonce: 2
            })
        );
        assert_eq!(
            parse(&encode_ping(1, 2, true)),
            Ok(Message::Pong {
                alloc_id: 1,
                nonce: 2
            })
        );
        assert_eq!(
            parse(&encode_peer_state(4, false)),
            Ok(Message::PeerState {
                alloc_id: 4,
                ready: false
            })
        );
        let (t, _, dev) = token(1000);
        let tok = t.encode();
        let sig = sign_alloc(&dev, &[5; 16], 1000, &tok);
        let m = encode_alloc(&[5; 16], 1000, &tok, &sig);
        assert_eq!(m.len(), 265);
        assert_eq!(
            parse(&m),
            Ok(Message::Alloc {
                cookie: [5; 16],
                ts: 1000,
                token: &tok,
                sig
            })
        );
    }

    #[test]
    fn cookie_response_is_smaller_than_request() {
        assert!(encode_cookie(&[0; 16]).len() < encode_cookie_req().len());
    }

    #[test]
    fn parse_rejects_malformed() {
        assert_eq!(parse(&[]), Err(ParseError::TooShort));
        assert_eq!(parse(&[0xA1]), Err(ParseError::TooShort));
        assert_eq!(parse(&[0x01, 0x01]), Err(ParseError::BadMagic));
        assert_eq!(parse(&[MAGIC, 0x77]), Err(ParseError::UnknownType));
        assert_eq!(
            parse(&[MAGIC, ty::COOKIE_REQ, 0]),
            Err(ParseError::BadLength)
        );
        assert_eq!(
            parse(&[MAGIC, ty::COOKIE, 1, 2]),
            Err(ParseError::BadLength)
        );
        assert_eq!(
            parse(&[MAGIC, ty::ALLOC_ERR, 99]),
            Err(ParseError::BadLength)
        );
        assert_eq!(
            parse(&[MAGIC, ty::ALLOC_OK, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 2]),
            Err(ParseError::BadLength)
        );
        assert_eq!(
            parse(&[MAGIC, ty::DATA, 0, 0, 0, 0, 0, 0, 0, 1]),
            Err(ParseError::TooShort)
        );
        assert_eq!(parse(&vec![MAGIC; MAX_FRAME + 1]), Err(ParseError::TooLong));
        // ALLOC con longitud de token declarada distinta
        let (t, _, dev) = token(1);
        let tok = t.encode();
        let mut m = encode_alloc(&[0; 16], 1, &tok, &sign_alloc(&dev, &[0; 16], 1, &tok));
        m[26] = 0xFF;
        assert_eq!(parse(&m), Err(ParseError::BadLength));
    }

    #[test]
    fn token_roundtrip_and_verification() {
        let (t, coord, _) = token(1000);
        let enc = t.encode();
        assert_eq!(enc.len(), TOKEN_LEN);
        assert_eq!(RelayToken::parse(&enc), Ok(t.clone()));
        let key = coord.verifying_key().to_bytes();
        assert_eq!(t.verify(&[key], 1000), Ok(()));
        // tolerancia de 30 s en ambos extremos
        assert_eq!(t.verify(&[key], 1000 - 30), Ok(()));
        assert_eq!(t.verify(&[key], 1000 - 31), Err(TokenError::NotYetValid));
        assert_eq!(t.verify(&[key], 1300 + 29), Ok(()));
        assert_eq!(t.verify(&[key], 1300 + 30), Err(TokenError::Expired));
    }

    #[test]
    fn token_verification_does_not_overflow_near_u64_max() {
        let coord = sk(1);
        let key = coord.verifying_key().to_bytes();
        let t = RelayToken::issue(
            &coord,
            [7; 16],
            [1; 16],
            [2; 16],
            [0; 32],
            u64::MAX - 100,
            u64::MAX - 1,
            [0; 8],
        );
        assert_eq!(t.verify(&[key], u64::MAX), Err(TokenError::Expired));
        assert_eq!(t.verify(&[key], 0), Err(TokenError::NotYetValid));
        assert_eq!(t.verify(&[key], u64::MAX - 50), Ok(()));
    }

    #[test]
    fn token_rejects_other_issuer_tampering_and_long_lifetime() {
        let (t, coord, _) = token(1000);
        let other = sk(3).verifying_key().to_bytes();
        let key = coord.verifying_key().to_bytes();
        assert_eq!(t.verify(&[other], 1000), Err(TokenError::BadSignature));
        assert_eq!(t.verify(&[], 1000), Err(TokenError::UnknownIssuer));
        assert_eq!(
            t.verify(&[[0u8; 32]], 1000),
            Err(TokenError::UnknownIssuer).or(Err(TokenError::BadSignature))
        );
        assert_eq!(t.verify(&[other, key], 1000), Ok(()));
        for i in [5, 21, 37, 53, 85, 93, 101, 120] {
            let mut b = t.encode();
            b[i] ^= 1;
            let parsed = RelayToken::parse(&b).unwrap();
            assert!(parsed.verify(&[key], 1000).is_err(), "byte {i}");
        }
        assert_eq!(
            RelayToken::parse(&t.encode()[..172]),
            Err(TokenError::Malformed)
        );
        let mut bad = t.encode();
        bad[0] = b'X';
        assert_eq!(RelayToken::parse(&bad), Err(TokenError::Malformed));
        let long = RelayToken::issue(
            &coord,
            [7; 16],
            [1; 16],
            [2; 16],
            [0; 32],
            1000,
            1000 + MAX_TOKEN_TTL_SECS + 1,
            [0; 8],
        );
        assert_eq!(long.verify(&[key], 1000), Err(TokenError::TooLong));
        let backwards = RelayToken::issue(
            &coord, [7; 16], [1; 16], [2; 16], [0; 32], 1000, 1000, [0; 8],
        );
        assert_eq!(backwards.verify(&[key], 1000), Err(TokenError::TooLong));
    }

    #[test]
    fn alloc_proof_binds_cookie_timestamp_token_and_domain() {
        let (t, coord, dev) = token(1000);
        let tok = t.encode();
        let pubkey = dev.verifying_key().to_bytes();
        let sig = sign_alloc(&dev, &[5; 16], 1000, &tok);
        assert!(verify_alloc(&pubkey, &[5; 16], 1000, &tok, &sig));
        assert!(!verify_alloc(&pubkey, &[6; 16], 1000, &tok, &sig));
        assert!(!verify_alloc(&pubkey, &[5; 16], 1001, &tok, &sig));
        let mut other = tok;
        other[60] ^= 1;
        assert!(!verify_alloc(&pubkey, &[5; 16], 1000, &other, &sig));
        assert!(!verify_alloc(
            &coord.verifying_key().to_bytes(),
            &[5; 16],
            1000,
            &tok,
            &sig
        ));
        // una firma del token (otro dominio) no vale como prueba de posesión
        let cross = coord.sign(&tok).to_bytes();
        assert!(!verify_alloc(
            &coord.verifying_key().to_bytes(),
            &[5; 16],
            1000,
            &tok,
            &cross
        ));
    }

    #[test]
    fn cookies_are_bound_to_source_and_expire() {
        let m = CookieMaker::new([4; 32]);
        let src = addr("192.0.2.1:5000");
        let c = m.cookie(src, 600);
        assert!(m.verify(src, &c, 600));
        assert!(m.verify(src, &c, 659));
        assert!(m.verify(src, &c, 660 + 59), "cubo anterior aceptado");
        assert!(!m.verify(src, &c, 660 + 60), "caduca tras dos cubos");
        assert!(!m.verify(addr("192.0.2.1:5001"), &c, 600));
        assert!(!m.verify(addr("192.0.2.2:5000"), &c, 600));
        assert!(!CookieMaker::new([5; 32]).verify(src, &c, 600));
        assert!(m.verify(
            addr("[2001:db8::1]:9"),
            &m.cookie(addr("[2001:db8::1]:9"), 5),
            5
        ));
    }

    #[test]
    fn frames_with_partial_reads_and_limits() {
        let a = encode_frame(b"uno").unwrap();
        let b = encode_frame(&vec![7u8; MAX_FRAME]).unwrap();
        let mut d = FrameDecoder::new();
        let stream: Vec<u8> = [a.clone(), b.clone(), a.clone()].concat();
        let mut got = Vec::new();
        for chunk in stream.chunks(3) {
            d.push(chunk);
            while let Some(f) = d.next_frame().unwrap() {
                got.push(f);
            }
        }
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], b"uno");
        assert_eq!(got[1].len(), MAX_FRAME);
        assert_eq!(d.buffered(), 0);
        assert_eq!(encode_frame(&[]), Err(FrameError::BadLength));
        assert_eq!(
            encode_frame(&vec![0; MAX_FRAME + 1]),
            Err(FrameError::BadLength)
        );
        let mut z = FrameDecoder::new();
        z.push(&[0, 0, 1]);
        assert_eq!(z.next_frame(), Err(FrameError::BadLength));
        let mut big = FrameDecoder::new();
        big.push(&((MAX_FRAME + 1) as u16).to_be_bytes());
        assert_eq!(big.next_frame(), Err(FrameError::BadLength));
        let mut incomplete = FrameDecoder::new();
        incomplete.push(&[0, 5, 1, 2]);
        assert_eq!(incomplete.next_frame(), Ok(None));
    }

    #[test]
    fn relayed_datagram_fits_a_frame() {
        const { assert!(DATA_OVERHEAD + MAX_RELAYED_DATAGRAM <= MAX_FRAME) };
    }

    proptest! {
        #[test]
        fn parse_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..300)) {
            let _ = parse(&buf);
        }

        #[test]
        fn token_parse_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..200)) {
            if let Ok(t) = RelayToken::parse(&buf) {
                let enc = t.encode();
                prop_assert_eq!(enc.as_slice(), buf.as_slice());
            }
        }

        #[test]
        fn frames_reassemble_regardless_of_chunking(
            bodies in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 1..200), 1..8),
            chunk in 1usize..50,
        ) {
            let stream: Vec<u8> = bodies.iter().flat_map(|b| encode_frame(b).unwrap()).collect();
            let mut d = FrameDecoder::new();
            let mut out = Vec::new();
            for c in stream.chunks(chunk) {
                d.push(c);
                while let Some(f) = d.next_frame().unwrap() { out.push(f); }
            }
            prop_assert_eq!(out, bodies);
        }
    }
}
