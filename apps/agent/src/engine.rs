//! Máquina de estados del agente: sin E/S. Recibe eventos y devuelve acciones.
//!
//! Un par puede alcanzarse por tres caminos (preferencia `directo > relay UDP > relay TLS`) que
//! comparten UNA sesión Noise y un único espacio de contadores (ADR-003): cambiar de camino no
//! cambia claves; el antirreplay del receptor elimina las copias. Un camino solo cuenta como sano
//! con paquetes AUTENTICADOS recibidos por él (docs/protocol/relay-v1.md §5.2).
//! Las rutas firmadas solo amplían orígenes autorizados y destinos locales; no hay tránsito
//! entre pares. En salida prima la dirección exacta y después el prefijo más largo (§4 de v2).

use crate::packet::parse_ip;
use crate::relay::RelayKind;
use atenea_crypto::{self as crypto, PendingInit, Session, StaticKeypair};
use atenea_identity::{CLOCK_TOLERANCE_SECS, DeviceConfig, DeviceId, NetworkId, Prefix};
use atenea_nat_traversal::{MAX_CANDIDATES, sanitize_candidates};
use atenea_policies::{Policy, classify, prefix_contains};
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
/// Mensajes 1 por segundo (cada uno cuesta un DH) admitidos de orígenes sin par asociado.
const INIT_RATE: f64 = 50.0;
/// Mensajes 1 por segundo y ráfaga de un mismo origen desconocido (IP; IPv6 por /64).
const ORIGIN_INIT_RATE: f64 = 2.0;
const ORIGIN_INIT_BURST: f64 = 5.0;
/// Orígenes desconocidos recordados a la vez; con la tabla llena de orígenes activos se descarta.
const MAX_ORIGINS: usize = 1024;
const ORIGIN_IDLE: Duration = Duration::from_secs(10);
/// Mensajes 1 por segundo y ráfaga reservados a cada par configurado (origen conocido o relay).
const PEER_INIT_RATE: f64 = 10.0;
const PEER_INIT_BURST: f64 = 20.0;
/// Sondeo (hole punching): ventana y límite de envíos del mismo mensaje 1 (~1 s con jitter ±25 %).
const PROBE_WINDOW: Duration = Duration::from_secs(10);
const PROBE_MAX_ROUNDS: u32 = 12;
/// Intervalo mínimo entre rendezvous al coordinator para un mismo par.
const RENDEZVOUS_MIN_INTERVAL: Duration = Duration::from_secs(5);
/// Paquete vacío con un único camino establecido (los mapeos UDP de los NAT caducan a ~30 s).
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
/// Keepalive de cada camino cuando hay un camino alternativo establecido (detección rápida).
const FAST_KEEPALIVE: Duration = Duration::from_secs(4);
/// Sin nada autenticado del par por la ruta directa durante este tiempo, se da por perdida.
const PATH_DEAD_AFTER: Duration = Duration::from_secs(45);
/// Un camino está sano si recibió un paquete autenticado en esta ventana.
const HEALTH_WINDOW: Duration = Duration::from_secs(9);
/// Un camino preferido debe estar sano de forma continua este tiempo antes de volver a él.
const UPGRADE_HOLD: Duration = Duration::from_secs(5);
/// Se envió tráfico y no llegó nada autenticado en este tiempo: se sospecha del camino.
const SUSPECT_AFTER: Duration = Duration::from_secs(4);
/// Sondeo directo sin éxito durante este tiempo: se pide relay en paralelo.
const RELAY_AFTER_PROBE: Duration = Duration::from_secs(4);
const RELAY_REQUEST_INTERVAL: Duration = Duration::from_secs(5);
/// El directo lleva sano este tiempo: se libera la asignación del relay.
const RELEASE_AFTER_DIRECT: Duration = Duration::from_secs(30);

/// Retardo creciente tras un sondeo fallido: 30, 60, 120, 240 y 300 s como máximo.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs((30u64 << failures.saturating_sub(1).min(4)).min(300))
}

/// Próximo reenvío del sondeo: 1 s ±25 %.
fn next_probe(now: Instant) -> Instant {
    now + Duration::from_millis(750 + u64::from(crypto::random_u32() % 501))
}

/// Información de NAT y candidatos propios que muestra el estado.
#[derive(Serialize, Clone)]
pub struct NatStatus {
    pub mapping: &'static str,
    pub reflexive: Option<String>,
    pub candidates: Vec<String>,
}

/// Camino hacia un par, de mayor a menor preferencia.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    Direct,
    RelayUdp,
    RelayTls,
}

const PATHS: [PathKind; 3] = [PathKind::Direct, PathKind::RelayUdp, PathKind::RelayTls];

impl PathKind {
    fn idx(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            PathKind::Direct => "direct",
            PathKind::RelayUdp => "relay-udp",
            PathKind::RelayTls => "relay-tls",
        }
    }

    fn of(k: RelayKind) -> PathKind {
        match k {
            RelayKind::Udp => PathKind::RelayUdp,
            RelayKind::Tls => PathKind::RelayTls,
        }
    }

    fn relay(self) -> Option<RelayKind> {
        match self {
            PathKind::Direct => None,
            PathKind::RelayUdp => Some(RelayKind::Udp),
            PathKind::RelayTls => Some(RelayKind::Tls),
        }
    }
}

/// Por dónde llegó un datagrama.
#[derive(Clone, Copy)]
enum Via {
    Direct(SocketAddr),
    Relay(RelayKind),
}

impl Via {
    fn kind(self) -> PathKind {
        match self {
            Via::Direct(_) => PathKind::Direct,
            Via::Relay(k) => PathKind::of(k),
        }
    }
}

pub enum Out {
    Udp(SocketAddr, Vec<u8>),
    /// Datagrama de Atenea para un par por su asignación de relay.
    Relay {
        peer: DeviceId,
        kind: RelayKind,
        dg: Vec<u8>,
    },
    Tun(Vec<u8>),
}

