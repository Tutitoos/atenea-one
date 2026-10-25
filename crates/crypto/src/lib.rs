//! Handshake Noise IK, sesiones con antirreplay y límites de rekey (ADR-001, ADR-003).
//!
//! Estado de sesión solo en memoria: tras reiniciar, handshake nuevo con efímeras nuevas,
//! por lo que nunca se reutiliza (clave, nonce).
pub mod hex;
pub mod replay;

/// Entero aleatorio del sistema (índices de sesión).
pub fn random_u32() -> u32 {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("RNG del sistema");
    u32::from_be_bytes(b)
}

use atenea_protocol as proto;
use replay::ReplayWindow;
use snow::{Builder, HandshakeState, StatelessTransportState};
use std::fmt;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const NOISE_PARAMS: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE_PREFIX: &[u8] = b"atenea-one/v1";

/// Rekey suave (ADR-003): 120 s o 2^32 mensajes.
pub const REKEY_AFTER_TIME: Duration = Duration::from_secs(120);
pub const REKEY_AFTER_MESSAGES: u64 = 1 << 32;
/// Rechazo duro: 180 s o 2^40 mensajes.
pub const REJECT_AFTER_TIME: Duration = Duration::from_secs(180);
pub const REJECT_AFTER_MESSAGES: u64 = 1 << 40;
/// Tolerancia del timestamp del mensaje 1 (ADR-001/002).
pub const TIMESTAMP_TOLERANCE: u64 = 30;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Noise,
    BadPayload,
    IndexMismatch,
    UnknownKey,
    StaleTimestamp,
    Replay,
    AuthFailed,
    Expired,
    NotConfirmed,
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Noise => "handshake Noise inválido",
            Error::BadPayload => "payload de handshake inválido",
            Error::IndexMismatch => "índice externo distinto del autenticado",
            Error::UnknownKey => "clave estática no autorizada",
            Error::StaleTimestamp => "timestamp fuera de ventana o no creciente",
            Error::Replay => "contador repetido o fuera de ventana",
            Error::AuthFailed => "autenticación AEAD fallida",
            Error::Expired => "sesión expirada",
            Error::NotConfirmed => "sesión no confirmada",
            Error::TooLarge => "mensaje demasiado grande",
        })
    }
}

impl std::error::Error for Error {}

pub struct StaticKeypair {
    private: Zeroizing<[u8; 32]>,
    pub public: [u8; 32],
}

impl StaticKeypair {
    pub fn generate() -> Self {
        let kp = Builder::new(NOISE_PARAMS.parse().expect("parámetros fijos"))
            .generate_keypair()
            .expect("RNG del sistema");
        let mut private = Zeroizing::new([0u8; 32]);
        private.copy_from_slice(&kp.private);
        Self::from_private(&private)
    }

    pub fn from_private(private: &[u8; 32]) -> Self {
        let secret = x25519_dalek::StaticSecret::from(*private);
        let public = x25519_dalek::PublicKey::from(&secret).to_bytes();
        Self {
            private: Zeroizing::new(*private),
            public,
        }
    }

    pub fn private_bytes(&self) -> &[u8; 32] {
        &self.private
    }
}

/// Prólogo: versión de protocolo y red (ADR-001).
pub fn prologue(network_id: &[u8; 16]) -> Vec<u8> {
    let mut p = PROLOGUE_PREFIX.to_vec();
    p.extend_from_slice(network_id);
    p
}

/// Crea el estado de handshake; con `remote` es iniciador, sin él, respondedor.
fn handshake(
    local: &StaticKeypair,
    network_id: &[u8; 16],
    remote: Option<&[u8; 32]>,
) -> Result<HandshakeState, Error> {
    let prologue = prologue(network_id);
    let mut b = Builder::new(NOISE_PARAMS.parse().map_err(|_| Error::Noise)?)
        .local_private_key(local.private_bytes())
        .map_err(|_| Error::Noise)?
        .prologue(&prologue)
        .map_err(|_| Error::Noise)?;
    if let Some(r) = remote {
        b = b.remote_public_key(r).map_err(|_| Error::Noise)?;
        b.build_initiator().map_err(|_| Error::Noise)
    } else {
        b.build_responder().map_err(|_| Error::Noise)
    }
}

