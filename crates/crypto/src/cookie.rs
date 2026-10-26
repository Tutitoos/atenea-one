// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Protección pre-DH de wire-v2. No autentica pares ni confirma rutas.
//!
//! El motor llama `CookieSecrets::rotate` con `Instant::now()`, luego `pre_dh`.
//! Solo `Proceed` permite tocar cuotas DH y llamar `read_init`. `Challenge` exige
//! las cuotas de respuesta independientes (20/s/origen, 100/s global, tabla4096)
//! del motor antes de llamar `seal_reply`; no asigna sesión ni consume cuota DH.
//! `CachedCookie` pertenece a un candidato pendiente: el motor conserva el mismo
//! INIT/Noise, limita reintentos y descarta la entrada al retirar clave/lease/par.
use atenea_protocol as proto;
use blake2::{
    Blake2s256, Blake2sMac, Digest,
    digest::{Mac, consts::U16},
};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{AeadInPlace, KeyInit},
};
use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const COOKIE_TTL: Duration = Duration::from_secs(120);
pub const DEFAULT_THRESHOLD: NonZeroU32 = NonZeroU32::new(100).expect("positivo");
type Mac16 = Blake2sMac<U16>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieError {
    InvalidPacket,
    InvalidSource,
    Authentication,
    Random,
}

