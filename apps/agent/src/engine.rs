//! Máquina de estados del agente: sin E/S. Recibe eventos y devuelve acciones.

use crate::config::Config;
use crate::packet::parse_ip;
use atenea_crypto::{self as crypto, PendingInit, Session, StaticKeypair};
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
pub struct Status {
    pub public_key: String,
    pub counters: BTreeMap<&'static str, u64>,
    pub peers: Vec<PeerStatus>,
}

pub struct Engine {
    net: [u8; 16],
    keys: StaticKeypair,
    own: Vec<IpAddr>,
    peers: Vec<Peer>,
    by_addr: HashMap<IpAddr, usize>,
    by_key: HashMap<[u8; 32], usize>,
    by_index: HashMap<u32, usize>,
    limiter: Limiter,
    counters: Counters,
}

impl Engine {
    pub fn new(cfg: &Config, keys: StaticKeypair, now: Instant) -> Engine {
        let mut peers = Vec::new();
        let mut by_addr = HashMap::new();
        let mut by_key = HashMap::new();
        for (i, p) in cfg.peers.iter().enumerate() {
            for a in &p.addresses {
                by_addr.insert(*a, i);
            }
            by_key.insert(p.public, i);
            peers.push(Peer {
                public: p.public,
                addrs: p.addresses.clone(),
                endpoint: p.endpoint,
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
            });
        }
        Engine {
            net: cfg.network_id,
            keys,
            own: cfg.addresses.clone(),
            peers,
            by_addr,
            by_key,
            by_index: HashMap::new(),
            limiter: Limiter {
                tokens: INIT_RATE,
                last: now,
            },
            counters: Counters::default(),
        }
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
        peer.rx_bytes += total as u64;
        let mut pkt = plain;
        pkt.truncate(total);
        out.push(Out::Tun(pkt));
    }

    /// Temporizador (~1 s): caducidad, reintentos y rekey.
    pub fn on_tick(&mut self, now: Instant, unix: u64) -> Vec<Out> {
        let mut out = Vec::new();
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
        self.by_index.clear();
        for (pi, p) in self.peers.iter().enumerate() {
            for s in [&p.current, &p.previous, &p.next].into_iter().flatten() {
                self.by_index.insert(s.local_index, pi);
            }
            if let Some((pend, _)) = &p.pending {
                self.by_index.insert(pend.local_index(), pi);
            }
        }
        out
    }

    pub fn status(&self, now: Instant) -> Status {
        Status {
            public_key: crypto::hex::encode(&self.keys.public),
            counters: self.counters.0.clone(),
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

    fn cfg(
        own4: &str,
        own6: &str,
        peer_pub: &[u8; 32],
        peer4: &str,
        peer6: &str,
        peer_ep: &str,
    ) -> Config {
        Config::parse(&format!(
            r#"
network_id = "00112233445566778899aabbccddeeff"
private_key_file = "/k"
interface = "t"
listen = "0.0.0.0:51820"
ipc_socket = "/s"
addresses = ["{own4}", "{own6}"]
networks = ["10.77.0.0/24", "fd77::/64"]
[[peer]]
public_key = "{}"
addresses = ["{peer4}", "{peer6}"]
endpoint = "{peer_ep}"
"#,
            crypto::hex::encode(peer_pub)
        ))
        .unwrap()
    }

    struct Lab {
        a: Engine,
        b: Engine,
        now: Instant,
        unix: u64,
    }

    fn lab() -> Lab {
        let (ka, kb) = (StaticKeypair::generate(), StaticKeypair::generate());
        let now = Instant::now();
        let a = Engine::new(
            &cfg(
                "10.77.0.1",
                "fd77::1",
                &kb.public,
                "10.77.0.2",
                "fd77::2",
                B_EP,
            ),
            ka,
            now,
        );
        let b = Engine::new(
            &cfg(
                "10.77.0.2",
                "fd77::2",
                &a.keys.public,
                "10.77.0.1",
                "fd77::1",
                A_EP,
            ),
            kb,
            now,
        );
        Lab {
            a,
            b,
            now,
            unix: 1_000_000,
        }
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
        let cfg_a = cfg(
            "10.77.0.1",
            "fd77::1",
            &l.b.keys.public,
            "10.77.0.2",
            "fd77::2",
            B_EP,
        );
        l.a = Engine::new(&cfg_a, keys, l.now);
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
}