/// Frescura del mensaje 1: dentro de ±30 s del reloj local y estrictamente mayor que el último aceptado.
pub fn check_timestamp(ts: u64, now_unix: u64, last: Option<u64>) -> Result<(), Error> {
    if ts.abs_diff(now_unix) > TIMESTAMP_TOLERANCE || last.is_some_and(|l| ts <= l) {
        return Err(Error::StaleTimestamp);
    }
    Ok(())
}

pub struct PendingInit {
    /// `None` solo tras consumir un mensaje 2 autenticado.
    hs: Option<HandshakeState>,
    remote_static: [u8; 32],
    local_index: u32,
}

/// Construye el mensaje 1 de Noise (sin cabecera de datagrama).
pub fn initiate(
    local: &StaticKeypair,
    remote_public: &[u8; 32],
    network_id: &[u8; 16],
    local_index: u32,
    unix_ts: u64,
) -> Result<(PendingInit, Vec<u8>), Error> {
    let mut hs = handshake(local, network_id, Some(remote_public))?;
    let mut payload = [0u8; proto::INIT_PAYLOAD_LEN];
    payload[..4].copy_from_slice(&local_index.to_be_bytes());
    payload[4..].copy_from_slice(&unix_ts.to_be_bytes());
    let mut msg = vec![0u8; proto::NOISE_MSG1_LEN];
    let n = hs
        .write_message(&payload, &mut msg)
        .map_err(|_| Error::Noise)?;
    if n != proto::NOISE_MSG1_LEN {
        return Err(Error::Noise);
    }
    Ok((
        PendingInit {
            hs: Some(hs),
            remote_static: *remote_public,
            local_index,
        },
        msg,
    ))
}

impl PendingInit {
    pub fn local_index(&self) -> u32 {
        self.local_index
    }

    /// Procesa el mensaje 2 y crea la sesión (el iniciador queda confirmado).
    /// Un mensaje 2 que no autentica deja intacto el handshake pendiente: quien responde sin
    /// conocer las claves no puede descartar el intento.
    pub fn finish(
        &mut self,
        msg2: &[u8],
        outer_remote_index: u32,
        now: Instant,
    ) -> Result<Session, Error> {
        if msg2.len() != proto::NOISE_MSG2_LEN {
            return Err(Error::BadPayload);
        }
        let hs = self.hs.as_mut().ok_or(Error::Noise)?;
        let mut payload = [0u8; 64];
        let n = hs
            .read_message(msg2, &mut payload)
            .map_err(|_| Error::Noise)?;
        if n != proto::RESP_PAYLOAD_LEN || !hs.is_handshake_finished() {
            return Err(Error::BadPayload);
        }
        let remote_index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
        if remote_index != outer_remote_index {
            return Err(Error::IndexMismatch);
        }
        let transport = self
            .hs
            .take()
            .ok_or(Error::Noise)?
            .into_stateless_transport_mode()
            .map_err(|_| Error::Noise)?;
        Ok(Session::new(
            transport,
            self.local_index,
            remote_index,
            self.remote_static,
            true,
            now,
        ))
    }
}

pub struct Incoming {
    hs: HandshakeState,
    pub remote_static: [u8; 32],
    pub remote_index: u32,
    pub timestamp: u64,
}

/// Lee el mensaje 1. El llamador debe comprobar `remote_static` contra sus peers autorizados y la
/// frescura (`check_timestamp`) **antes** de llamar a `accept`; si no, descartar sin responder.
pub fn read_init(
    local: &StaticKeypair,
    network_id: &[u8; 16],
    msg1: &[u8],
    outer_sender_index: u32,
) -> Result<Incoming, Error> {
    if msg1.len() != proto::NOISE_MSG1_LEN {
        return Err(Error::BadPayload);
    }
    let mut hs = handshake(local, network_id, None)?;
    let mut payload = [0u8; 64];
    let n = hs
        .read_message(msg1, &mut payload)
        .map_err(|_| Error::Noise)?;
    if n != proto::INIT_PAYLOAD_LEN {
        return Err(Error::BadPayload);
    }
    let remote_index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
    if remote_index != outer_sender_index {
        return Err(Error::IndexMismatch);
    }
    let mut ts = [0u8; 8];
    ts.copy_from_slice(&payload[4..12]);
    let rs = hs.get_remote_static().ok_or(Error::Noise)?;
    let remote_static: [u8; 32] = rs.try_into().map_err(|_| Error::Noise)?;
    Ok(Incoming {
        hs,
        remote_static,
        remote_index,
        timestamp: u64::from_be_bytes(ts),
    })
}