/// Origen ya canonicalizado; los campos relay proceden del contexto local/Rx,
/// nunca del INIT no autenticado. UDP y TLS del mismo relay comparten origen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Source {
    bytes: [u8; 65],
    len: u8,
}
impl Source {
    pub fn direct(addr: SocketAddr) -> Result<Self, CookieError> {
        let mut bytes = [0; 65];
        let ip = match addr.ip() {
            IpAddr::V6(ip) => {
                if matches!(addr, SocketAddr::V6(a) if a.scope_id() != 0) {
                    return Err(CookieError::InvalidSource);
                }
                if let Some(v4) = ip.to_ipv4_mapped() {
                    IpAddr::V4(v4)
                } else {
                    if ip.is_unicast_link_local() {
                        return Err(CookieError::InvalidSource);
                    }
                    IpAddr::V6(ip)
                }
            }
            ip => ip,
        };
        let len = match ip {
            IpAddr::V4(ip) => {
                bytes[0] = 1;
                bytes[1..5].copy_from_slice(&ip.octets());
                7
            }
            IpAddr::V6(ip) => {
                bytes[0] = 2;
                bytes[1..17].copy_from_slice(&ip.octets());
                19
            }
        };
        bytes[len - 2..len].copy_from_slice(&addr.port().to_be_bytes());
        Ok(Self {
            bytes,
            len: len as u8,
        })
    }
    pub fn relay(
        relay_id: &[u8; 16],
        network: &[u8; 16],
        sender: &[u8; 16],
        receiver: &[u8; 16],
    ) -> Self {
        let mut bytes = [0; 65];
        bytes[0] = 3;
        bytes[1..17].copy_from_slice(relay_id);
        bytes[17..33].copy_from_slice(network);
        bytes[33..49].copy_from_slice(sender);
        bytes[49..65].copy_from_slice(receiver);
        Self { bytes, len: 65 }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

fn keyed(key: &[u8], bytes: &[u8]) -> Mac16 {
    let mut mac =
        <Mac16 as blake2::digest::KeyInit>::new_from_slice(key).expect("clave 16/32 bytes");
    Mac::update(&mut mac, bytes);
    mac
}
/// BLAKE2s keyed con tamaño de salida16 en el bloque de parámetros; no truncado.
pub fn mac16(key: &[u8], bytes: &[u8]) -> [u8; 16] {
    keyed(key, bytes).finalize().into_bytes().into()
}
fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Blake2s256::new();
    for part in parts {
        Digest::update(&mut h, part);
    }
    h.finalize().into()
}
pub fn mac1_key(public: &[u8; 32]) -> [u8; 32] {
    hash(&[b"label-mac1", public])
}
pub fn reply_key(public: &[u8; 32], mac1: &[u8; 16]) -> [u8; 32] {
    hash(&[b"label-cookie", public, mac1])
}

/// Construye un INIT v2 completo; al reintentar se conserva el mismo Noise/índice.
pub fn encode_init(
    index: u32,
    noise: &[u8],
    public: &[u8; 32],
    cookie: Option<&[u8; 16]>,
) -> Result<Vec<u8>, proto::ParseError> {
    let mut packet = proto::encode_init(index, noise, &[0; 16], &[0; 16])?;
    let mac1 = mac16(&mac1_key(public), &packet[..114]);
    packet[114..130].copy_from_slice(&mac1);
    if let Some(cookie) = cookie {
        let mac2 = mac16(cookie, &packet[..130]);
        packet[130..].copy_from_slice(&mac2);
    }
    Ok(packet)
}
pub fn verify_mac1(packet: &[u8], public: &[u8; 32]) -> bool {
    matches!(proto::parse(packet), Ok(proto::Packet::Init { .. }))
        && keyed(&mac1_key(public), &packet[..114])
            .verify_slice(&packet[114..130])
            .is_ok()
}

/// Secretos efímeros por proceso. Reloj monotónico inyectado; un salto de dos
/// períodos elimina el anterior. Fallo RNG se propaga: no continuar pre-DH.
pub struct CookieSecrets {
    current: Zeroizing<[u8; 32]>,
    previous: Option<Zeroizing<[u8; 32]>>,
    since: Instant,
}
impl CookieSecrets {
    pub fn new(now: Instant) -> Result<Self, CookieError> {
        let mut secret = Zeroizing::new([0; 32]);
        getrandom::fill(secret.as_mut()).map_err(|_| CookieError::Random)?;
        Ok(Self {
            current: secret,
            previous: None,
            since: now,
        })
    }
    pub fn rotate(&mut self, now: Instant) -> Result<(), CookieError> {
        self.rotate_with(now, getrandom::fill)
    }
    fn rotate_with(
        &mut self,
        now: Instant,
        rng: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Result<(), CookieError> {
        let elapsed = now.saturating_duration_since(self.since);
        if elapsed < COOKIE_TTL {
            return Ok(());
        }
        let mut next = Zeroizing::new([0; 32]);
        rng(next.as_mut()).map_err(|_| CookieError::Random)?;
        let old = std::mem::replace(&mut self.current, next);
        self.previous = (elapsed < COOKIE_TTL * 2).then_some(old);
        // Anclar a los períodos originales, no al retraso del temporizador.
        self.since =
            now - Duration::from_nanos((elapsed.as_nanos() % COOKIE_TTL.as_nanos()) as u64);
        Ok(())
    }
    pub fn cookie(&self, source: &Source) -> Zeroizing<[u8; 16]> {
        Zeroizing::new(mac16(self.current.as_ref(), source.as_bytes()))
    }
    pub fn verify_mac2(&self, packet: &[u8], source: &Source) -> bool {
        if !matches!(proto::parse(packet), Ok(proto::Packet::Init { .. })) {
            return false;
        }
        let current = self.cookie(source);
        let previous = Zeroizing::new(mac16(
            self.previous
                .as_ref()
                .map_or(self.current.as_ref(), |s| s.as_ref()),
            source.as_bytes(),
        ));
        // Calcular y comparar ambas candidatas sin cortocircuito.
        let a = keyed(current.as_ref(), &packet[..130])
            .verify_slice(&packet[130..])
            .is_ok();
        let b = keyed(previous.as_ref(), &packet[..130])
            .verify_slice(&packet[130..])
            .is_ok();
        let nonzero = packet[130..].iter().fold(0u8, |acc, byte| acc | byte) != 0;
        (a | (b & self.previous.is_some())) & nonzero
    }
}

/// Contador por estática local; no tabla por emisor ni reloj Unix.
pub struct LoadWindow {
    start: Instant,
    count: u64,
    threshold: NonZeroU32,
}
impl LoadWindow {
    pub fn new(now: Instant, threshold: NonZeroU32) -> Self {
        Self {
            start: now,
            count: 0,
            threshold,
        }
    }
    fn count(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.start);
        if elapsed >= Duration::from_secs(1) {
            self.start = now - Duration::from_nanos(elapsed.subsec_nanos().into());
            self.count = 0;
        }
        self.count = self.count.saturating_add(1);
        self.count > u64::from(self.threshold.get())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreDh {
    Drop,
    Challenge,
    Proceed,
}
/// Orden puro/testeable: parseo→MAC1→contar→MAC2. No RNG, DH, cuotas por
/// origen ni sesiones. El llamador rota secretos antes y aplica cuotas después.
pub fn pre_dh(
    packet: &[u8],
    public: &[u8; 32],
    source: &Source,
    secrets: &CookieSecrets,
    load: &mut LoadWindow,
    now: Instant,
) -> PreDh {
    if !verify_mac1(packet, public) {
        return PreDh::Drop;
    }
    if load.count(now) && !secrets.verify_mac2(packet, source) {
        PreDh::Challenge
    } else {
        PreDh::Proceed
    }
}

/// Cifra una respuesta tras obtener cupo de respuesta. RNG fallido => Err y
/// ningún datagrama. El nonce fijo solo existe en tests, no en esta API.
pub fn seal_reply(
    index: u32,
    public: &[u8; 32],
    mac1: &[u8; 16],
    cookie: &[u8; 16],
) -> Result<[u8; 62], CookieError> {
    seal_with_rng(index, public, mac1, cookie, getrandom::fill)
}
fn seal_with_rng(
    index: u32,
    public: &[u8; 32],
    mac1: &[u8; 16],
    cookie: &[u8; 16],
    rng: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
) -> Result<[u8; 62], CookieError> {
    let mut nonce = [0; 24];
    rng(&mut nonce).map_err(|_| CookieError::Random)?;
    seal_with_nonce(index, public, mac1, cookie, &nonce)
}
fn seal_with_nonce(
    index: u32,
    public: &[u8; 32],
    mac1: &[u8; 16],
    cookie: &[u8; 16],
    nonce: &[u8; 24],
) -> Result<[u8; 62], CookieError> {
    let mut packet = proto::encode_cookie_reply(index, nonce, &[0; 32]);
    let mut aad = [0; 46];
    aad[..30].copy_from_slice(&packet[..30]);
    aad[30..].copy_from_slice(mac1);
    let cipher = XChaCha20Poly1305::new((&reply_key(public, mac1)).into());
    packet[30..46].copy_from_slice(cookie);
    let tag = cipher
        .encrypt_in_place_detached(XNonce::from_slice(nonce), &aad, &mut packet[30..46])
        .map_err(|_| CookieError::Authentication)?;
    packet[46..].copy_from_slice(&tag);
    Ok(packet)
}
/// Abrir solo para un intento todavía pendiente en su ruta y respondedor.
/// Un error no muta el intento ni la caché; no confirma endpoint/salud.
pub fn open_reply(
    packet: &[u8],
    expected_index: u32,
    public: &[u8; 32],
    mac1: &[u8; 16],
) -> Result<Zeroizing<[u8; 16]>, CookieError> {
    let proto::Packet::CookieReply {
        receiver_index,
        nonce,
        encrypted_cookie,
    } = proto::parse(packet).map_err(|_| CookieError::InvalidPacket)?
    else {
        return Err(CookieError::InvalidPacket);
    };
    if receiver_index != expected_index {
        return Err(CookieError::InvalidPacket);
    }
    let mut aad = [0; 46];
    aad[..30].copy_from_slice(&packet[..30]);
    aad[30..].copy_from_slice(mac1);
    let cipher = XChaCha20Poly1305::new((&reply_key(public, mac1)).into());
    let mut cookie = Zeroizing::new([0; 16]);
    cookie.copy_from_slice(&encrypted_cookie[..16]);
    cipher
        .decrypt_in_place_detached(
            XNonce::from_slice(nonce),
            &aad,
            cookie.as_mut(),
            chacha20poly1305::Tag::from_slice(&encrypted_cookie[16..]),
        )
        .map_err(|_| CookieError::Authentication)?;
    Ok(cookie)
}

/// Una entrada por candidato, sin tabla dinámica. La clave incluye estática y
/// ruta canónica; se borra al expirar y al descartar el candidato/configuración.
pub struct CachedCookie {
    public: [u8; 32],
    source: Source,
    cookie: Zeroizing<[u8; 16]>,
    received: Instant,
}
impl CachedCookie {
    pub fn new(
        public: [u8; 32],
        source: Source,
        cookie: Zeroizing<[u8; 16]>,
        received: Instant,
    ) -> Self {
        Self {
            public,
            source,
            cookie,
            received,
        }
    }
    pub fn get(&self, public: &[u8; 32], source: &Source, now: Instant) -> Option<&[u8; 16]> {
        (self.public == *public
            && self.source == *source
            && now.saturating_duration_since(self.received) < COOKIE_TTL)
            .then_some(&*self.cookie)
    }
}

#[cfg(test)]
mod tests;
