//! Máquina de estados del agente: sin E/S. Recibe eventos y devuelve acciones.

use crate::packet::parse_ip;
use atenea_crypto::{self as crypto, PendingInit, Session, StaticKeypair};
use atenea_identity::{CLOCK_TOLERANCE_SECS, DeviceConfig, DeviceId, NetworkId};
use atenea_policies::{Policy, classify};
use atenea_protocol::{self as proto, Packet, ParseError};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

/// Reintento de handshake y caducidad de un handshake sin respuesta.
const HANDSHAKE_RETRY: Duration = Duration::from_secs(5);
/// Un paquete en cola espera como máximo este tiempo a que haya sesión.
const QUEUE_TTL: Duration = Duration::from_secs(10);
/// Solo se renegocia una sesión que está cerca de caducar si hubo tráfico reciente.
const ACTIVE_WINDOW: Duration = Duration::from_secs(15);
/// Máximo de mensajes 1 procesados por segundo (cada uno cuesta operaciones DH).
const INIT_RATE: f64 = 50.0;

pub enum Out {
    Udp(SocketAddr, Vec<u8>),
    Tun(Vec<u8>),
}

#[derive(Default)]
struct Counters(BTreeMap<&'static str, u64>);

impl Counters {
    fn inc(&mut self, k: &'static str) {
        *self.0.entry(k).or_insert(0) += 1;
    }
}

struct Limiter {
    tokens: f64,
    last: Instant,
}

impl Limiter {
    fn allow(&mut self, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * INIT_RATE).min(INIT_RATE);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

struct Peer {
    device_id: DeviceId,
    public: [u8; 32],
    addrs: Vec<IpAddr>,
    endpoint: Option<SocketAddr>,
    current: Option<Session>,
    previous: Option<Session>,
    /// Sesión de respondedor aún sin confirmar por un paquete válido del iniciador.
    next: Option<Session>,
    pending: Option<(PendingInit, Instant)>,
    last_attempt: Option<Instant>,
    queued: Option<(Vec<u8>, Instant)>,
    last_init_ts: Option<u64>,
    last_handshake: Option<Instant>,
    last_data: Option<Instant>,
    tx_bytes: u64,
    rx_bytes: u64,
}

impl Peer {
    fn new(device_id: DeviceId, public: [u8; 32]) -> Peer {
        Peer {
            device_id,
            public,
            addrs: Vec::new(),
            endpoint: None,
            current: None,
            previous: None,
            next: None,
            pending: None,
            last_attempt: None,
            queued: None,
            last_init_ts: None,
            last_handshake: None,
            last_data: None,
            tx_bytes: 0,
            rx_bytes: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Current,
    Next,
    Previous,
}

impl Peer {
    fn session(&mut self, slot: Slot) -> Option<&mut Session> {
        match slot {
            Slot::Current => self.current.as_mut(),
            Slot::Next => self.next.as_mut(),
            Slot::Previous => self.previous.as_mut(),
        }
    }

    fn slot_of(&self, index: u32) -> Option<Slot> {
        let has = |s: &Option<Session>| s.as_ref().is_some_and(|s| s.local_index == index);
        if has(&self.current) {
            Some(Slot::Current)
        } else if has(&self.next) {
            Some(Slot::Next)
        } else if has(&self.previous) {
            Some(Slot::Previous)
        } else {
            None
        }
    }

    fn usable_current(&self, now: Instant) -> bool {
        self.current.as_ref().is_some_and(|s| !s.is_expired(now))
    }
}

#[derive(Serialize)]
pub struct PeerStatus {
    pub public_key: String,
    pub endpoint: Option<String>,
    pub session: Option<String>,
    pub last_handshake_secs_ago: Option<u64>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Serialize)]
pub struct ConfigStatus {
    pub version: u64,
    pub valid_until_unix: Option<u64>,
    pub lease_active: bool,
    pub last_error: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Status {
    pub public_key: String,
    pub counters: BTreeMap<&'static str, u64>,
    pub config: ConfigStatus,
    pub peers: Vec<PeerStatus>,
}

pub struct Engine {
    net: NetworkId,
    keys: StaticKeypair,
    own: Vec<IpAddr>,
    peers: Vec<Peer>,
    by_addr: HashMap<IpAddr, usize>,
    by_key: HashMap<[u8; 32], usize>,
    by_index: HashMap<u32, usize>,
    limiter: Limiter,
    counters: Counters,
    policy: Option<Policy>,
    /// Ventana (not_before, not_after) de la configuración vigente; sin ella todo se deniega.
    lease: Option<(u64, u64)>,
    config_version: u64,
    last_config_error: Option<&'static str>,
}

impl Engine {
    /// Sin configuración: deniega todo hasta `apply_config` (fail-closed, ADR-002).
    pub fn new(net: NetworkId, keys: StaticKeypair, now: Instant) -> Engine {
        Engine {
            net,
            keys,
            own: Vec::new(),
            peers: Vec::new(),
            by_addr: HashMap::new(),
            by_key: HashMap::new(),
            by_index: HashMap::new(),
            limiter: Limiter {
                tokens: INIT_RATE,
                last: now,
            },
            counters: Counters::default(),
            policy: None,
            lease: None,
            config_version: 0,
            last_config_error: None,
        }
    }

    /// Aplica una configuración ya verificada y persistida. Conserva las sesiones de los peers cuya
    /// clave no cambió; destruye las de peers eliminados o con clave nueva (ADR-002).
    pub fn apply_config(&mut self, cfg: &DeviceConfig) {
        let mut old = std::mem::take(&mut self.peers);
        for p in &cfg.peers {
            let mut peer = match old
                .iter()
                .position(|o| o.device_id == p.device_id && o.public == p.noise_pubkey)
            {
                Some(i) => old.swap_remove(i),
                None => Peer::new(p.device_id, p.noise_pubkey),
            };
            peer.addrs = p.addrs.clone();
            if peer.endpoint.is_none() {
                peer.endpoint = p.endpoints.first().copied();
            }
            self.peers.push(peer);
        }
        self.own = cfg.tunnel_addrs.clone();
        self.policy = Some(Policy::new(cfg));
        self.lease = Some((cfg.not_before, cfg.not_after));
        self.config_version = cfg.config_version;
        self.last_config_error = None;
        self.reindex();
        self.counters.inc("config_applied");
    }

    /// Una configuración rechazada no revoca la vigente (sigue hasta su lease).
    pub fn note_config_error(&mut self, code: &'static str) {
        self.counters.inc("config_rejected");
        self.last_config_error = Some(code);
    }

    /// Dentro de `[not_before - 30, not_after + 30)` (ADR-002).
    pub fn lease_active(&self, unix: u64) -> bool {
        self.lease.is_some_and(|(nb, na)| {
            unix + CLOCK_TOLERANCE_SECS >= nb && unix < na + CLOCK_TOLERANCE_SECS
        })
    }

    fn reindex(&mut self) {
        self.by_addr.clear();
        self.by_key.clear();
        self.by_index.clear();
        for (pi, p) in self.peers.iter().enumerate() {
            for a in &p.addrs {
                self.by_addr.insert(*a, pi);
            }
            self.by_key.insert(p.public, pi);
            for s in [&p.current, &p.previous, &p.next].into_iter().flatten() {
                self.by_index.insert(s.local_index, pi);
            }
            if let Some((pend, _)) = &p.pending {
                self.by_index.insert(pend.local_index(), pi);
            }
        }
    }

    /// Destruye todas las sesiones y estado de handshake (lease caducado).
    fn deny_all(&mut self) -> bool {
        let mut had = false;
        for p in &mut self.peers {
            had |= p.current.is_some()
                || p.previous.is_some()
                || p.next.is_some()
                || p.pending.is_some();
            p.current = None;
            p.previous = None;
            p.next = None;
            p.pending = None;
            p.queued = None;
            p.last_attempt = None;
        }
        self.by_index.clear();
        had
    }

    fn fresh_index(&self) -> u32 {
        loop {
            let i = crypto::random_u32();
            if i != 0 && !self.by_index.contains_key(&i) {
                return i;
            }
        }
    }

    /// Paquete IP leído de la TUN.
    pub fn on_tun(&mut self, pkt: &[u8], now: Instant, unix: u64) -> Vec<Out> {
        let mut out = Vec::new();
        if !self.lease_active(unix) {
            self.counters.inc("denied_no_lease");
            return out;
        }
        let Some((src, dst, total)) = parse_ip(pkt) else {
            self.counters.inc("tx_malformed");
            return out;
        };
        if !self.own.contains(&src) {
            self.counters.inc("tx_bad_source");
            return out;
        }
        let Some(&pi) = self.by_addr.get(&dst) else {
            self.counters.inc("tx_no_route");
            return out;
        };
        let pkt = &pkt[..total];
        if !self
            .policy
            .as_ref()
            .is_some_and(|p| classify(pkt).is_some_and(|f| p.allows(&f)))
        {
            self.counters.inc("tx_acl_denied");
            return out;
        }
        let peer = &mut self.peers[pi];
        peer.last_data = Some(now);
        if peer.usable_current(now)
            && let Some(ep) = peer.endpoint
            && let Some(s) = peer.current.as_mut()
        {
            match s.encrypt(pkt, now) {
                Ok(dg) => {
                    peer.tx_bytes += pkt.len() as u64;
                    out.push(Out::Udp(ep, dg));
                    return out;
                }
                Err(crypto::Error::NotConfirmed) => {}
                Err(_) => self.counters.inc("tx_encrypt_failed"),
            }
        }
        // Sin sesión utilizable: se guarda el último paquete y se inicia handshake.
        let peer = &mut self.peers[pi];
        peer.queued = Some((pkt.to_vec(), now));
        self.maybe_initiate(pi, now, unix, &mut out);
        out
    }

    fn maybe_initiate(&mut self, pi: usize, now: Instant, unix: u64, out: &mut Vec<Out>) {
        let index = self.fresh_index();
        let peer = &mut self.peers[pi];
        let Some(ep) = peer.endpoint else { return };
        if peer.pending.is_some()
            || peer
                .last_attempt
                .is_some_and(|t| now.duration_since(t) < HANDSHAKE_RETRY)
        {
            return;
        }
        let Ok((pending, msg)) = crypto::initiate(&self.keys, &peer.public, &self.net, index, unix)
        else {
            self.counters.inc("tx_init_failed");
            return;
        };
        let Ok(dg) = proto::encode_init(index, &msg) else {
            return;
        };
        peer.pending = Some((pending, now));
        peer.last_attempt = Some(now);
        self.by_index.insert(index, pi);
        self.counters.inc("tx_init");
        out.push(Out::Udp(ep, dg));
    }

    /// Datagrama UDP recibido.
    pub fn on_udp(&mut self, from: SocketAddr, dg: &[u8], now: Instant, unix: u64) -> Vec<Out> {
        let mut out = Vec::new();
        if !self.lease_active(unix) {
            self.counters.inc("denied_no_lease");
            return out;
        }
        match proto::parse(dg) {
            Err(ParseError::TooLarge) => self.counters.inc("rx_oversize"),
            Err(_) => self.counters.inc("rx_malformed"),
            Ok(Packet::Init {
                sender_index,
                noise,
            }) => self.on_init(from, sender_index, noise, now, unix, &mut out),
            Ok(Packet::Resp {
                sender_index,
                receiver_index,
                noise,
            }) => self.on_resp(from, sender_index, receiver_index, noise, now, &mut out),
            Ok(Packet::Data {
                receiver_index,
                counter,
                ciphertext,
            }) => self.on_data(from, receiver_index, counter, ciphertext, now, &mut out),
        }
        out
    }

    fn on_init(
        &mut self,
        from: SocketAddr,
        sender_index: u32,
        noise: &[u8],
        now: Instant,
        unix: u64,
        out: &mut Vec<Out>,
    ) {
        if !self.limiter.allow(now) {
            self.counters.inc("rx_init_ratelimited");
            return;
        }
        let Ok(inc) = crypto::read_init(&self.keys, &self.net, noise, sender_index) else {
            self.counters.inc("rx_init_invalid");
            return;
        };
        // Autorización antes de asignar recursos; sin respuesta para desconocidos (ADR-001).
        let Some(&pi) = self.by_key.get(&inc.remote_static) else {
            self.counters.inc("rx_init_unknown_peer");
            return;
        };
        let index = self.fresh_index();
        let peer = &mut self.peers[pi];
        if crypto::check_timestamp(inc.timestamp, unix, peer.last_init_ts).is_err() {
            self.counters.inc("rx_init_stale");
            return;
        }
        // Iniciación simultánea: gana como iniciador la clave pública menor.
        if peer.pending.is_some() && self.keys.public < peer.public {
            self.counters.inc("rx_init_tiebreak");
            return;
        }
        let (remote_index, ts) = (inc.remote_index, inc.timestamp);
        let Ok((session, msg2)) = inc.accept(index, now) else {
            self.counters.inc("rx_init_invalid");
            return;
        };
        let Ok(resp) = proto::encode_resp(index, remote_index, &msg2) else {
            return;
        };
        peer.pending = None;
        peer.next = Some(session);
        peer.last_init_ts = Some(ts);
        peer.endpoint = Some(from);
        self.by_index.insert(index, pi);
        self.counters.inc("rx_init");
        out.push(Out::Udp(from, resp));
    }

    fn on_resp(
        &mut self,
        from: SocketAddr,
        sender_index: u32,
        receiver_index: u32,
        noise: &[u8],
        now: Instant,
        out: &mut Vec<Out>,
    ) {
        let Some(&pi) = self.by_index.get(&receiver_index) else {
            self.counters.inc("rx_resp_unknown");
            return;
        };
        let peer = &mut self.peers[pi];
        if peer.pending.as_ref().map(|(p, _)| p.local_index()) != Some(receiver_index) {
            self.counters.inc("rx_resp_unknown");
            return;
        }
        let (pending, _) = peer.pending.take().expect("comprobado arriba");
        let Ok(session) = pending.finish(noise, sender_index, now) else {
            self.counters.inc("rx_resp_invalid");
            return;
        };
        peer.previous = peer.current.take();
        peer.current = Some(session);
        peer.endpoint = Some(from);
        peer.last_handshake = Some(now);
        self.counters.inc("handshakes_initiator");
        // Keepalive inmediato: confirma la sesión en el respondedor.
        let s = peer.current.as_mut().expect("recién creada");
        if let Ok(dg) = s.encrypt(&[], now) {
            out.push(Out::Udp(from, dg));
        }
        if let Some((pkt, at)) = peer.queued.take()
            && now.duration_since(at) < QUEUE_TTL
            && let Ok(dg) = peer
                .current
                .as_mut()
                .expect("recién creada")
                .encrypt(&pkt, now)
        {
            peer.tx_bytes += pkt.len() as u64;
            out.push(Out::Udp(from, dg));
        }
    }

    fn on_data(
        &mut self,
        from: SocketAddr,
        receiver_index: u32,
        counter: u64,
        ct: &[u8],
        now: Instant,
        out: &mut Vec<Out>,
    ) {
        let Some(&pi) = self.by_index.get(&receiver_index) else {
            self.counters.inc("rx_unknown_index");
            return;
        };
        let peer = &mut self.peers[pi];
        let Some(slot) = peer.slot_of(receiver_index) else {
            self.counters.inc("rx_unknown_index");
            return;
        };
        let res = peer
            .session(slot)
            .expect("slot válido")
            .decrypt(counter, ct, now);
        let plain = match res {
            Ok(p) => p,
            Err(e) => {
                self.counters.inc(match e {
                    crypto::Error::Replay => "rx_replay",
                    crypto::Error::AuthFailed => "rx_auth_fail",
                    crypto::Error::Expired => "rx_expired",
                    crypto::Error::TooLarge => "rx_oversize",
                    _ => "rx_other",
                });
                return;
            }
        };
        // Primer paquete válido de una sesión de respondedor: se promueve.
        if slot == Slot::Next {
            peer.previous = peer.current.take();
            peer.current = peer.next.take();
            peer.last_handshake = Some(now);
            self.counters.inc("handshakes_responder");
        }
        // Roaming: el endpoint solo cambia tras un paquete autenticado (ADR-003).
        if peer.endpoint != Some(from) {
            peer.endpoint = Some(from);
            self.counters.inc("endpoint_updates");
        }
        peer.last_data = Some(now);
        // Con sesión confirmada, un paquete en cola esperando ya puede salir.
        if peer.queued.is_some()
            && let Some(ep) = peer.endpoint
            && let Some(s) = peer.current.as_mut()
            && s.is_confirmed()
            && let Some((q, at)) = peer.queued.take()
            && now.duration_since(at) < QUEUE_TTL
            && let Ok(dg) = s.encrypt(&q, now)
        {
            peer.tx_bytes += q.len() as u64;
            out.push(Out::Udp(ep, dg));
        }
        if plain.is_empty() {
            self.counters.inc("rx_keepalive");
            return;
        }
        let Some((src, dst, total)) = parse_ip(&plain) else {
            self.counters.inc("rx_bad_packet");
            return;
        };
        if !peer.addrs.contains(&src) {
            self.counters.inc("rx_bad_source");
            return;
        }
        if !self.own.contains(&dst) {
            self.counters.inc("rx_bad_destination");
            return;
        }
        if !self
            .policy
            .as_ref()
            .is_some_and(|p| classify(&plain[..total]).is_some_and(|f| p.allows(&f)))
        {
            self.counters.inc("rx_acl_denied");
            return;
        }
        peer.rx_bytes += total as u64;
        let mut pkt = plain;
        pkt.truncate(total);
        out.push(Out::Tun(pkt));
    }

    /// Temporizador (~1 s): caducidad, reintentos y rekey.
    pub fn on_tick(&mut self, now: Instant, unix: u64) -> Vec<Out> {
        let mut out = Vec::new();
        if !self.lease_active(unix) {
            // Lease caducado: fail-closed, sin sesiones hasta recibir una configuración vigente.
            if self.deny_all() {
                self.counters.inc("lease_expired");
            }
            return out;
        }
        for pi in 0..self.peers.len() {
            let peer = &mut self.peers[pi];
            for slot in [&mut peer.current, &mut peer.previous, &mut peer.next] {
                if slot.as_ref().is_some_and(|s| s.is_expired(now)) {
                    *slot = None;
                }
            }
            if peer
                .pending
                .as_ref()
                .is_some_and(|(_, at)| now.duration_since(*at) >= HANDSHAKE_RETRY)
            {
                peer.pending = None;
            }
            if peer
                .queued
                .as_ref()
                .is_some_and(|(_, at)| now.duration_since(*at) >= QUEUE_TTL)
            {
                peer.queued = None;
            }
            let active = peer
                .last_data
                .is_some_and(|t| now.duration_since(t) < ACTIVE_WINDOW);
            let stale = peer.current.as_ref().is_none_or(|s| s.should_rekey(now));
            let need = peer.queued.is_some() || (active && stale);
            if need {
                self.maybe_initiate(pi, now, unix, &mut out);
            }
        }
        self.reindex();
        out
    }

    pub fn status(&self, now: Instant, unix: u64) -> Status {
        Status {
            public_key: crypto::hex::encode(&self.keys.public),
            counters: self.counters.0.clone(),
            config: ConfigStatus {
                version: self.config_version,
                valid_until_unix: self.lease.map(|(_, na)| na),
                lease_active: self.lease_active(unix),
                last_error: self.last_config_error,
            },
            peers: self
                .peers
                .iter()
                .map(|p| PeerStatus {
                    public_key: crypto::hex::encode(&p.public),
                    endpoint: p.endpoint.map(|e| e.to_string()),
                    session: p
                        .current
                        .as_ref()
                        .map(|s| format!("activa {}s", s.age(now).as_secs())),
                    last_handshake_secs_ago: p
                        .last_handshake
                        .map(|t| now.duration_since(t).as_secs()),
                    tx_bytes: p.tx_bytes,
                    rx_bytes: p.rx_bytes,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{build_ipv4, build_ipv6};
    use std::net::{Ipv4Addr, Ipv6Addr};

    const A_EP: &str = "192.0.2.1:51820";
    const B_EP: &str = "192.0.2.2:51820";

    use atenea_identity::{Peer as CfgPeer, Prefix, Rule, Selector};

    const NET: NetworkId = [7; 16];
    const NOT_BEFORE: u64 = 999_000;
    const NOT_AFTER: u64 = 1_100_000;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn allow_all() -> Vec<Rule> {
        vec![Rule {
            src: Selector::Any,
            dst: Selector::Any,
            proto: 0,
            port_min: 0,
            port_max: 0,
        }]
    }

    #[allow(clippy::too_many_arguments)]
    fn dev_cfg(
        own_id: u8,
        own4: &str,
        own6: &str,
        peer_id: u8,
        peer_pub: &[u8; 32],
        peer4: &str,
        peer6: &str,
        peer_ep: &str,
        rules: Vec<Rule>,
    ) -> DeviceConfig {
        DeviceConfig {
            network_id: NET,
            device_id: [own_id; 16],
            issuer_serial: 1,
            config_version: 1,
            not_before: NOT_BEFORE,
            not_after: NOT_AFTER,
            noise_pubkey: [0; 32],
            identity_pubkey: [0; 32],
            mtu: 1400,
            tunnel_addrs: vec![ip(own4), ip(own6)],
            network_prefixes: vec![
                Prefix {
                    addr: ip("10.77.0.0"),
                    len: 24,
                },
                Prefix {
                    addr: ip("fd77::"),
                    len: 64,
                },
            ],
            peers: vec![CfgPeer {
                device_id: [peer_id; 16],
                noise_pubkey: *peer_pub,
                addrs: vec![ip(peer4), ip(peer6)],
                endpoints: vec![ep(peer_ep)],
            }],
            rules,
        }
    }

    fn mk(cfg: &DeviceConfig, keys: StaticKeypair, now: Instant) -> Engine {
        let mut e = Engine::new(NET, keys, now);
        e.apply_config(cfg);
        e
    }

    struct Lab {
        a: Engine,
        b: Engine,
        cfg_a: DeviceConfig,
        cfg_b: DeviceConfig,
        now: Instant,
        unix: u64,
    }

    fn lab_with(rules_a: Vec<Rule>, rules_b: Vec<Rule>) -> Lab {
        let (ka, kb) = (StaticKeypair::generate(), StaticKeypair::generate());
        let now = Instant::now();
        let cfg_a = dev_cfg(
            1,
            "10.77.0.1",
            "fd77::1",
            2,
            &kb.public,
            "10.77.0.2",
            "fd77::2",
            B_EP,
            rules_a,
        );
        let cfg_b = dev_cfg(
            2,
            "10.77.0.2",
            "fd77::2",
            1,
            &ka.public,
            "10.77.0.1",
            "fd77::1",
            A_EP,
            rules_b,
        );
        Lab {
            a: mk(&cfg_a, ka, now),
            b: mk(&cfg_b, kb, now),
            cfg_a,
            cfg_b,
            now,
            unix: 1_000_000,
        }
    }

    fn lab() -> Lab {
        lab_with(allow_all(), allow_all())
    }

    fn ep(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn v4(a: u8, b: u8, data: &[u8]) -> Vec<u8> {
        build_ipv4(
            Ipv4Addr::new(10, 77, 0, a),
            Ipv4Addr::new(10, 77, 0, b),
            data,
        )
    }

    /// Entrega acciones entre A y B hasta que no quede nada; devuelve lo entregado a cada TUN.
    fn pump(
        l: &mut Lab,
        mut from_a: Vec<Out>,
        mut from_b: Vec<Out>,
    ) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
        let (mut tun_a, mut tun_b) = (vec![], vec![]);
        for _ in 0..20 {
            if from_a.is_empty() && from_b.is_empty() {
                break;
            }
            let (mut na, mut nb) = (vec![], vec![]);
            for o in from_a.drain(..) {
                match o {
                    Out::Udp(_, dg) => nb.extend(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix)),
                    Out::Tun(p) => tun_a.push(p),
                }
            }
            for o in from_b.drain(..) {
                match o {
                    Out::Udp(_, dg) => na.extend(l.a.on_udp(ep(B_EP), &dg, l.now, l.unix)),
                    Out::Tun(p) => tun_b.push(p),
                }
            }
            from_a = na;
            from_b = nb;
        }
        (tun_a, tun_b)
    }

    #[test]
    fn first_packet_triggers_handshake_and_is_delivered() {
        let mut l = lab();
        let pkt = v4(1, 2, b"hola");
        let out = l.a.on_tun(&pkt, l.now, l.unix);
        assert_eq!(out.len(), 1); // solo el handshake; el paquete queda en cola
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b, vec![pkt]);
        // sesión establecida en ambos lados; tráfico en sentido contrario
        let reply = v4(2, 1, b"adios");
        let out = l.b.on_tun(&reply, l.now, l.unix);
        let (tun_a, _) = pump(&mut l, vec![], out);
        assert_eq!(tun_a, vec![reply]);
        assert_eq!(l.a.counters.0["handshakes_initiator"], 1);
        assert_eq!(l.b.counters.0["handshakes_responder"], 1);
    }

    #[test]
    fn ipv6_flows_too() {
        let mut l = lab();
        let pkt = build_ipv6(
            "fd77::1".parse::<Ipv6Addr>().unwrap(),
            "fd77::2".parse().unwrap(),
            b"v6",
        );
        let out = l.a.on_tun(&pkt, l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b, vec![pkt]);
    }

    #[test]
    fn source_spoofing_is_dropped_both_ways() {
        let mut l = lab();
        // salida: origen que no es nuestro
        assert!(l.a.on_tun(&v4(9, 2, b"x"), l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["tx_bad_source"], 1);
        // entrada: B cifra un paquete con origen que no pertenece a A
        let out = l.a.on_tun(&v4(1, 2, b"ok"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let forged = v4(5, 1, b"x");
        let s = l.b.peers[0].current.as_mut().unwrap();
        let dg = s.encrypt(&forged, l.now).unwrap();
        assert!(l.a.on_udp(ep(B_EP), &dg, l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["rx_bad_source"], 1);
    }

    #[test]
    fn replay_and_tamper_are_counted_not_delivered() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"uno"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let out = l.a.on_tun(&v4(1, 2, b"dos"), l.now, l.unix);
        let Out::Udp(_, dg) = &out[0] else { panic!() };
        assert_eq!(l.b.on_udp(ep(A_EP), dg, l.now, l.unix).len(), 1);
        assert!(l.b.on_udp(ep(A_EP), dg, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_replay"], 1);
        let out3 = l.a.on_tun(&v4(1, 2, b"tres"), l.now, l.unix);
        let Out::Udp(_, dg3) = &out3[0] else { panic!() };
        let mut bad = dg3.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(l.b.on_udp(ep(A_EP), &bad, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_auth_fail"], 1);
        // el original sin alterar sigue siendo aceptable: el fallo no movió la ventana
        assert_eq!(l.b.on_udp(ep(A_EP), dg3, l.now, l.unix).len(), 1);
        assert_eq!(l.b.counters.0["rx_auth_fail"], 1);
        assert!(
            l.b.on_udp(ep(A_EP), &vec![1u8; proto::MAX_DATAGRAM + 1], l.now, l.unix)
                .is_empty()
        );
        assert_eq!(l.b.counters.0["rx_oversize"], 1);
        assert!(l.b.on_udp(ep(A_EP), &[9, 9, 9], l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_malformed"], 1);
    }

    #[test]
    fn unknown_initiator_gets_no_response() {
        let mut l = lab();
        let stranger = StaticKeypair::generate();
        let (_, msg) = crypto::initiate(&stranger, &l.b.keys.public, &l.b.net, 5, l.unix).unwrap();
        let dg = proto::encode_init(5, &msg).unwrap();
        assert!(
            l.b.on_udp(ep("198.51.100.9:1"), &dg, l.now, l.unix)
                .is_empty()
        );
        assert_eq!(l.b.counters.0["rx_init_unknown_peer"], 1);
    }

    #[test]
    fn stale_and_repeated_init_rejected() {
        let mut l = lab();
        let (_, msg) =
            crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 5, l.unix - 100).unwrap();
        let dg = proto::encode_init(5, &msg).unwrap();
        assert!(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_init_stale"], 1);
        let (_, msg) = crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 6, l.unix).unwrap();
        let dg = proto::encode_init(6, &msg).unwrap();
        assert_eq!(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix).len(), 1);
        // el mismo mensaje 1 repetido ya no es creciente
        assert!(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_init_stale"], 2);
    }

    #[test]
    fn init_flood_is_rate_limited() {
        let mut l = lab();
        let (_, msg) = crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 5, l.unix).unwrap();
        let dg = proto::encode_init(5, &msg).unwrap();
        for _ in 0..200 {
            l.b.on_udp(ep(A_EP), &dg, l.now, l.unix);
        }
        assert!(l.b.counters.0["rx_init_ratelimited"] >= 140);
    }

    #[test]
    fn restart_creates_fresh_session_without_nonce_reuse() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"antes"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let first_index = l.a.peers[0].current.as_ref().unwrap().remote_index;
        // A "reinicia": estado en memoria nuevo, mismas claves.
        let keys = StaticKeypair::from_private(*l.a.keys.private_bytes());
        l.a = mk(&l.cfg_a, keys, l.now);
        l.unix += 2; // timestamp creciente para B
        let out = l.a.on_tun(&v4(1, 2, b"despues"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
        let second_index = l.a.peers[0].current.as_ref().unwrap().remote_index;
        assert_ne!(first_index, second_index);
    }

    #[test]
    fn simultaneous_initiation_converges() {
        let mut l = lab();
        let oa = l.a.on_tun(&v4(1, 2, b"a"), l.now, l.unix);
        let ob = l.b.on_tun(&v4(2, 1, b"b"), l.now, l.unix);
        let (tun_a, tun_b) = pump(&mut l, oa, ob);
        assert_eq!((tun_a.len(), tun_b.len()), (1, 1));
        // a partir de aquí hay sesión usable en ambos sentidos
        let out = l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
        let out = l.b.on_tun(&v4(2, 1, b"y"), l.now, l.unix);
        let (tun_a, _) = pump(&mut l, vec![], out);
        assert_eq!(tun_a.len(), 1);
    }

    #[test]
    fn rekey_after_deadline_keeps_traffic_flowing() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        l.now += crypto::REKEY_AFTER_TIME + Duration::from_secs(1);
        l.unix += 121;
        let out = l.a.on_tun(&v4(1, 2, b"2"), l.now, l.unix); // sesión vieja aún válida
        let tick = l.a.on_tick(l.now, l.unix); // dispara rekey
        assert!(!tick.is_empty());
        let mut both = out;
        both.extend(tick);
        let (_, tun_b) = pump(&mut l, both, vec![]);
        assert_eq!(tun_b.len(), 1);
        assert_eq!(l.a.counters.0["handshakes_initiator"], 2);
        let out = l.a.on_tun(&v4(1, 2, b"3"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
    }

    #[test]
    fn wrong_network_is_rejected() {
        let mut l = lab();
        l.b.net = [1; 16];
        let out = l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert!(tun_b.is_empty());
        assert_eq!(l.b.counters.0["rx_init_invalid"], 1);
    }

    #[test]
    fn roaming_updates_endpoint_only_after_auth() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let out = l.a.on_tun(&v4(1, 2, b"2"), l.now, l.unix);
        let Out::Udp(_, dg) = &out[0] else { panic!() };
        let mut bad = dg.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        l.b.on_udp(ep("203.0.113.7:9"), &bad, l.now, l.unix);
        assert_eq!(l.b.peers[0].endpoint, Some(ep(A_EP)));
        l.b.on_udp(ep("203.0.113.7:9"), dg, l.now, l.unix);
        assert_eq!(l.b.peers[0].endpoint, Some(ep("203.0.113.7:9")));
    }

    fn udp_to(dport: u16) -> Vec<u8> {
        let mut d = vec![0x30, 0x39];
        d.extend_from_slice(&dport.to_be_bytes());
        d.extend_from_slice(&[0, 8, 0, 0]);
        v4(1, 2, &d)
    }

    fn allow_udp_a_to_b(lo: u16, hi: u16) -> Vec<Rule> {
        vec![Rule {
            src: Selector::Device([1; 16]),
            dst: Selector::Device([2; 16]),
            proto: 17,
            port_min: lo,
            port_max: hi,
        }]
    }

    #[test]
    fn acl_denies_by_default_and_allows_only_the_rule() {
        let mut l = lab_with(allow_udp_a_to_b(5000, 5010), allow_udp_a_to_b(5000, 5010));
        let ok = udp_to(5005);
        let out = l.a.on_tun(&ok, l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b, vec![ok]);
        // fuera del rango: se descarta en origen
        assert!(l.a.on_tun(&udp_to(80), l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["tx_acl_denied"], 1);
        // el sentido contrario no está permitido por ninguna regla
        let reply = v4(2, 1, &[0, 53, 0x30, 0x39, 0, 8, 0, 0]);
        assert!(l.b.on_tun(&reply, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["tx_acl_denied"], 1);
    }

    #[test]
    fn ingress_acl_protects_against_a_permissive_peer() {
        // A permite todo, pero B solo admite 5000-5010: B descarta lo demás aunque A lo envíe.
        let mut l = lab_with(allow_all(), allow_udp_a_to_b(5000, 5010));
        let out = l.a.on_tun(&udp_to(5005), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let out = l.a.on_tun(&udp_to(80), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert!(tun_b.is_empty());
        assert_eq!(l.b.counters.0["rx_acl_denied"], 1);
    }

    #[test]
    fn no_configuration_means_no_traffic() {
        let mut e = Engine::new(NET, StaticKeypair::generate(), Instant::now());
        assert!(
            e.on_tun(&v4(1, 2, b"x"), Instant::now(), 1_000_000)
                .is_empty()
        );
        assert!(
            e.on_udp(ep(A_EP), &[1, 1, 0], Instant::now(), 1_000_000)
                .is_empty()
        );
        assert_eq!(e.counters.0["denied_no_lease"], 2);
    }

    #[test]
    fn expired_lease_fails_closed_and_new_config_recovers() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        // bordes de la tolerancia de 30 s
        assert!(l.a.lease_active(NOT_AFTER + 29));
        assert!(!l.a.lease_active(NOT_AFTER + 30));
        assert!(l.a.lease_active(NOT_BEFORE - 30));
        assert!(!l.a.lease_active(NOT_BEFORE - 31));
        l.unix = NOT_AFTER + 30;
        assert!(l.a.on_tun(&v4(1, 2, b"2"), l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["denied_no_lease"], 1);
        assert!(l.a.on_tick(l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["lease_expired"], 1);
        assert!(l.a.peers[0].current.is_none()); // sesiones destruidas
        // nueva configuración con lease vigente: vuelve a funcionar con handshake nuevo
        let mut fresh_a = l.cfg_a.clone();
        fresh_a.not_after = NOT_AFTER + 10_000;
        fresh_a.config_version = 2;
        l.a.apply_config(&fresh_a);
        let mut fresh_b = l.cfg_b.clone();
        fresh_b.not_after = NOT_AFTER + 10_000;
        l.b.apply_config(&fresh_b);
        let out = l.a.on_tun(&v4(1, 2, b"3"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
    }

    #[test]
    fn config_without_a_peer_cuts_it_and_key_change_drops_the_session() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let old_index = l.b.peers[0].current.as_ref().unwrap().local_index;
        // B recibe una configuración sin A: el tráfico de A deja de aceptarse al instante
        let mut without = l.cfg_b.clone();
        without.peers.clear();
        without.config_version = 2;
        l.b.apply_config(&without);
        let out = l.a.on_tun(&v4(1, 2, b"2"), l.now, l.unix);
        let Out::Udp(_, dg) = &out[0] else { panic!() };
        assert!(l.b.on_udp(ep(A_EP), dg, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_unknown_index"], 1);
        // mismo peer pero con otra clave: la sesión vieja no sobrevive
        let mut rekeyed = l.cfg_b.clone();
        rekeyed.peers[0].noise_pubkey = StaticKeypair::generate().public;
        rekeyed.config_version = 3;
        l.b.apply_config(&rekeyed);
        assert!(l.b.peers[0].current.is_none());
        assert!(!l.b.by_index.contains_key(&old_index));
    }

    #[test]
    fn unchanged_peer_keeps_its_session_across_config_updates() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let idx = l.a.peers[0].current.as_ref().unwrap().local_index;
        let mut next = l.cfg_a.clone();
        next.config_version = 2;
        l.a.apply_config(&next);
        assert_eq!(l.a.peers[0].current.as_ref().unwrap().local_index, idx);
        let out = l.a.on_tun(&v4(1, 2, b"2"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
        assert_eq!(l.a.counters.0["handshakes_initiator"], 1);
    }
}