impl Incoming {
    /// Genera el mensaje 2 y la sesión (sin confirmar hasta el primer paquete válido del iniciador).
    pub fn accept(mut self, local_index: u32, now: Instant) -> Result<(Session, Vec<u8>), Error> {
        let mut msg = vec![0u8; proto::NOISE_MSG2_LEN];
        let n = self
            .hs
            .write_message(&local_index.to_be_bytes(), &mut msg)
            .map_err(|_| Error::Noise)?;
        if n != proto::NOISE_MSG2_LEN || !self.hs.is_handshake_finished() {
            return Err(Error::Noise);
        }
        let transport = self
            .hs
            .into_stateless_transport_mode()
            .map_err(|_| Error::Noise)?;
        Ok((
            Session::new(
                transport,
                local_index,
                self.remote_index,
                self.remote_static,
                false,
                now,
            ),
            msg,
        ))
    }
}

pub struct Session {
    transport: StatelessTransportState,
    pub local_index: u32,
    pub remote_index: u32,
    pub remote_static: [u8; 32],
    initiator: bool,
    confirmed: bool,
    send_counter: u64,
    window: ReplayWindow,
    created: Instant,
}

impl Session {
    fn new(
        transport: StatelessTransportState,
        local_index: u32,
        remote_index: u32,
        remote_static: [u8; 32],
        initiator: bool,
        now: Instant,
    ) -> Self {
        Self {
            transport,
            local_index,
            remote_index,
            remote_static,
            initiator,
            confirmed: initiator,
            send_counter: 0,
            window: ReplayWindow::new(),
            created: now,
        }
    }

