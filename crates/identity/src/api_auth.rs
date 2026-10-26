// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Autenticación de peticiones de dispositivo al control plane (docs/api/control-plane-v1.md §3.3).
//!
//! `Authorization: AtDevice v1 device=<hex16>, ts=<unix>, nonce=<hex16>, sig=<hex64>` con
//! `sig = Ed25519(identity_sk, DOMAIN ‖ METHOD \n PATH_Y_QUERY \n ts \n nonce \n BLAKE2s-256(cuerpo))`.
//! Esta función solo comprueba la firma: la ventana de tiempo y la repetición de `nonce` las aplica el servidor.

use crate::{DeviceId, Error, PublicKey, SigningKey, VerifyingKey, hash};
use ed25519_dalek::{Signature, Signer};

pub const API_AUTH_DOMAIN: &[u8] = b"atenea-one/api-auth/v1\0";
const PREFIX: &str = "AtDevice v1 ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedRequest {
    pub device_id: DeviceId,
    pub ts: u64,
    pub nonce: [u8; 16],
    pub sig: [u8; 64],
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn message(method: &str, path_and_query: &str, ts: u64, nonce: &[u8; 16], body: &[u8]) -> Vec<u8> {
    let mut m = API_AUTH_DOMAIN.to_vec();
    m.extend_from_slice(method.to_ascii_uppercase().as_bytes());
    m.push(b'\n');
    m.extend_from_slice(path_and_query.as_bytes());
    m.push(b'\n');
    m.extend_from_slice(ts.to_string().as_bytes());
    m.push(b'\n');
    m.extend_from_slice(hex(nonce).as_bytes());
    m.push(b'\n');
    m.extend_from_slice(&hash(body));
    m
}

/// Valor de la cabecera `Authorization` para una petición.
pub fn sign_request(
    identity: &SigningKey,
    device_id: DeviceId,
    method: &str,
    path_and_query: &str,
    body: &[u8],
    ts: u64,
    nonce: [u8; 16],
) -> String {
    let sig = identity
        .sign(&message(method, path_and_query, ts, &nonce, body))
        .to_bytes();
    format!(
        "{PREFIX}device={}, ts={ts}, nonce={}, sig={}",
        hex(&device_id),
        hex(&nonce),
        hex(&sig)
    )
}

/// Parseo estricto de la cabecera (campos exactos, en orden, sin duplicados ni extras).
pub fn parse_authorization(header: &str) -> Result<SignedRequest, Error> {
    let rest = header.strip_prefix(PREFIX).ok_or(Error::Malformed)?;
    let mut parts = rest.split(", ");
    let mut field = |name: &str| -> Result<&str, Error> {
        parts
            .next()
            .and_then(|p| p.strip_prefix(name))
            .and_then(|p| p.strip_prefix('='))
            .ok_or(Error::Malformed)
    };
    let device_id = unhex::<16>(field("device")?).ok_or(Error::Malformed)?;
    let ts_text = field("ts")?;
    let ts: u64 = ts_text.parse().map_err(|_| Error::Malformed)?;
    if ts.to_string() != ts_text {
        return Err(Error::Malformed);
    }
    let nonce = unhex::<16>(field("nonce")?).ok_or(Error::Malformed)?;
    let sig = unhex::<64>(field("sig")?).ok_or(Error::Malformed)?;
    if parts.next().is_some() {
        return Err(Error::Malformed);
    }
    Ok(SignedRequest {
        device_id,
        ts,
        nonce,
        sig,
    })
}

/// Verifica la firma de una petición con la clave de identidad registrada del dispositivo.
pub fn verify_request(
    req: &SignedRequest,
    identity_pub: &PublicKey,
    method: &str,
    path_and_query: &str,
    body: &[u8],
) -> Result<(), Error> {
    let key = VerifyingKey::from_bytes(identity_pub).map_err(|_| Error::BadSignature)?;
    key.verify_strict(
        &message(method, path_and_query, req.ts, &req.nonce, body),
        &Signature::from_bytes(&req.sig),
    )
    .map_err(|_| Error::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing_key;

    fn key() -> SigningKey {
        signing_key([9; 32])
    }

    fn signed(method: &str, path: &str, body: &[u8]) -> (String, PublicKey) {
        let k = key();
        (
            sign_request(&k, [1; 16], method, path, body, 1_000, [2; 16]),
            k.verifying_key().to_bytes(),
        )
    }

    #[test]
    fn roundtrip() {
        let (h, pubkey) = signed("get", "/api/v1/device/config?x=1", b"");
        let req = parse_authorization(&h).unwrap();
        assert_eq!(
            (req.device_id, req.ts, req.nonce),
            ([1; 16], 1_000, [2; 16])
        );
        assert!(verify_request(&req, &pubkey, "GET", "/api/v1/device/config?x=1", b"").is_ok());
    }

    #[test]
    fn any_change_breaks_the_signature() {
        let (h, pubkey) = signed("GET", "/a", b"cuerpo");
        let req = parse_authorization(&h).unwrap();
        assert!(verify_request(&req, &pubkey, "GET", "/a", b"cuerpo").is_ok());
        assert!(verify_request(&req, &pubkey, "POST", "/a", b"cuerpo").is_err());
        assert!(verify_request(&req, &pubkey, "GET", "/b", b"cuerpo").is_err());
        assert!(verify_request(&req, &pubkey, "GET", "/a", b"otro").is_err());
        let mut t = req.clone();
        t.ts += 1;
        assert!(verify_request(&t, &pubkey, "GET", "/a", b"cuerpo").is_err());
        let mut n = req.clone();
        n.nonce[0] ^= 1;
        assert!(verify_request(&n, &pubkey, "GET", "/a", b"cuerpo").is_err());
        assert!(
            verify_request(
                &req,
                &signing_key([8; 32]).verifying_key().to_bytes(),
                "GET",
                "/a",
                b"cuerpo"
            )
            .is_err()
        );
    }

    #[test]
    fn domain_is_separated_from_other_signatures() {
        // Una firma de otro dominio (p. ej. el paquete de dispositivo) no vale como petición.
        let k = key();
        let other = k
            .sign(b"atenea-one/device-bundle/v1\0GET\n/a\n1000\n02020202020202020202020202020202\n")
            .to_bytes();
        let req = SignedRequest {
            device_id: [1; 16],
            ts: 1_000,
            nonce: [2; 16],
            sig: other,
        };
        assert!(verify_request(&req, &k.verifying_key().to_bytes(), "GET", "/a", b"").is_err());
    }

    #[test]
    fn parser_is_strict() {
        let (h, _) = signed("GET", "/a", b"");
        assert!(parse_authorization(&h).is_ok());
        for bad in [
            h.replace("AtDevice v1", "AtDevice v2"),
            h.replace("device=", "dev="),
            format!("{h}, extra=1"),
            h.replace("ts=1000", "ts=01000"),
            h.replace("ts=1000", "ts=-1"),
            h.replace(", nonce", ",nonce"),
            h[..h.len() - 2].to_string(),
            String::new(),
        ] {
            assert!(parse_authorization(&bad).is_err(), "{bad}");
        }
        // orden distinto
        assert!(parse_authorization("AtDevice v1 ts=1, device=00000000000000000000000000000000, nonce=00000000000000000000000000000000, sig=0").is_err());
    }
}