#[derive(Default)]
struct Counters(BTreeMap<&'static str, u64>);

impl Counters {
    fn inc(&mut self, k: &'static str) {
        *self.0.entry(k).or_insert(0) += 1;
    }
}

/// Cubo de tokens: `rate` por segundo con ráfaga máxima `burst`.
struct Limiter {
    tokens: f64,
    last: Option<Instant>,
    rate: f64,
    burst: f64,
}

impl Limiter {
    fn new(rate: f64, burst: f64) -> Limiter {
        Limiter {
            tokens: burst,
            last: None,
            rate,
            burst,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last {
            let dt = now.saturating_duration_since(last).as_secs_f64();
            self.tokens = (self.tokens + dt * self.rate).min(self.burst);
        }
        self.last = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Agrupa IPv6 por /64: un atacante con un prefijo no multiplica sus cubos.
fn origin_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(a) => {
            let mut o = a.octets();
            o[8..].fill(0);
            IpAddr::V6(o.into())
        }
        v4 => v4,
    }
}

/// Sondeo en curso: reenvío periódico (mismo mensaje 1, o keepalives con sesión) a los candidatos.
struct Probe {
    started: Instant,
    next_at: Instant,
    rounds: u32,
}

#[derive(Default, Clone)]
struct PathStat {
    /// Solo relays: la asignación está lista para enviar.
    ready: bool,
    last_rx: Option<Instant>,
    healthy_since: Option<Instant>,
    last_tx: Option<Instant>,
    rtt_ms: Option<u32>,
}

struct Peer {
    device_id: DeviceId,
    public: [u8; 32],
    addrs: Vec<IpAddr>,
    routes: Vec<Prefix>,
    /// Ruta directa CONFIRMADA: solo cambia con un paquete Noise autenticado del par.
    endpoint: Option<SocketAddr>,
    /// Pistas estáticas de la configuración firmada (no autenticadas como ruta).
    cfg_endpoints: Vec<SocketAddr>,
    /// Pistas del coordinator (saneadas, acotadas, no autenticadas).
    candidates: Vec<SocketAddr>,
    current: Option<Session>,
    previous: Option<Session>,
    /// Sesión de respondedor aún sin confirmar por un paquete válido del iniciador.
    next: Option<Session>,
    pending: Option<(PendingInit, Instant)>,
    /// Mensaje 1 en vuelo, para reenviarlo idéntico durante el sondeo.
    pending_dg: Option<Vec<u8>>,
    probe: Option<Probe>,
    /// Sondeo del directo con la sesión ya establecida (keepalives cifrados a los candidatos).
    kprobe: Option<Probe>,
    probe_failures: u32,
    /// Sin sondear ni pedir rendezvous hasta este instante (retardo tras un sondeo fallido).
    hold_until: Option<Instant>,
    next_rendezvous: Option<Instant>,
    rendezvous_wanted: bool,
    relay_wanted: bool,
    next_relay_request: Option<Instant>,
    last_attempt: Option<Instant>,
    queued: Option<(Vec<u8>, Instant)>,
    last_init_ts: Option<u64>,
    /// Cupo propio de mensajes 1: un tercero no puede gastar el de un par configurado.
    init_limiter: Limiter,
    /// Último mensaje 1 aceptado y su respuesta, para repetirla ante retransmisiones idénticas.
    last_init_cache: Option<(Vec<u8>, Vec<u8>)>,
    last_handshake: Option<Instant>,
    /// Última actividad de datos (envío o recepción).
    last_data: Option<Instant>,
    /// Último envío de datos de usuario (no keepalives).
    last_data_tx: Option<Instant>,
    stat: [PathStat; 3],
    /// Camino por el que se envían los datos.
    active: PathKind,
    tx_bytes: u64,
    rx_bytes: u64,
}

impl Peer {
    fn new(device_id: DeviceId, public: [u8; 32]) -> Peer {
        Peer {
            device_id,
            public,
            addrs: Vec::new(),
            routes: Vec::new(),
            endpoint: None,
            cfg_endpoints: Vec::new(),
            candidates: Vec::new(),
            current: None,
            previous: None,
            next: None,
            pending: None,
            pending_dg: None,
            probe: None,
            kprobe: None,
            probe_failures: 0,
            hold_until: None,
            next_rendezvous: None,
            rendezvous_wanted: false,
            relay_wanted: false,
            next_relay_request: None,
            last_attempt: None,
            queued: None,
            last_init_ts: None,
            init_limiter: Limiter::new(PEER_INIT_RATE, PEER_INIT_BURST),
            last_init_cache: None,
            last_handshake: None,
            last_data: None,
            last_data_tx: None,
            stat: Default::default(),
            active: PathKind::Direct,
            tx_bytes: 0,
            rx_bytes: 0,
        }
    }

    /// La IP es un origen conocido de este par (ruta confirmada o pistas); no está autenticada.
    fn knows_ip(&self, ip: IpAddr) -> bool {
        self.endpoint
            .iter()
            .chain(&self.cfg_endpoints)
            .chain(&self.candidates)
            .any(|a| a.ip() == ip)
    }

    /// ¿Hay forma de enviar por ese camino?
    fn sendable(&self, k: PathKind) -> bool {
        match k {
            PathKind::Direct => self.endpoint.is_some(),
            _ => self.stat[k.idx()].ready,
        }
    }

    fn healthy(&self, k: PathKind, now: Instant) -> bool {
        self.sendable(k)
            && self.stat[k.idx()]
                .last_rx
                .is_some_and(|t| now.duration_since(t) < HEALTH_WINDOW)
    }

    /// Registra un paquete AUTENTICADO recibido por `k`.
    fn note_rx(&mut self, k: PathKind, now: Instant) {
        let s = &mut self.stat[k.idx()];
        if s.last_rx
            .is_none_or(|t| now.duration_since(t) >= HEALTH_WINDOW)
        {
            s.healthy_since = Some(now);
        }
        s.last_rx = Some(now);
    }

    /// Camino de envío: el activo, o el mejor disponible si el activo no se puede usar.
    fn send_path(&self) -> Option<PathKind> {
        if self.sendable(self.active) {
            Some(self.active)
        } else {
            PATHS.into_iter().find(|k| self.sendable(*k))
        }
    }

    fn out(&self, k: PathKind, dg: Vec<u8>) -> Option<Out> {
        match k {
            PathKind::Direct => self.endpoint.map(|e| Out::Udp(e, dg)),
            _ => Some(Out::Relay {
                peer: self.device_id,
                kind: k.relay()?,
                dg,
            }),
        }
    }

    fn reply(&self, via: Via, dg: Vec<u8>) -> Out {
        match via {
            Via::Direct(a) => Out::Udp(a, dg),
            Via::Relay(kind) => Out::Relay {
                peer: self.device_id,
                kind,
                dg,
            },
        }
    }

    fn established_paths(&self) -> usize {
        PATHS.into_iter().filter(|k| self.sendable(*k)).count()
    }

    fn last_rx_any(&self) -> Option<Instant> {
        self.stat.iter().filter_map(|s| s.last_rx).max()
    }

    fn path_name(&self, now: Instant) -> &'static str {
        // Con un único camino el keepalive es de 15 s: para informar basta con que esté vivo.
        let alive = self.sendable(self.active)
            && self.stat[self.active.idx()]
                .last_rx
                .is_some_and(|t| now.duration_since(t) < KEEPALIVE_INTERVAL + HEALTH_WINDOW);
        if alive {
            self.active.name()
        } else if let Some(k) = PATHS.into_iter().find(|k| self.healthy(*k, now)) {
            k.name()
        } else if self.pending.is_some() && self.probe.is_some() {
            "probing"
        } else {
            "none"
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
pub struct PathStatus {
    pub kind: &'static str,
    pub ready: bool,
    pub healthy: bool,
    pub last_rx_secs: Option<u64>,
    pub rtt_ms: Option<u32>,
}

#[derive(Serialize)]
pub struct PeerStatus {
    pub device_id: DeviceId,
    pub public_key: String,
    pub endpoint: Option<String>,
    pub session: Option<String>,
    pub last_handshake_secs_ago: Option<u64>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    /// `direct`, `relay-udp`, `relay-tls` (camino sano en uso), `probing` (hole punching en curso) o `none`.
    pub path: &'static str,
    pub candidates: usize,
    pub last_rx_secs: Option<u64>,
    pub paths: Vec<PathStatus>,
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
    pub nat: Option<NatStatus>,
    pub public_key: String,
    pub counters: BTreeMap<&'static str, u64>,
    pub config: ConfigStatus,
    pub peers: Vec<PeerStatus>,
}

pub struct Engine {
    net: NetworkId,
    keys: StaticKeypair,
    own: Vec<IpAddr>,
    advertised: Vec<Prefix>,
    /// Red del túnel y subredes (propias y de los pares): nada de ello es un destino de sondeo directo.
    tunnel_only: Vec<Prefix>,
    peers: Vec<Peer>,
    by_addr: HashMap<IpAddr, usize>,
    by_key: HashMap<[u8; 32], usize>,
    by_index: HashMap<u32, usize>,
    /// Cubo global de mensajes 1 de orígenes sin par asociado.
    limiter: Limiter,
    origins: HashMap<IpAddr, Limiter>,
    /// Un mensaje 1 con marca anterior a este instante (arranque del agente) no se acepta.
    init_floor: u64,
    counters: Counters,
    policy: Option<Policy>,
    /// Ventana (not_before, not_after) de la configuración vigente; sin ella todo se deniega.
    lease: Option<(u64, u64)>,
    config_version: u64,
    last_config_error: Option<&'static str>,
    nat: Option<NatStatus>,
}

impl Engine {
    /// Sin configuración: deniega todo hasta `apply_config` (fail-closed, ADR-002).
    /// `init_floor`: marca (unix) mínima de un mensaje 1; el arranque la fija para que un mensaje 1
    /// capturado antes de reiniciar no vuelva a aceptarse.
    pub fn new(net: NetworkId, keys: StaticKeypair, init_floor: u64) -> Engine {
        Engine {
            net,
            keys,
            own: Vec::new(),
            advertised: Vec::new(),
            tunnel_only: Vec::new(),
            peers: Vec::new(),
            by_addr: HashMap::new(),
            by_key: HashMap::new(),
            by_index: HashMap::new(),
            limiter: Limiter::new(INIT_RATE, INIT_RATE),
            origins: HashMap::new(),
            init_floor,
            counters: Counters::default(),
            policy: None,
            lease: None,
            config_version: 0,
            last_config_error: None,
            nat: None,
        }
    }

    /// Cupo de un origen desconocido; la tabla está acotada y se purga de orígenes inactivos.
    fn origin_allow(&mut self, key: IpAddr, now: Instant) -> bool {
        if !self.origins.contains_key(&key) && self.origins.len() >= MAX_ORIGINS {
            self.origins.retain(|_, l| {
                l.last
                    .is_some_and(|t| now.saturating_duration_since(t) < ORIGIN_IDLE)
            });
            if self.origins.len() >= MAX_ORIGINS {
                return false;
            }
        }
        self.origins
            .entry(key)
            .or_insert_with(|| Limiter::new(ORIGIN_INIT_RATE, ORIGIN_INIT_BURST))
            .allow(now)
    }

    /// Aplica una configuración ya verificada y persistida. Conserva las sesiones de los peers cuya
    /// clave no cambió; destruye las de peers eliminados o con clave nueva (ADR-002).
    pub fn apply_config(&mut self, cfg: &DeviceConfig) {
        let mut old = std::mem::take(&mut self.peers);
        // Prefijos alcanzables únicamente a través del túnel: red del túnel, rutas de los pares y las propias.
        let tunnel_only: Vec<Prefix> = cfg
            .network_prefixes
            .iter()
            .cloned()
            .chain(cfg.peers.iter().flat_map(|p| p.routes.iter().cloned()))
            .chain(cfg.advertised.iter().map(|r| r.prefix.clone()))
            .collect();
        for p in &cfg.peers {
            let mut peer = match old
                .iter()
                .position(|o| o.device_id == p.device_id && o.public == p.noise_pubkey)
            {
                Some(i) => old.swap_remove(i),
                None => Peer::new(p.device_id, p.noise_pubkey),
            };
            peer.addrs = p.addrs.clone();
            peer.routes = p.routes.clone();
            peer.cfg_endpoints = p
                .endpoints
                .iter()
                .copied()
                .filter(|e| !tunnel_only.iter().any(|t| prefix_contains(t, &e.ip())))
                .collect();
            peer.candidates
                .retain(|c| !tunnel_only.iter().any(|t| prefix_contains(t, &c.ip())));
            self.peers.push(peer);
        }
        self.tunnel_only = tunnel_only;
        self.own = cfg.tunnel_addrs.clone();
        self.advertised = cfg.advertised.iter().map(|r| r.prefix.clone()).collect();
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

    pub fn knows_peer(&self, id: &DeviceId) -> bool {
        self.peers.iter().any(|p| &p.device_id == id)
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
            p.kprobe = None;
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

    /// Un paquete rechazado por el backend se descarta; los fallos del dispositivo se propagan.
    pub fn on_tun_error(&mut self, error: std::io::Error) -> std::io::Result<Vec<Out>> {
        match error.kind() {
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => {
                self.counters.inc("tx_malformed");
                Ok(Vec::new())
            }
            _ => Err(error),
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
        if !self.own.contains(&src) && !self.advertised.iter().any(|p| prefix_contains(p, &src)) {
            self.counters.inc("tx_bad_source");
            return out;
        }
        let destination = self.by_addr.get(&dst).copied().or_else(|| {
            self.peers
                .iter()
                .enumerate()
                .flat_map(|(pi, peer)| peer.routes.iter().map(move |p| (pi, p)))
                .filter(|(_, p)| prefix_contains(p, &dst))
                .max_by_key(|(_, p)| p.len)
                .map(|(pi, _)| pi)
        });
        let Some(pi) = destination else {
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
            && let Some(k) = peer.send_path()
            && let Some(s) = peer.current.as_mut()
        {
            match s.encrypt(pkt, now) {
                Ok(dg) => {
                    peer.tx_bytes += pkt.len() as u64;
                    peer.last_data_tx = Some(now);
                    peer.stat[k.idx()].last_tx = Some(now);
                    if let Some(o) = peer.out(k, dg) {
                        out.push(o);
                    }
                    return out;
                }
                Err(crypto::Error::NotConfirmed) => {}
                Err(_) => self.counters.inc("tx_encrypt_failed"),
            }
        }
        // Sin sesión utilizable: se guarda el último paquete y se inicia handshake.
        let peer = &mut self.peers[pi];
        peer.queued = Some((pkt.to_vec(), now));
        peer.rendezvous_wanted = peer.endpoint.is_none();
        self.maybe_initiate(pi, now, unix, &mut out);
        out
    }

    /// Destinos directos de un handshake o sondeo: la ruta confirmada o, sin ella, las pistas.
    fn targets(peer: &Peer) -> Vec<SocketAddr> {
        if let Some(ep) = peer.endpoint {
            return vec![ep];
        }
        let mut v = peer.cfg_endpoints.clone();
        for c in &peer.candidates {
            if !v.contains(c) {
                v.push(*c);
            }
        }
        v
    }

    /// Primer camino de relay listo (UDP antes que TLS).
    fn ready_relay(peer: &Peer) -> Option<PathKind> {
        [PathKind::RelayUdp, PathKind::RelayTls]
            .into_iter()
            .find(|k| peer.sendable(*k))
    }

    fn maybe_initiate(&mut self, pi: usize, now: Instant, unix: u64, out: &mut Vec<Out>) {
        let index = self.fresh_index();
        let peer = &mut self.peers[pi];
        if peer.pending.is_some()
            || peer
                .last_attempt
                .is_some_and(|t| now.duration_since(t) < HANDSHAKE_RETRY)
            || (peer.endpoint.is_none()
                && peer.hold_until.is_some_and(|h| now < h)
                && Self::ready_relay(peer).is_none())
        {
            return;
        }
        // Con ruta directa activa y confirmada solo se usa esa; si no, también el mejor relay listo.
        let direct_only = peer.endpoint.is_some() && peer.active == PathKind::Direct;
        let targets = Self::targets(peer);
        let relay = if direct_only {
            None
        } else {
            Self::ready_relay(peer)
        };
        if targets.is_empty() && relay.is_none() {
            // Sin pistas: solo el coordinator puede aportar candidatos.
            peer.rendezvous_wanted = true;
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
        let probing = peer.endpoint.is_none();
        peer.pending = Some((pending, now));
        peer.pending_dg = Some(dg.clone());
        peer.probe = probing.then(|| Probe {
            started: now,
            next_at: next_probe(now),
            rounds: 1,
        });
        peer.last_attempt = Some(now);
        if probing {
            peer.rendezvous_wanted = true;
        }
        self.by_index.insert(index, pi);
        self.counters.inc("tx_init");
        for t in targets {
            out.push(Out::Udp(t, dg.clone()));
        }
        if let Some(k) = relay
            && let Some(o) = peer.out(k, dg)
        {
            out.push(o);
        }
    }

    /// Datagrama UDP recibido por el socket de datos.
    pub fn on_udp(&mut self, from: SocketAddr, dg: &[u8], now: Instant, unix: u64) -> Vec<Out> {
        self.on_datagram(Via::Direct(from), None, dg, now, unix)
    }

    /// Datagrama de Atenea recibido de `peer` a través de su asignación de relay.
    pub fn on_relay_rx(
        &mut self,
        peer: &DeviceId,
        kind: RelayKind,
        dg: &[u8],
        now: Instant,
        unix: u64,
    ) -> Vec<Out> {
        self.on_datagram(Via::Relay(kind), Some(peer), dg, now, unix)
    }

    fn on_datagram(
        &mut self,
        via: Via,
        hint: Option<&DeviceId>,
        dg: &[u8],
        now: Instant,
        unix: u64,
    ) -> Vec<Out> {
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
            }) => self.on_init(via, hint, sender_index, noise, now, unix, &mut out),
            Ok(Packet::Resp {
                sender_index,
                receiver_index,
                noise,
            }) => self.on_resp(
                via,
                hint,
                sender_index,
                receiver_index,
                noise,
                now,
                &mut out,
            ),
            Ok(Packet::Data {
                receiver_index,
                counter,
                ciphertext,
            }) => self.on_data(
                via,
                hint,
                receiver_index,
                counter,
                ciphertext,
                now,
                &mut out,
            ),
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn on_init(
        &mut self,
        via: Via,
        hint: Option<&DeviceId>,
        sender_index: u32,
        noise: &[u8],
        now: Instant,
        unix: u64,
        out: &mut Vec<Out>,
    ) {
        let known = match (via, hint) {
            (Via::Relay(_), Some(h)) => self.peers.iter().position(|p| p.device_id == *h),
            (Via::Direct(from), _) => self.peers.iter().position(|p| p.knows_ip(from.ip())),
            _ => None,
        };
        let allowed = match (known, via) {
            (Some(pi), _) => self.peers[pi].init_limiter.allow(now),
            (None, Via::Direct(from)) => {
                self.origin_allow(origin_key(from.ip()), now) && self.limiter.allow(now)
            }
            (None, _) => self.limiter.allow(now),
        };
        if !allowed {
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
        if hint.is_some_and(|h| *h != self.peers[pi].device_id) {
            self.counters.inc("rx_relay_peer_mismatch");
            return;
        }
        if inc.timestamp < self.init_floor {
            self.counters.inc("rx_init_stale");
            return;
        }
        let index = self.fresh_index();
        let peer = &mut self.peers[pi];
        if crypto::check_timestamp(inc.timestamp, unix, peer.last_init_ts).is_err() {
            // Retransmisión idéntica del último mensaje 1 aceptado (sondeo por varios candidatos o
            // respuesta perdida por un NAT): se repite la respuesta guardada sin crear sesión ni
            // tocar el endpoint. Una repetición ajena solo recibe una respuesta más corta que la petición.
            if let Some((n, r)) = &peer.last_init_cache
                && n.as_slice() == noise
            {
                out.push(peer.reply(via, r.clone()));
                self.counters.inc("rx_init_retransmit");
            } else {
                self.counters.inc("rx_init_stale");
            }
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
        peer.pending_dg = None;
        peer.probe = None;
        peer.next = Some(session);
        peer.last_init_ts = Some(ts);
        peer.last_init_cache = Some((noise.to_vec(), resp.clone()));
        // El endpoint NO cambia aquí: solo un paquete autenticado de la sesión lo confirma.
        self.by_index.insert(index, pi);
        self.counters.inc("rx_init");
        out.push(peer.reply(via, resp));
    }

    #[allow(clippy::too_many_arguments)]
    fn on_resp(
        &mut self,
        via: Via,
        hint: Option<&DeviceId>,
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
        if hint.is_some_and(|h| *h != peer.device_id) {
            self.counters.inc("rx_relay_peer_mismatch");
            return;
        }
        let Some((pending, _)) = peer.pending.as_mut() else {
            self.counters.inc("rx_resp_unknown");
            return;
        };
        if pending.local_index() != receiver_index {
            self.counters.inc("rx_resp_unknown");
            return;
        }
        // Solo un mensaje 2 autenticado consume el handshake pendiente.
        let Ok(session) = pending.finish(noise, sender_index, now) else {
            self.counters.inc("rx_resp_invalid");
            return;
        };
        peer.pending = None;
        peer.previous = peer.current.take();
        peer.current = Some(session);
        // Camino confirmado: la respuesta solo puede producirla el par (Noise IK autenticado).
        let kind = via.kind();
        if let Via::Direct(from) = via
            && !self
                .tunnel_only
                .iter()
                .any(|t| prefix_contains(t, &from.ip()))
        {
            peer.endpoint = Some(from);
        }
        peer.note_rx(kind, now);
        if !peer.healthy(peer.active, now) {
            peer.active = kind;
        }
        peer.last_handshake = Some(now);
        peer.pending_dg = None;
        peer.probe = None;
        if kind == PathKind::Direct {
            peer.probe_failures = 0;
            peer.hold_until = None;
            peer.rendezvous_wanted = false;
        }
        self.counters.inc("handshakes_initiator");
        // Keepalive inmediato: confirma la sesión en el respondedor.
        let s = peer.current.as_mut().expect("recién creada");
        if let Ok(dg) = s.encrypt(&[], now) {
            peer.stat[kind.idx()].last_tx = Some(now);
            out.push(peer.reply(via, dg));
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
            peer.last_data_tx = Some(now);
            out.push(peer.reply(via, dg));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_data(
        &mut self,
        via: Via,
        hint: Option<&DeviceId>,
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
        if hint.is_some_and(|h| *h != peer.device_id) {
            self.counters.inc("rx_relay_peer_mismatch");
            return;
        }
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
        // Paquete AUTENTICADO por este camino: lo marca sano. El endpoint directo solo cambia aquí (ADR-003).
        let kind = via.kind();
        if let Via::Direct(from) = via
            && peer.endpoint != Some(from)
            && !self
                .tunnel_only
                .iter()
                .any(|t| prefix_contains(t, &from.ip()))
        {
            peer.endpoint = Some(from);
            self.counters.inc("endpoint_updates");
        }
        peer.note_rx(kind, now);
        if !peer.healthy(peer.active, now) {
            peer.active = kind;
        }
        if kind == PathKind::Direct {
            peer.kprobe = None;
            peer.probe_failures = 0;
            peer.hold_until = None;
            peer.rendezvous_wanted = false;
        }
        peer.last_data = Some(now);
        // Con sesión confirmada, un paquete en cola esperando ya puede salir.
        if peer.queued.is_some()
            && let Some(k) = peer.send_path()
            && let Some(s) = peer.current.as_mut()
            && s.is_confirmed()
            && let Some((q, at)) = peer.queued.take()
            && now.duration_since(at) < QUEUE_TTL
            && let Ok(dg) = s.encrypt(&q, now)
        {
            peer.tx_bytes += q.len() as u64;
            peer.last_data_tx = Some(now);
            peer.stat[k.idx()].last_tx = Some(now);
            if let Some(o) = peer.out(k, dg) {
                out.push(o);
            }
        }
        if plain.is_empty() {
            self.counters.inc("rx_keepalive");
            return;
        }
        let Some((src, dst, total)) = parse_ip(&plain) else {
            self.counters.inc("rx_bad_packet");
            return;
        };
        if !peer.addrs.contains(&src) && !peer.routes.iter().any(|p| prefix_contains(p, &src)) {
            self.counters.inc("rx_bad_source");
            return;
        }
        if !self.own.contains(&dst) && !self.advertised.iter().any(|p| prefix_contains(p, &dst)) {
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

    /// Temporizador (~250 ms): caducidad, sondeo, keepalive, conmutación de caminos, reintentos y rekey.
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
            let targets = Self::targets(&self.peers[pi]);
            let peer = &mut self.peers[pi];
            for slot in [&mut peer.current, &mut peer.previous, &mut peer.next] {
                if slot.as_ref().is_some_and(|s| s.is_expired(now)) {
                    *slot = None;
                }
            }
            let active = peer
                .last_data
                .is_some_and(|t| now.duration_since(t) < ACTIVE_WINDOW);
            // Sondeo: el MISMO mensaje 1 por directo y relay; el primer envío UDP puede perderse.
            if let Some((_, at)) = &peer.pending {
                let relay = Self::ready_relay(peer).and_then(PathKind::relay);
                let probe_over = peer.probe.as_ref().is_some_and(|p| {
                    now.duration_since(p.started) >= PROBE_WINDOW || p.rounds >= PROBE_MAX_ROUNDS
                });
                if probe_over {
                    peer.pending = None;
                    peer.pending_dg = None;
                    peer.probe = None;
                    peer.probe_failures += 1;
                    peer.hold_until = Some(now + backoff(peer.probe_failures));
                    self.counters.inc("probe_failed");
                } else if let (Some(p), Some(dg)) = (peer.probe.as_mut(), peer.pending_dg.as_ref())
                {
                    if now >= p.next_at {
                        for t in &targets {
                            out.push(Out::Udp(*t, dg.clone()));
                        }
                        if let Some(kind) = relay {
                            out.push(Out::Relay {
                                peer: peer.device_id,
                                kind,
                                dg: dg.clone(),
                            });
                        }
                        p.rounds += 1;
                        p.next_at = next_probe(now);
                        self.counters.inc("tx_probe");
                    }
                    // El directo no llega: se pide relay en paralelo sin abandonar el sondeo.
                    if now.duration_since(p.started) >= RELAY_AFTER_PROBE
                        && Self::ready_relay(peer).is_none()
                    {
                        peer.relay_wanted = true;
                    }
                } else if now.duration_since(*at) >= HANDSHAKE_RETRY {
                    peer.pending = None;
                    peer.pending_dg = None;
                }
            }
            // Con la sesión establecida y sin directo confirmado: keepalives cifrados a los candidatos.
            let session_ok = peer
                .current
                .as_ref()
                .is_some_and(|s| s.is_confirmed() && !s.is_expired(now));
            if session_ok && peer.endpoint.is_none() && peer.pending.is_none() {
                if peer.kprobe.is_none()
                    && !targets.is_empty()
                    && peer.hold_until.is_none_or(|h| now >= h)
                    && peer.established_paths() > 0
                {
                    peer.kprobe = Some(Probe {
                        started: now,
                        next_at: now,
                        rounds: 0,
                    });
                }
                let mut finished = false;
                if let Some(kp) = peer.kprobe.as_mut() {
                    if now.duration_since(kp.started) >= PROBE_WINDOW
                        || kp.rounds >= PROBE_MAX_ROUNDS
                    {
                        finished = true;
                    } else if now >= kp.next_at
                        && let Some(s) = peer.current.as_mut()
                        && let Ok(dg) = s.encrypt(&[], now)
                    {
                        for t in &targets {
                            out.push(Out::Udp(*t, dg.clone()));
                        }
                        kp.rounds += 1;
                        kp.next_at = next_probe(now);
                        self.counters.inc("tx_probe_ka");
                    }
                }
                if finished {
                    peer.kprobe = None;
                    peer.probe_failures += 1;
                    peer.hold_until = Some(now + backoff(peer.probe_failures));
                    self.counters.inc("probe_failed");
                }
            } else {
                peer.kprobe = None;
            }
            if peer
                .queued
                .as_ref()
                .is_some_and(|(_, at)| now.duration_since(*at) >= QUEUE_TTL)
            {
                peer.queued = None;
            }
            // Keepalive por cada camino establecido: rápido si hay alternativa, lento si es el único.
            if session_ok {
                let interval = if peer.established_paths() >= 2 {
                    FAST_KEEPALIVE
                } else {
                    KEEPALIVE_INTERVAL
                };
                for k in PATHS {
                    if peer.sendable(k)
                        && peer.stat[k.idx()]
                            .last_tx
                            .is_none_or(|t| now.duration_since(t) >= interval)
                        && let Some(s) = peer.current.as_mut()
                        && let Ok(dg) = s.encrypt(&[], now)
                        && let Some(o) = peer.out(k, dg)
                    {
                        peer.stat[k.idx()].last_tx = Some(now);
                        out.push(o);
                    }
                }
            }
            // Ruta directa perdida: nada autenticado del par por ella durante demasiado tiempo.
            if peer.endpoint.is_some()
                && peer.stat[0]
                    .last_rx
                    .is_some_and(|t| now.duration_since(t) >= PATH_DEAD_AFTER)
            {
                peer.endpoint = None;
                peer.stat[0].last_rx = None;
                peer.stat[0].healthy_since = None;
                self.counters.inc("path_lost");
                if active {
                    peer.rendezvous_wanted = true;
                }
            }
            // Conmutación de caminos (docs/protocol/relay-v1.md §5.2).
            let healthy = PATHS.map(|k| peer.healthy(k, now));
            for k in PATHS {
                if !healthy[k.idx()] {
                    peer.stat[k.idx()].healthy_since = None;
                }
            }
            if !healthy[peer.active.idx()] {
                // Conmutación a peor: inmediata si otro camino está sano.
                if let Some(best) = PATHS.into_iter().find(|k| healthy[k.idx()]) {
                    peer.active = best;
                    self.counters.inc("path_switches");
                }
            } else if let Some(better) = PATHS.into_iter().find(|k| {
                k.idx() < peer.active.idx()
                    && healthy[k.idx()]
                    && peer.stat[k.idx()]
                        .healthy_since
                        .is_some_and(|t| now.duration_since(t) >= UPGRADE_HOLD)
            }) {
                // Conmutación a mejor: solo tras estar sano de forma continua (histéresis).
                peer.active = better;
                self.counters.inc("path_switches");
            }
            // Sospecha: se envió tráfico por el camino activo y no llegó nada autenticado.
            if session_ok
                && Self::ready_relay(peer).is_none()
                && let Some(tx) = peer.last_data_tx
                && now.duration_since(tx) < SUSPECT_AFTER * 2
                && peer
                    .last_rx_any()
                    .is_none_or(|rx| tx > rx && now.duration_since(rx) >= SUSPECT_AFTER)
            {
                peer.relay_wanted = true;
            }
            let stale = peer.current.as_ref().is_none_or(|s| s.should_rekey(now));
            let need = peer.queued.is_some() || (active && stale);
            if need {
                self.maybe_initiate(pi, now, unix, &mut out);
            }
        }
        self.reindex();
        out
    }

    /// Pares que necesitan un rendezvous con el coordinator ahora (sin ruta confirmada, con tráfico
    /// pendiente o ruta perdida), respetando el intervalo mínimo y el retardo tras un sondeo fallido.
    pub fn take_rendezvous(&mut self, now: Instant) -> Vec<DeviceId> {
        let mut v = Vec::new();
        for p in &mut self.peers {
            if p.rendezvous_wanted
                && p.endpoint.is_none()
                && p.next_rendezvous.is_none_or(|t| now >= t)
                && p.hold_until.is_none_or(|t| now >= t)
            {
                v.push(p.device_id);
                p.next_rendezvous = Some(now + RENDEZVOUS_MIN_INTERVAL);
                p.rendezvous_wanted = false;
            }
        }
        v
    }

    /// Pares que necesitan un relay ahora (el directo no llega y no hay relay listo).
    pub fn take_relay_requests(&mut self, now: Instant) -> Vec<DeviceId> {
        let mut v = Vec::new();
        for p in &mut self.peers {
            if p.relay_wanted
                && Self::ready_relay(p).is_none()
                && p.next_relay_request.is_none_or(|t| now >= t)
            {
                v.push(p.device_id);
                p.next_relay_request = Some(now + RELAY_REQUEST_INTERVAL);
                self.counters.inc("relay_requests");
            }
            p.relay_wanted = false;
        }
        v
    }

    /// Pares cuyo relay ya no hace falta (directo sano y estable): el llamador cierra la asignación.
    pub fn take_relay_releases(&mut self, now: Instant) -> Vec<DeviceId> {
        let mut v = Vec::new();
        for p in &mut self.peers {
            if p.active == PathKind::Direct
                && p.healthy(PathKind::Direct, now)
                && p.stat[0]
                    .healthy_since
                    .is_some_and(|t| now.duration_since(t) >= RELEASE_AFTER_DIRECT)
                && (p.stat[1].ready || p.stat[2].ready)
            {
                for k in [PathKind::RelayUdp, PathKind::RelayTls] {
                    p.stat[k.idx()] = PathStat::default();
                }
                v.push(p.device_id);
                self.counters.inc("relay_released");
            }
        }
        v
    }

    /// El cliente de relay informa de que la asignación hacia `peer` está lista o caída.
    /// Al estar lista se reenvía el mensaje 1 pendiente y un keepalive por ese camino, para que el
    /// par aprenda el camino.
    pub fn set_relay_ready(
        &mut self,
        peer: &DeviceId,
        kind: RelayKind,
        ready: bool,
        now: Instant,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(p) = self.peers.iter_mut().find(|p| &p.device_id == peer) else {
            return out;
        };
        let k = PathKind::of(kind);
        let s = &mut p.stat[k.idx()];
        s.ready = ready;
        s.last_rx = None;
        s.healthy_since = None;
        if ready {
            self.counters.inc("relay_ready");
            if let Some(dg) = p.pending_dg.clone()
                && let Some(o) = p.out(k, dg)
            {
                out.push(o);
            }
            if let Some(s) = p.current.as_mut()
                && s.is_confirmed()
                && let Ok(dg) = s.encrypt(&[], now)
                && let Some(o) = p.out(k, dg)
            {
                p.stat[k.idx()].last_tx = Some(now);
                out.push(o);
            }
        } else {
            self.counters.inc("relay_down");
            if p.active == k {
                p.active = PATHS
                    .into_iter()
                    .find(|x| p.sendable(*x))
                    .unwrap_or(PathKind::Direct);
            }
            // Si aún hace falta, se vuelve a pedir tras el intervalo mínimo.
            if p.last_data
                .is_some_and(|t| now.duration_since(t) < ACTIVE_WINDOW)
            {
                p.relay_wanted = true;
            }
        }
        out
    }

    pub fn set_relay_rtt(&mut self, peer: &DeviceId, kind: RelayKind, ms: u32) {
        if let Some(p) = self.peers.iter_mut().find(|p| &p.device_id == peer) {
            p.stat[PathKind::of(kind).idx()].rtt_ms = Some(ms);
        }
    }

    /// Candidatos de un par entregados por el coordinator: son PISTAS no autenticadas. Se sanean,
    /// se acotan y solo sirven como destino de sondeos; la ruta se confirma con un paquete
    /// autenticado (nunca por el aviso del coordinator).
    pub fn on_peer_candidates(
        &mut self,
        id: &DeviceId,
        cands: Vec<SocketAddr>,
        now: Instant,
        unix: u64,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        if !self.lease_active(unix) {
            return out;
        }
        let Some(pi) = self.peers.iter().position(|p| &p.device_id == id) else {
            self.counters.inc("coord_unknown_peer");
            return out;
        };
        let tunnel_only = &self.tunnel_only;
        let peer = &mut self.peers[pi];
        // Un candidato dentro de la red del túnel o de una subred servida solo es alcanzable POR el túnel:
        // sondearlo sería un bucle (y el coordinator no es de confianza para decidirlo).
        peer.candidates = sanitize_candidates(cands, MAX_CANDIDATES)
            .into_iter()
            .filter(|c| !tunnel_only.iter().any(|p| prefix_contains(p, &c.ip())))
            .collect();
        peer.hold_until = None;
        if peer.healthy(PathKind::Direct, now) {
            return out;
        }
        self.counters.inc("rendezvous_candidates");
        if peer.pending.is_some() {
            if let Some(p) = peer.probe.as_mut() {
                // Nuevas pistas no renuevan la ventana ni el presupuesto del mismo mensaje 1.
                p.next_at = now;
            }
        } else if peer.current.as_ref().is_some_and(|s| s.is_confirmed()) {
            // Sesión ya establecida (p. ej. por relay): se sondea el directo con keepalives cifrados.
            peer.endpoint = None;
            peer.kprobe = Some(Probe {
                started: now,
                next_at: now,
                rounds: 0,
            });
        } else {
            peer.last_attempt = None;
            self.maybe_initiate(pi, now, unix, &mut out);
        }
        out
    }

    /// Información de NAT/candidatos propios para el estado (la calcula el bucle de E/S).
    pub fn set_nat(&mut self, nat: NatStatus) {
        self.nat = Some(nat);
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
            nat: self.nat.clone(),
            peers: self
                .peers
                .iter()
                .map(|p| PeerStatus {
                    device_id: p.device_id,
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
                    path: p.path_name(now),
                    candidates: p.candidates.len() + p.cfg_endpoints.len(),
                    last_rx_secs: p.last_rx_any().map(|t| now.duration_since(t).as_secs()),
                    paths: PATHS
                        .into_iter()
                        .map(|k| PathStatus {
                            kind: k.name(),
                            ready: p.sendable(k),
                            healthy: p.healthy(k, now),
                            last_rx_secs: p.stat[k.idx()]
                                .last_rx
                                .map(|t| now.duration_since(t).as_secs()),
                            rtt_ms: p.stat[k.idx()].rtt_ms,
                        })
                        .collect(),
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

    #[test]
    fn malformed_tun_read_is_counted_without_stopping_following_traffic() {
        let mut l = lab();
        for kind in [
            std::io::ErrorKind::InvalidData,
            std::io::ErrorKind::InvalidInput,
        ] {
            assert!(
                l.a.on_tun_error(std::io::Error::from(kind))
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(l.a.counters.0["tx_malformed"], 2);
        let out = l.a.on_tun(&v4(1, 2, b"despues"), l.now, l.unix);
        let (_, received) = pump(&mut l, out, vec![]);
        assert_eq!(received.len(), 1);
        assert_eq!(
            l.a.on_tun_error(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
    }

    use atenea_identity::{AdvertisedRoute, DnsConfig, Peer as CfgPeer, Rule, Selector};

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
                routes: vec![],
            }],
            rules,
            advertised: vec![],
            dns: DnsConfig {
                servers: vec![],
                domains: vec![],
            },
        }
    }

    fn mk(cfg: &DeviceConfig, keys: StaticKeypair, _now: Instant) -> Engine {
        let mut e = Engine::new(NET, keys, 0);
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
        /// Simulación de red: si es falso, los datagramas directos / por relay se pierden.
        direct_ok: bool,
        relay_ok: bool,
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
            direct_ok: true,
            relay_ok: true,
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
                    Out::Udp(_, dg) if l.direct_ok => {
                        nb.extend(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix))
                    }
                    Out::Relay { kind, dg, .. } if l.relay_ok => {
                        nb.extend(l.b.on_relay_rx(&[1; 16], kind, &dg, l.now, l.unix))
                    }
                    Out::Tun(p) => tun_a.push(p),
                    _ => {}
                }
            }
            for o in from_b.drain(..) {
                match o {
                    Out::Udp(_, dg) if l.direct_ok => {
                        na.extend(l.a.on_udp(ep(B_EP), &dg, l.now, l.unix))
                    }
                    Out::Relay { kind, dg, .. } if l.relay_ok => {
                        na.extend(l.a.on_relay_rx(&[2; 16], kind, &dg, l.now, l.unix))
                    }
                    Out::Tun(p) => tun_b.push(p),
                    _ => {}
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

    fn subnet_lab() -> Lab {
        let mut l = lab();
        let routes = vec![
            Prefix {
                addr: ip("192.168.40.0"),
                len: 24,
            },
            Prefix {
                addr: ip("fd40::"),
                len: 64,
            },
        ];
        l.cfg_a.peers[0].routes = routes.clone();
        l.cfg_b.advertised = routes
            .into_iter()
            .map(|prefix| AdvertisedRoute {
                prefix,
                snat: false,
            })
            .collect();
        l.a.apply_config(&l.cfg_a);
        l.b.apply_config(&l.cfg_b);
        l
    }

    #[test]
    fn subnet_routes_deliver_ipv4_and_ipv6_and_accept_advertised_replies() {
        let mut l = subnet_lab();
        let packets = [
            (
                build_ipv4(
                    "10.77.0.1".parse().unwrap(),
                    "192.168.40.9".parse().unwrap(),
                    b"ida",
                ),
                build_ipv4(
                    "192.168.40.9".parse().unwrap(),
                    "10.77.0.1".parse().unwrap(),
                    b"vuelta",
                ),
            ),
            (
                build_ipv6(
                    "fd77::1".parse().unwrap(),
                    "fd40::9".parse().unwrap(),
                    b"ida",
                ),
                build_ipv6(
                    "fd40::9".parse().unwrap(),
                    "fd77::1".parse().unwrap(),
                    b"vuelta",
                ),
            ),
        ];
        for (request, reply) in packets {
            let out = l.a.on_tun(&request, l.now, l.unix);
            let (_, tun_b) = pump(&mut l, out, vec![]);
            assert_eq!(tun_b, vec![request]);
            let out = l.b.on_tun(&reply, l.now, l.unix);
            let (tun_a, _) = pump(&mut l, vec![], out);
            assert_eq!(tun_a, vec![reply]);
        }
    }

    #[test]
    fn peer_endpoints_inside_tunnel_only_prefixes_are_discarded() {
        let mut l = lab();
        l.cfg_a.peers[0].routes = vec![Prefix {
            addr: ip("192.168.50.0"),
            len: 24,
        }];
        l.cfg_a.peers[0].endpoints = vec![ep("192.168.50.1:51820"), ep("192.0.2.9:51820")];
        l.a.apply_config(&l.cfg_a);
        assert_eq!(l.a.peers[0].cfg_endpoints, vec![ep("192.0.2.9:51820")]);
        l.a.on_peer_candidates(
            &[2; 16],
            vec![
                ep("192.168.50.1:51820"),
                ep("192.0.2.7:51820"),
                ep("10.77.0.2:51820"),
            ],
            l.now,
            l.unix,
        );
        assert_eq!(l.a.peers[0].candidates, vec![ep("192.0.2.7:51820")]);
    }

    #[test]
    fn exact_destination_precedes_longest_route_and_missing_routes_are_dropped() {
        let mut l = lab();
        // Se omite el verificador para probar precedencia incluso con solapes prohibidos.
        l.cfg_a.peers[0].routes = vec![Prefix {
            addr: ip("192.168.0.0"),
            len: 16,
        }];
        let mut narrow = l.cfg_a.peers[0].clone();
        narrow.device_id = [3; 16];
        narrow.noise_pubkey = StaticKeypair::generate().public;
        narrow.addrs = vec![ip("10.77.0.3")];
        narrow.routes = vec![
            Prefix {
                addr: ip("192.168.40.0"),
                len: 24,
            },
            Prefix {
                addr: ip("10.77.0.0"),
                len: 24,
            },
        ];
        l.cfg_a.peers.push(narrow);
        l.a.apply_config(&l.cfg_a);
        for (dst, expected) in [("192.168.40.9", 1), ("192.168.41.9", 0), ("10.77.0.2", 0)] {
            for peer in &mut l.a.peers {
                peer.queued = None;
            }
            let pkt = build_ipv4("10.77.0.1".parse().unwrap(), dst.parse().unwrap(), b"ruta");
            l.a.on_tun(&pkt, l.now, l.unix);
            assert_eq!(l.a.peers[expected].queued.as_ref().unwrap().0, pkt);
            assert!(l.a.peers[1 - expected].queued.is_none());
        }
        let missing = build_ipv4(
            "10.77.0.1".parse().unwrap(),
            "192.168.99.9".parse().unwrap(),
            b"x",
        );
        // Retira también la ruta amplia: ya no debe quedar coincidencia.
        l.cfg_a.peers[0].routes.clear();
        l.a.apply_config(&l.cfg_a);
        assert!(l.a.on_tun(&missing, l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["tx_no_route"], 1);
    }

    #[test]
    fn ingress_never_transits_to_another_peer_address_or_route() {
        let mut l = subnet_lab();
        let mut third = l.cfg_b.peers[0].clone();
        third.device_id = [3; 16];
        third.noise_pubkey = StaticKeypair::generate().public;
        third.addrs = vec![ip("10.77.0.3")];
        third.routes = vec![Prefix {
            addr: ip("192.168.50.0"),
            len: 24,
        }];
        l.cfg_b.peers.push(third);
        l.b.apply_config(&l.cfg_b);
        let out = l.a.on_tun(&v4(1, 2, b"sesion"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        for dst in ["10.77.0.3", "192.168.50.9"] {
            let pkt = build_ipv4(
                "10.77.0.1".parse().unwrap(),
                dst.parse().unwrap(),
                b"transito",
            );
            let dg = l.a.peers[0]
                .current
                .as_mut()
                .unwrap()
                .encrypt(&pkt, l.now)
                .unwrap();
            assert!(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix).is_empty());
        }
        assert_eq!(l.b.counters.0["rx_bad_destination"], 2);
        assert!(l.b.peers[1].queued.is_none());
    }

    #[test]
    fn subnet_source_must_belong_to_the_authenticated_peer_or_local_advertised() {
        let mut l = subnet_lab();
        let forged = build_ipv4(
            "192.168.41.9".parse().unwrap(),
            "10.77.0.1".parse().unwrap(),
            b"x",
        );
        assert!(l.b.on_tun(&forged, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["tx_bad_source"], 1);
        let out = l.a.on_tun(&v4(1, 2, b"sesion"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        // La subred es de B, no de A: ni siquiera un par autenticado puede usarla.
        let forged = build_ipv4(
            "192.168.40.9".parse().unwrap(),
            "10.77.0.2".parse().unwrap(),
            b"x",
        );
        let dg = l.a.peers[0]
            .current
            .as_mut()
            .unwrap()
            .encrypt(&forged, l.now)
            .unwrap();
        assert!(l.b.on_udp(ep(A_EP), &dg, l.now, l.unix).is_empty());
        assert_eq!(l.b.counters.0["rx_bad_source"], 1);
    }

    #[test]
    fn route_prefix_acl_is_enforced_at_both_ends_without_implicit_route_permission() {
        let mut l = subnet_lab();
        let prefix = Prefix {
            addr: ip("192.168.40.0"),
            len: 24,
        };
        let rule = Rule {
            src: Selector::Device([1; 16]),
            dst: Selector::Prefix(prefix.clone()),
            proto: 0,
            port_min: 0,
            port_max: 0,
        };
        let request = build_ipv4(
            "10.77.0.1".parse().unwrap(),
            "192.168.40.9".parse().unwrap(),
            b"acl",
        );
        l.cfg_a.rules.clear();
        l.a.apply_config(&l.cfg_a);
        assert!(l.a.on_tun(&request, l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["tx_acl_denied"], 1);
        l.cfg_a.rules = vec![rule.clone()];
        l.cfg_b.rules.clear();
        l.a.apply_config(&l.cfg_a);
        l.b.apply_config(&l.cfg_b);
        let out = l.a.on_tun(&request, l.now, l.unix);
        assert!(pump(&mut l, out, vec![]).1.is_empty());
        assert_eq!(l.b.counters.0["rx_acl_denied"], 1);
        l.cfg_b.rules = vec![rule];
        l.b.apply_config(&l.cfg_b);
        let out = l.a.on_tun(&request, l.now, l.unix);
        assert_eq!(pump(&mut l, out, vec![]).1, vec![request]);
        let reply = build_ipv4(
            "192.168.40.9".parse().unwrap(),
            "10.77.0.1".parse().unwrap(),
            b"acl",
        );
        assert!(l.b.on_tun(&reply, l.now, l.unix).is_empty());
        let reverse = Rule {
            src: Selector::Prefix(prefix),
            dst: Selector::Device([1; 16]),
            proto: 0,
            port_min: 0,
            port_max: 0,
        };
        l.cfg_a.rules.push(reverse.clone());
        l.cfg_b.rules.push(reverse);
        l.a.apply_config(&l.cfg_a);
        l.b.apply_config(&l.cfg_b);
        let out = l.b.on_tun(&reply, l.now, l.unix);
        assert_eq!(pump(&mut l, vec![], out).0, vec![reply]);
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
        let first = l.b.on_udp(ep(A_EP), &dg, l.now, l.unix);
        assert_eq!(first.len(), 1);
        let again = l.b.on_udp(ep(A_EP), &dg, l.now, l.unix);
        assert_eq!(again.len(), 1);
        let (Out::Udp(_, r1), Out::Udp(_, r2)) = (&first[0], &again[0]) else {
            panic!()
        };
        assert_eq!(r1, r2, "se repite la misma respuesta");
        assert_eq!(l.b.counters.0["rx_init_retransmit"], 1);
        assert_eq!(l.b.counters.0["rx_init_stale"], 1);
        // otro mensaje 1 distinto con el mismo timestamp no es una retransmisión
        let (_, msg) = crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 7, l.unix).unwrap();
        let dg2 = proto::encode_init(7, &msg).unwrap();
        assert!(l.b.on_udp(ep(A_EP), &dg2, l.now, l.unix).is_empty());
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
        let keys = StaticKeypair::from_private(l.a.keys.private_bytes());
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

    #[test]
    fn roaming_never_adopts_an_endpoint_inside_the_tunnel() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let out = l.a.on_tun(&v4(1, 2, b"2"), l.now, l.unix);
        let Out::Udp(_, dg) = &out[0] else { panic!() };
        l.b.on_udp(ep("[fd77::99]:9"), dg, l.now, l.unix);
        assert_eq!(l.b.peers[0].endpoint, Some(ep(A_EP)));
    }

    fn garbage_init(sender: u32) -> Vec<u8> {
        proto::encode_init(sender, &[0u8; proto::NOISE_MSG1_LEN]).unwrap()
    }

    #[test]
    fn spoofed_init_flood_does_not_stop_a_configured_peer() {
        let mut l = lab();
        for i in 0..400u32 {
            let from = SocketAddr::new(
                IpAddr::from([198, 51, (i / 250) as u8, (i % 250) as u8 + 1]),
                9,
            );
            l.b.on_udp(from, &garbage_init(i), l.now, l.unix);
        }
        assert!(l.b.counters.0["rx_init_ratelimited"] >= 300);
        let out = l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
    }

    #[test]
    fn one_origin_cannot_exceed_its_own_quota() {
        let mut l = lab();
        let from = ep("198.51.100.9:9");
        for i in 0..50 {
            l.b.on_udp(from, &garbage_init(i), l.now, l.unix);
        }
        assert_eq!(l.b.counters.0["rx_init_invalid"], 5);
        assert_eq!(l.b.counters.0["rx_init_ratelimited"], 45);
    }

    #[test]
    fn forged_responses_do_not_cancel_a_pending_handshake() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        let index = l.a.peers[0].pending.as_ref().unwrap().0.local_index();
        let forged = proto::encode_resp(9, index, &[0u8; proto::NOISE_MSG2_LEN]).unwrap();
        assert!(l.a.on_udp(ep(B_EP), &forged, l.now, l.unix).is_empty());
        assert_eq!(l.a.counters.0["rx_resp_invalid"], 1);
        assert!(l.a.peers[0].pending.is_some());
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b.len(), 1);
    }

    #[test]
    fn init_older_than_the_floor_is_not_accepted() {
        let mut l = lab();
        l.b.init_floor = l.unix;
        let (_, old) =
            crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 3, l.unix - 1).unwrap();
        assert!(
            l.b.on_udp(
                ep(A_EP),
                &proto::encode_init(3, &old).unwrap(),
                l.now,
                l.unix
            )
            .is_empty()
        );
        assert_eq!(l.b.counters.0["rx_init_stale"], 1);
        let (_, new) = crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 4, l.unix).unwrap();
        assert_eq!(
            l.b.on_udp(
                ep(A_EP),
                &proto::encode_init(4, &new).unwrap(),
                l.now,
                l.unix
            )
            .len(),
            1
        );
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
        let mut e = Engine::new(NET, StaticKeypair::generate(), 0);
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

    // ---- Conexión directa (fase 4): candidatos, sondeo, confirmación de ruta, keepalive ----

    fn without_static_endpoints() -> Lab {
        let mut l = lab();
        l.cfg_a.peers[0].endpoints.clear();
        l.cfg_b.peers[0].endpoints.clear();
        let (ca, cb) = (l.cfg_a.clone(), l.cfg_b.clone());
        l.a.apply_config(&ca);
        l.b.apply_config(&cb);
        l
    }

    fn udp_of(outs: &[Out]) -> Vec<(SocketAddr, Vec<u8>)> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Udp(a, d) => Some((*a, d.clone())),
                Out::Tun(_) | Out::Relay { .. } => None,
            })
            .collect()
    }

    #[test]
    fn no_hints_means_rendezvous_not_blind_probing() {
        let mut l = without_static_endpoints();
        let out = l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        assert!(out.is_empty(), "sin candidatos no se envía nada");
        assert_eq!(l.a.take_rendezvous(l.now), vec![[2u8; 16]]);
        // el intervalo mínimo evita repetir el rendezvous
        assert!(l.a.take_rendezvous(l.now).is_empty());
    }

    #[test]
    fn hole_punching_both_sides_converge_on_a_direct_path() {
        let mut l = without_static_endpoints();
        l.a.on_tun(&v4(1, 2, b"hola"), l.now, l.unix);
        l.b.on_tun(&v4(2, 1, b"eco"), l.now, l.unix);
        // el coordinator entrega a cada lado los candidatos del otro
        let oa =
            l.a.on_peer_candidates(&[2; 16], vec![ep(B_EP)], l.now, l.unix);
        let ob =
            l.b.on_peer_candidates(&[1; 16], vec![ep(A_EP)], l.now, l.unix);
        assert_eq!(udp_of(&oa).len(), 1);
        let (tun_a, tun_b) = pump(&mut l, oa, ob);
        assert_eq!((tun_a.len(), tun_b.len()), (1, 1));
        assert_eq!(l.a.peers[0].path_name(l.now), "direct");
        assert_eq!(l.b.peers[0].path_name(l.now), "direct");
        assert_eq!(l.a.peers[0].endpoint, Some(ep(B_EP)));
        assert_eq!(l.b.peers[0].endpoint, Some(ep(A_EP)));
    }

    #[test]
    fn candidates_from_the_coordinator_are_sanitized_and_only_for_known_peers() {
        let mut l = without_static_endpoints();
        l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        let many: Vec<SocketAddr> = (1..=20)
            .map(|i| ep(&format!("198.51.100.{i}:5000")))
            .collect();
        let mut cands = vec![ep("127.0.0.1:9"), ep("[fe80::1]:9"), ep("0.0.0.0:9")];
        cands.extend(many);
        let out = l.a.on_peer_candidates(&[2; 16], cands, l.now, l.unix);
        let sent = udp_of(&out);
        assert_eq!(sent.len(), atenea_nat_traversal::MAX_CANDIDATES);
        assert!(
            sent.iter()
                .all(|(a, _)| a.ip().to_string().starts_with("198.51.100."))
        );
        // un par que no figura en la configuración se ignora
        assert!(
            l.a.on_peer_candidates(&[9; 16], vec![ep(B_EP)], l.now, l.unix)
                .is_empty()
        );
        assert_eq!(l.a.counters.0["coord_unknown_peer"], 1);
    }

    #[test]
    fn probing_resends_the_same_message_then_fails_and_backs_off() {
        let mut l = without_static_endpoints();
        l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        let first = udp_of(&l.a.on_peer_candidates(
            &[2; 16],
            vec![ep(B_EP), ep("203.0.113.9:7")],
            l.now,
            l.unix,
        ));
        assert_eq!(first.len(), 2);
        let mut resent = 0;
        for _ in 0..40 {
            l.now += Duration::from_millis(250);
            for (_, dg) in udp_of(&l.a.on_tick(l.now, l.unix)) {
                assert_eq!(dg, first[0].1, "se reenvía el mismo mensaje 1");
                resent += 1;
            }
        }
        assert!(
            (16..=26).contains(&resent),
            "≈ 1 ráfaga/s a 2 candidatos durante 10 s: {resent}"
        );
        assert_eq!(l.a.counters.0["probe_failed"], 1);
        assert_eq!(l.a.peers[0].path_name(l.now), "none");
        // tras fallar, retardo antes de pedir otro rendezvous o sondear de nuevo
        l.a.on_tun(&v4(1, 2, b"y"), l.now, l.unix);
        assert!(l.a.take_rendezvous(l.now).is_empty());
        l.now += Duration::from_secs(31);
        l.unix += 31;
        assert_eq!(l.a.take_rendezvous(l.now), vec![[2u8; 16]]);
    }

    #[test]
    fn an_init_from_another_address_never_redirects_the_confirmed_endpoint() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        assert_eq!(l.b.peers[0].endpoint, Some(ep(A_EP)));
        // Repetición/captura del mensaje 1 desde otra IP: se responde a ella, pero la ruta no cambia.
        l.unix += 1;
        let (_, msg) = crypto::initiate(&l.a.keys, &l.b.keys.public, &l.a.net, 77, l.unix).unwrap();
        let dg = proto::encode_init(77, &msg).unwrap();
        let out = l.b.on_udp(ep("203.0.113.66:40000"), &dg, l.now, l.unix);
        assert_eq!(udp_of(&out)[0].0, ep("203.0.113.66:40000"));
        assert_eq!(
            l.b.peers[0].endpoint,
            Some(ep(A_EP)),
            "el endpoint solo cambia con un paquete autenticado de la sesión"
        );
    }

    #[test]
    fn keepalive_keeps_the_path_and_silence_kills_it() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"1"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        // inactividad: a los 15 s cada extremo emite un keepalive hacia el endpoint confirmado
        l.now += KEEPALIVE_INTERVAL + Duration::from_secs(1);
        l.unix += 16;
        let ka = udp_of(&l.a.on_tick(l.now, l.unix));
        assert_eq!(ka.len(), 1);
        assert_eq!(ka[0].0, ep(B_EP));
        assert!(l.a.on_tick(l.now, l.unix).is_empty(), "uno por intervalo");
        // el keepalive llega y mantiene viva la ruta en el otro extremo
        let (_, _) = pump(&mut l, vec![Out::Udp(ep(B_EP), ka[0].1.clone())], vec![]);
        assert_eq!(l.b.counters.0["rx_keepalive"], 2);
        // silencio total del par: la ruta se da por perdida y se pide rendezvous si había tráfico
        l.a.peers[0].last_data = Some(l.now);
        l.now += PATH_DEAD_AFTER + Duration::from_secs(1);
        l.unix += 46;
        l.a.on_tick(l.now, l.unix);
        assert_eq!(l.a.counters.0["path_lost"], 1);
        assert_eq!(l.a.peers[0].endpoint, None);
        assert_eq!(l.a.peers[0].path_name(l.now), "none");
    }

    /// Avanza `secs` segundos con ticks cada 250 ms, entregando lo que se genere.
    fn run(l: &mut Lab, secs: u64) {
        for _ in 0..secs * 4 {
            l.now += Duration::from_millis(250);
            let (oa, ob) = (l.a.on_tick(l.now, l.unix), l.b.on_tick(l.now, l.unix));
            pump(l, oa, ob);
        }
    }

    #[test]
    fn repeated_candidates_do_not_extend_a_pending_probe() {
        let mut l = lab();
        l.a.on_tun(&v4(1, 2, b"a"), l.now, l.unix);
        let first = l.a.peers[0].pending.as_ref().unwrap().0.local_index();
        for _ in 0..40 {
            l.now += Duration::from_millis(250);
            l.a.on_peer_candidates(&[2; 16], vec![ep(B_EP)], l.now, l.unix);
            l.a.on_tick(l.now, l.unix);
        }
        // Puede empezar otro intento con pistas nuevas, pero el original ya debe haber caducado.
        assert_ne!(
            l.a.peers[0].pending.as_ref().map(|(p, _)| p.local_index()),
            Some(first)
        );
        assert!(l.a.counters.0["probe_failed"] >= 1);
    }

    #[test]
    fn relay_handshake_retries_after_the_first_datagrams_are_lost() {
        let mut l = lab();
        l.direct_ok = false;
        // Se pierden los mensajes iniciales, incluso los enviados al abrir el relay.
        l.a.on_tun(&v4(1, 2, b"a"), l.now, l.unix);
        l.b.on_tun(&v4(2, 1, b"b"), l.now, l.unix);
        l.a.set_relay_ready(&[2; 16], RelayKind::Udp, true, l.now);
        l.b.set_relay_ready(&[1; 16], RelayKind::Udp, true, l.now);
        run(&mut l, 3);
        assert_eq!(l.a.peers[0].path_name(l.now), "relay-udp");
        assert_eq!(l.b.peers[0].path_name(l.now), "relay-udp");
    }

    #[test]
    fn path_switches_to_relay_and_back_with_hysteresis() {
        let mut l = lab();
        let out = l.a.on_tun(&v4(1, 2, b"x"), l.now, l.unix);
        pump(&mut l, out, vec![]);
        let (ra, rb) = (
            l.a.set_relay_ready(&[2; 16], RelayKind::Udp, true, l.now),
            l.b.set_relay_ready(&[1; 16], RelayKind::Udp, true, l.now),
        );
        pump(&mut l, ra, rb);
        run(&mut l, 10);
        assert_eq!(l.a.peers[0].active, PathKind::Direct);
        // El directo deja de llegar: se pasa al relay en menos de 10 s sin perder la sesión.
        l.direct_ok = false;
        run(&mut l, 10);
        assert_eq!(l.a.peers[0].active, PathKind::RelayUdp);
        assert_eq!(l.b.peers[0].active, PathKind::RelayUdp);
        let out = l.a.on_tun(&v4(1, 2, b"por relay"), l.now, l.unix);
        let (_, tun_b) = pump(&mut l, out, vec![]);
        assert_eq!(tun_b, vec![v4(1, 2, b"por relay")]);
        assert_eq!(l.a.counters.0["handshakes_initiator"], 1, "misma sesión");
        // Vuelve el directo: no se regresa de inmediato (histéresis de 5 s).
        l.direct_ok = true;
        run(&mut l, 2);
        let early = l.a.peers[0].active;
        run(&mut l, 20);
        assert_eq!(early, PathKind::RelayUdp);
        assert_eq!(l.a.peers[0].active, PathKind::Direct);
        assert_eq!(l.a.counters.0["path_switches"], 2);
    }
}