    pub fn is_initiator(&self) -> bool {
        self.initiator
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    pub fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.created)
    }

    /// Debe iniciarse un handshake nuevo.
    pub fn should_rekey(&self, now: Instant) -> bool {
        self.age(now) >= REKEY_AFTER_TIME || self.send_counter >= REKEY_AFTER_MESSAGES
    }

    /// La sesión ya no puede enviar ni recibir.
    pub fn is_expired(&self, now: Instant) -> bool {
        self.age(now) >= REJECT_AFTER_TIME || self.send_counter >= REJECT_AFTER_MESSAGES
    }

    /// Cifra y devuelve el datagrama completo. El contador se consume aunque falle el cifrado.
    /// Un respondedor no envía hasta confirmar la sesión.
    pub fn encrypt(&mut self, payload: &[u8], now: Instant) -> Result<Vec<u8>, Error> {
        if !self.confirmed {
            return Err(Error::NotConfirmed);
        }
        if self.is_expired(now) {
            return Err(Error::Expired);
        }
        if payload.len() > proto::MAX_MTU {
            return Err(Error::TooLarge);
        }
        let counter = self.send_counter;
        self.send_counter += 1;
        let mut out = proto::data_header(self.remote_index, counter).to_vec();
        out.resize(proto::DATA_HEADER_LEN + payload.len() + proto::TAG_LEN, 0);
        let n = self
            .transport
            .write_message(counter, payload, &mut out[proto::DATA_HEADER_LEN..])
            .map_err(|_| Error::Noise)?;
        out.truncate(proto::DATA_HEADER_LEN + n);
        Ok(out)
    }

    /// Orden ADR-003: comprobar ventana → autenticar → actualizar ventana.
    /// Un paquete válido confirma una sesión de respondedor.
    pub fn decrypt(
        &mut self,
        counter: u64,
        ciphertext: &[u8],
        now: Instant,
    ) -> Result<Vec<u8>, Error> {
        if self.age(now) >= REJECT_AFTER_TIME || counter >= REJECT_AFTER_MESSAGES {
            return Err(Error::Expired);
        }
        if ciphertext.len() < proto::TAG_LEN || ciphertext.len() > proto::MAX_MTU + proto::TAG_LEN {
            return Err(Error::TooLarge);
        }
        if !self.window.check(counter) {
            return Err(Error::Replay);
        }
        let mut out = vec![0u8; ciphertext.len()];
        let n = self
            .transport
            .read_message(counter, ciphertext, &mut out)
            .map_err(|_| Error::AuthFailed)?;
        if !self.window.update(counter) {
            return Err(Error::Replay);
        }
        self.confirmed = true;
        out.truncate(n);
        Ok(out)
    }

    #[cfg(test)]
    fn set_send_counter(&mut self, c: u64) {
        self.send_counter = c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atenea_protocol::{Packet, parse};

    const NET: [u8; 16] = [7; 16];

    fn pair() -> (Session, Session, StaticKeypair, StaticKeypair) {
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        let now = Instant::now();
        let (mut pending, msg1) = initiate(&a, &b.public, &NET, 11, 1000).unwrap();
        let inc = read_init(&b, &NET, &msg1, 11).unwrap();
        assert_eq!(inc.remote_static, a.public);
        assert_eq!(inc.timestamp, 1000);
        let (sb, msg2) = inc.accept(22, now).unwrap();
        let sa = pending.finish(&msg2, 22, now).unwrap();
        (sa, sb, a, b)
    }

    fn data(dg: &[u8]) -> (u32, u64, Vec<u8>) {
        match parse(dg).unwrap() {
            Packet::Data {
                receiver_index,
                counter,
                ciphertext,
            } => (receiver_index, counter, ciphertext.to_vec()),
            _ => panic!("no es datos"),
        }
    }

    #[test]
    fn public_key_derivation_matches_snow() {
        let kp = StaticKeypair::generate();
        let again = StaticKeypair::from_private(kp.private_bytes());
        assert_eq!(kp.public, again.public);
    }

    #[test]
    fn roundtrip_and_confirmation() {
        let (mut a, mut b, _, _) = pair();
        let now = Instant::now();
        // el respondedor no envía hasta recibir algo válido
        assert_eq!(b.encrypt(b"x", now), Err(Error::NotConfirmed));
        let dg = a.encrypt(b"hola", now).unwrap();
        assert_eq!(dg.len(), proto::DATA_OVERHEAD + 4);
        let (ri, c, ct) = data(&dg);
        assert_eq!(ri, 22);
        assert_eq!(c, 0);
        assert_eq!(b.decrypt(c, &ct, now).unwrap(), b"hola");
        assert!(b.is_confirmed());
        let back = b.encrypt(b"adios", now).unwrap();
        let (ri, c, ct) = data(&back);
        assert_eq!(ri, 11);
        assert_eq!(a.decrypt(c, &ct, now).unwrap(), b"adios");
    }

    #[test]
    fn keepalive_is_empty_payload() {
        let (mut a, mut b, _, _) = pair();
        let now = Instant::now();
        let (_, c, ct) = data(&a.encrypt(b"", now).unwrap());
        assert_eq!(ct.len(), proto::TAG_LEN);
        assert!(b.decrypt(c, &ct, now).unwrap().is_empty());
    }

    #[test]
    fn replay_reorder_and_tamper() {
        let (mut a, mut b, _, _) = pair();
        let now = Instant::now();
        let d: Vec<_> = (0..5)
            .map(|i| data(&a.encrypt(&[i; 20], now).unwrap()))
            .collect();
        // desorden
        for i in [3usize, 0, 4, 1, 2] {
            assert_eq!(b.decrypt(d[i].1, &d[i].2, now).unwrap(), vec![i as u8; 20]);
        }
        // replay
        assert_eq!(b.decrypt(d[2].1, &d[2].2, now), Err(Error::Replay));
        // alteración: no autentica y no mueve la ventana
        let (_, c, ct) = data(&a.encrypt(b"fresco", now).unwrap());
        let mut bad = ct.clone();
        bad[0] ^= 1;
        assert_eq!(b.decrypt(c, &bad, now), Err(Error::AuthFailed));
        assert_eq!(b.decrypt(c, &ct, now).unwrap(), b"fresco");
        // contador alterado
        let (_, c, ct) = data(&a.encrypt(b"otro", now).unwrap());
        assert_eq!(b.decrypt(c + 1, &ct, now), Err(Error::AuthFailed));
        assert_eq!(b.decrypt(c, &ct, now).unwrap(), b"otro");
    }

    #[test]
    fn oversize_is_rejected() {
        let (mut a, mut b, _, _) = pair();
        let now = Instant::now();
        assert_eq!(
            a.encrypt(&vec![0u8; proto::MAX_MTU + 1], now),
            Err(Error::TooLarge)
        );
        assert_eq!(
            b.decrypt(0, &vec![0u8; proto::MAX_MTU + proto::TAG_LEN + 1], now),
            Err(Error::TooLarge)
        );
    }

    #[test]
    fn wrong_responder_key_fails() {
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        let evil = StaticKeypair::generate();
        let (_, msg1) = initiate(&a, &b.public, &NET, 1, 1).unwrap();
        assert!(read_init(&evil, &NET, &msg1, 1).is_err());
    }

    #[test]
    fn other_network_or_tampered_init_fails() {
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        let (_, msg1) = initiate(&a, &b.public, &NET, 1, 1).unwrap();
        assert!(read_init(&b, &[8; 16], &msg1, 1).is_err());
        let mut bad = msg1.clone();
        bad[40] ^= 1;
        assert!(read_init(&b, &NET, &bad, 1).is_err());
        assert_eq!(
            read_init(&b, &NET, &msg1, 2).err(),
            Some(Error::IndexMismatch)
        );
    }

    #[test]
    fn tampered_resp_fails() {
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        let now = Instant::now();
        let (mut p, msg1) = initiate(&a, &b.public, &NET, 1, 1).unwrap();
        let (_, mut msg2) = read_init(&b, &NET, &msg1, 1)
            .unwrap()
            .accept(2, now)
            .unwrap();
        msg2[10] ^= 1;
        assert!(p.finish(&msg2, 2, now).is_err());
        // Un mensaje 2 ajeno no consume el handshake: el legítimo aún completa.
        let (_, ok2) = read_init(&b, &NET, &msg1, 1)
            .unwrap()
            .accept(2, now)
            .unwrap();
        assert!(p.finish(&ok2, 2, now).is_ok());
    }

    #[test]
    fn resp_index_must_match_authenticated_one() {
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        let now = Instant::now();
        let (mut p, msg1) = initiate(&a, &b.public, &NET, 1, 1).unwrap();
        let (_, msg2) = read_init(&b, &NET, &msg1, 1)
            .unwrap()
            .accept(2, now)
            .unwrap();
        assert_eq!(p.finish(&msg2, 3, now).err(), Some(Error::IndexMismatch));
    }

    #[test]
    fn new_sessions_never_share_keys_or_nonce_space() {
        // Mismas claves estáticas, dos handshakes: mismo plaintext y contador, distinto cifrado.
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        let now = Instant::now();
        let mk = || {
            let (mut p, m1) = initiate(&a, &b.public, &NET, 1, 1).unwrap();
            let (_, m2) = read_init(&b, &NET, &m1, 1).unwrap().accept(2, now).unwrap();
            p.finish(&m2, 2, now).unwrap()
        };
        let (mut s1, mut s2) = (mk(), mk());
        let c1 = data(&s1.encrypt(b"mismo", now).unwrap());
        let c2 = data(&s2.encrypt(b"mismo", now).unwrap());
        assert_eq!(c1.1, c2.1);
        assert_ne!(c1.2, c2.2);
    }

    #[test]
    fn sender_counter_never_repeats_and_hard_limits_apply() {
        let (mut a, _, _, _) = pair();
        let now = Instant::now();
        let c0 = data(&a.encrypt(b"a", now).unwrap()).1;
        let c1 = data(&a.encrypt(b"a", now).unwrap()).1;
        assert_eq!((c0, c1), (0, 1));
        a.set_send_counter(REKEY_AFTER_MESSAGES);
        assert!(a.should_rekey(now));
        assert!(a.encrypt(b"a", now).is_ok());
        a.set_send_counter(REJECT_AFTER_MESSAGES);
        assert_eq!(a.encrypt(b"a", now), Err(Error::Expired));
        let (mut a, _, _, _) = pair();
        let later = Instant::now() + REJECT_AFTER_TIME;
        assert!(a.is_expired(later));
        assert_eq!(a.encrypt(b"a", later), Err(Error::Expired));
        assert!(a.should_rekey(Instant::now() + REKEY_AFTER_TIME));
    }

    #[test]
    fn timestamp_freshness() {
        assert!(check_timestamp(1000, 1000, None).is_ok());
        assert!(check_timestamp(1030, 1000, None).is_ok());
        assert_eq!(
            check_timestamp(1031, 1000, None),
            Err(Error::StaleTimestamp)
        );
        assert_eq!(check_timestamp(969, 1000, None), Err(Error::StaleTimestamp));
        assert_eq!(
            check_timestamp(1000, 1000, Some(1000)),
            Err(Error::StaleTimestamp)
        );
        assert!(check_timestamp(1001, 1000, Some(1000)).is_ok());
    }
}
