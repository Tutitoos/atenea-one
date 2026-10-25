//! Cliente de relay del agente (docs/protocol/relay-v2.md §6).

use atenea_identity::{DeviceId, SigningKey};
use atenea_relay_core as wire;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    sync::{OnceCell, mpsc, watch},
    task::JoinHandle,
    time::{Instant, timeout},
};

const QUEUE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum RelayKind {
    Udp,
    Tls,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RelayEndpoint {
    #[serde(with = "relay_id_hex")]
    pub relay_id: [u8; 16],
    pub token: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub udp: Option<String>,
    #[serde(default)]
    pub tls: Option<String>,
    #[serde(default)]
    pub tls_name: Option<String>,
}

mod relay_id_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(id: &[u8; 16], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&atenea_crypto::hex::encode(id))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 16], D::Error> {
        let text = String::deserialize(d)?;
        let id = atenea_crypto::hex::decode::<16>(&text)
            .filter(|id| *id != [0; 16] && atenea_crypto::hex::encode(id) == text)
            .ok_or_else(|| serde::de::Error::custom("relay_id inválido"))?;
        Ok(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayEvent {
    Ready {
        peer: DeviceId,
        relay_id: [u8; 16],
        kind: RelayKind,
    },
    Down {
        peer: DeviceId,
        relay_id: [u8; 16],
        kind: RelayKind,
    },
    Rx {
        peer: DeviceId,
        relay_id: [u8; 16],
        kind: RelayKind,
        datagram: Vec<u8>,
    },
    Rtt {
        peer: DeviceId,
        relay_id: [u8; 16],
        kind: RelayKind,
        ms: u32,
    },
}

#[derive(Debug, Clone, Default)]
pub struct RelayClientConfig {
    pub ca_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RelayStatus {
    pub allocations: usize,
    pub entries: Vec<AllocationStatus>,
    pub drops: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllocationStatus {
    pub peer: String,
    pub transport: RelayKind,
    pub relay: String,
    pub relay_id: String,
    pub state: String,
    pub peer_ready: bool,
    pub rtt_ms: Option<u32>,
    pub pings_sent: u64,
    pub pings_lost: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub drops: BTreeMap<String, u64>,
    pub last_error: Option<String>,
}

struct Entry {
    status: Arc<Mutex<AllocationStatus>>,
    grants: watch::Sender<Vec<RelayEndpoint>>,
    out: mpsc::Sender<([u8; 16], RelayKind, Vec<u8>)>,
    task: JoinHandle<()>,
}

#[derive(Clone, Copy)]
struct Timing {
    retry: Duration,
    ping: Duration,
    renew: Duration,
    connect: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            retry: Duration::from_secs(1),
            ping: Duration::from_secs(5),
            renew: Duration::from_secs(60),
            connect: Duration::from_secs(10),
        }
    }
}

struct Route {
    addr: SocketAddr,
    id: Option<u64>,
    awaiting: bool,
    input: mpsc::Sender<Vec<u8>>,
    status: Arc<Mutex<AllocationStatus>>,
}
struct Hub {
    socket: Arc<UdpSocket>,
    routes: Arc<Mutex<HashMap<DeviceId, Route>>>,
    gate: Arc<tokio::sync::Mutex<()>>,
    reader: JoinHandle<()>,
}
impl Drop for Hub {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

pub struct RelayClient {
    identity: SigningKey,
    device: DeviceId,
    network: [u8; 16],
    cfg: RelayClientConfig,
    events: mpsc::Sender<RelayEvent>,
    entries: Mutex<HashMap<DeviceId, Entry>>,
    hub: Arc<OnceCell<Hub>>,
    drops: Arc<Mutex<BTreeMap<String, u64>>>,
    timing: Timing,
}

fn count(drops: &mut BTreeMap<String, u64>, reason: &str) {
    *drops.entry(reason.into()).or_default() += 1;
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn alloc_error(code: wire::ErrCode) -> &'static str {
    match code {
        wire::ErrCode::UnsupportedVersion => "relay incompatible: se requiere relay-v2",
        wire::ErrCode::Malformed => "mensaje de relay inválido",
        wire::ErrCode::WrongRelay => "relay_id incorrecto",
        wire::ErrCode::Expired => "token_expired",
        _ => "alloc_rejected",
    }
}

pub(crate) fn load_roots(path: Option<&Path>) -> Result<rustls::RootCertStore, String> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = path {
        let pem = std::fs::read(path).map_err(|_| "relay_ca_file: no se puede leer el PEM")?;
        let mut n = 0;
        for cert in CertificateDer::pem_slice_iter(&pem) {
            roots
                .add(cert.map_err(|_| "relay_ca_file: PEM inválido")?)
                .map_err(|_| "relay_ca_file: certificado inválido")?;
            n += 1;
        }
        if n == 0 {
            return Err("relay_ca_file: PEM sin certificados".into());
        }
    }
    Ok(roots)
}

impl RelayClient {
    pub fn new(
        cfg: RelayClientConfig,
        identity: SigningKey,
        device: DeviceId,
        network: [u8; 16],
    ) -> (RelayClient, mpsc::Receiver<RelayEvent>) {
        let (events, rx) = mpsc::channel(QUEUE);
        (
            Self {
                identity,
                device,
                network,
                cfg,
                events,
                entries: Mutex::new(HashMap::new()),
                hub: Arc::new(OnceCell::new()),
                drops: Arc::new(Mutex::new(BTreeMap::new())),
                timing: Timing::default(),
            },
            rx,
        )
    }

    pub fn open(&self, peer: DeviceId, grants: Vec<RelayEndpoint>) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get(&peer).filter(|e| !e.task.is_finished()) {
            entry.grants.send_replace(grants);
            return;
        }
        let (grants_tx, grants_rx) = watch::channel(grants);
        let (out, rx) = mpsc::channel(QUEUE);
        let status = Arc::new(Mutex::new(AllocationStatus {
            peer: atenea_crypto::hex::encode(&peer),
            transport: RelayKind::Udp,
            relay: String::new(),
            relay_id: String::new(),
            state: "connecting".into(),
            peer_ready: false,
            rtt_ms: None,
            pings_sent: 0,
            pings_lost: 0,
            tx_bytes: 0,
            tx_packets: 0,
            rx_bytes: 0,
            rx_packets: 0,
            drops: BTreeMap::new(),
            last_error: None,
        }));
        let worker = Worker {
            peer,
            device: self.device,
            network: self.network,
            relay_id: [0; 16],
            identity: self.identity.clone(),
            cfg: self.cfg.clone(),
            events: self.events.clone(),
            status: status.clone(),
            hub: self.hub.clone(),
            drops: self.drops.clone(),
            timing: self.timing,
        };
        let task = tokio::spawn(worker.run(grants_rx, rx));
        entries.insert(
            peer,
            Entry {
                status,
                grants: grants_tx,
                out,
                task,
            },
        );
    }

    pub fn send(&self, peer: DeviceId, relay_id: [u8; 16], kind: RelayKind, datagram: Vec<u8>) {
        let entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get(&peer) else {
            count(&mut self.drops.lock().unwrap(), "not_ready");
            return;
        };
        let mut s = entry.status.lock().unwrap();
        let reason = if datagram.len() > wire::MAX_RELAYED_DATAGRAM {
            Some("oversized")
        } else if s.state != "ready"
            || s.transport != kind
            || s.relay_id != atenea_crypto::hex::encode(&relay_id)
        {
            Some("not_ready")
        } else if atenea_protocol::parse(&datagram).is_err() {
            Some("invalid_datagram")
        } else if entry.out.try_send((relay_id, kind, datagram)).is_err() {
            Some("queue_full")
        } else {
            None
        };
        if let Some(reason) = reason {
            count(&mut s.drops, reason);
        }
    }

    pub fn close(&self, peer: DeviceId) {
        if let Some(entry) = self.entries.lock().unwrap().remove(&peer) {
            entry.task.abort();
            if let Some(hub) = self.hub.get() {
                hub.routes.lock().unwrap().remove(&peer);
            }
        }
    }

    pub fn status(&self) -> RelayStatus {
        let entries = self.entries.lock().unwrap();
        let mut statuses: Vec<_> = entries
            .values()
            .map(|e| e.status.lock().unwrap().clone())
            .collect();
        statuses.sort_by(|a, b| a.peer.cmp(&b.peer));
        RelayStatus {
            allocations: statuses.len(),
            entries: statuses,
            drops: self.drops.lock().unwrap().clone(),
        }
    }
}
impl Drop for RelayClient {
    fn drop(&mut self) {
        for entry in self.entries.get_mut().unwrap().values() {
            entry.task.abort();
        }
    }
}

struct Worker {
    peer: DeviceId,
    device: DeviceId,
    network: [u8; 16],
    relay_id: [u8; 16],
    identity: SigningKey,
    cfg: RelayClientConfig,
    events: mpsc::Sender<RelayEvent>,
    status: Arc<Mutex<AllocationStatus>>,
    hub: Arc<OnceCell<Hub>>,
    drops: Arc<Mutex<BTreeMap<String, u64>>>,
    timing: Timing,
}

enum Link {
    Udp {
        socket: Arc<UdpSocket>,
        addr: SocketAddr,
        input: mpsc::Receiver<Vec<u8>>,
    },
    Tls {
        stream: Box<tokio_rustls::client::TlsStream<TcpStream>>,
        decoder: wire::FrameDecoder,
    },
}
impl Link {
    async fn send(&mut self, body: &[u8]) -> Result<(), &'static str> {
        match self {
            Self::Udp { socket, addr, .. } => {
                socket.send_to(body, *addr).await.map_err(|_| "udp_write")?;
            }
            Self::Tls { stream, .. } => {
                stream
                    .write_all(&wire::encode_frame(body).map_err(|_| "frame")?)
                    .await
                    .map_err(|_| "tls_write")?;
            }
        }
        Ok(())
    }
    async fn recv(&mut self) -> Result<Vec<u8>, &'static str> {
        match self {
            Self::Udp { input, .. } => input.recv().await.ok_or("udp_closed"),
            Self::Tls { stream, decoder } => loop {
                if let Some(frame) = decoder.next_frame().map_err(|_| "frame")? {
                    return Ok(frame);
                }
                let mut buf = [0; 2048];
                let n = stream.read(&mut buf).await.map_err(|_| "tls_read")?;
                if n == 0 {
                    return Err("tls_closed");
                }
                decoder.push(&buf[..n]);
            },
        }
    }
}

impl Worker {
    fn grant_token(&self, grants: &[RelayEndpoint]) -> Result<Vec<u8>, &'static str> {
        use base64::Engine as _;
        if grants.is_empty()
            || grants.len() > 4
            || grants.iter().enumerate().any(|(i, r)| {
                r.relay_id == [0; 16]
                    || grants[..i].iter().any(|other| other.relay_id == r.relay_id)
            })
        {
            return Err("concesión relay-v2 inválida");
        }
        let grant = grants
            .iter()
            .find(|r| r.relay_id == self.relay_id)
            .ok_or("concesión relay-v2 retirada")?;
        let token = base64::engine::general_purpose::STANDARD
            .decode(&grant.token)
            .map_err(|_| "token_malformed")?;
        self.valid_token(&token)?;
        Ok(token)
    }
    fn event(&self, event: RelayEvent) {
        if self.events.try_send(event).is_err() {
            count(&mut self.status.lock().unwrap().drops, "event_queue_full");
        }
    }
    fn alloc(&self, token: &[u8], cookie: &[u8; 16]) -> Vec<u8> {
        let ts = now();
        wire::encode_alloc(
            &self.relay_id,
            cookie,
            ts,
            token,
            &wire::sign_alloc(&self.identity, &self.relay_id, cookie, ts, token),
        )
    }
    fn valid_token(&self, token: &[u8]) -> Result<u64, &'static str> {
        let t = wire::RelayToken::parse(token).map_err(|_| "token_malformed")?;
        if t.relay_id != self.relay_id
            || t.network_id != self.network
            || t.src_device != self.device
            || t.src_device == t.dst_device
            || t.dst_device != self.peer
            || t.src_identity_pub != self.identity.verifying_key().to_bytes()
        {
            return Err("token_identity");
        }
        if t.not_after <= now()
            || t.not_before > now().saturating_add(30)
            || t.not_after <= t.not_before
            || t.not_after - t.not_before > wire::MAX_TOKEN_TTL_SECS
        {
            return Err("token_expired");
        }
        Ok(t.not_after)
    }
    async fn udp(
        &self,
        endpoint: &str,
        token: &[u8],
    ) -> Result<(Link, Vec<u8>, [u8; 16]), &'static str> {
        let addr = timeout(self.timing.connect, tokio::net::lookup_host(endpoint))
            .await
            .map_err(|_| "dns_timeout")?
            .map_err(|_| "dns")?
            .next()
            .ok_or("dns_empty")?;
        let addr = match addr {
            SocketAddr::V4(v4) => SocketAddr::new(v4.ip().to_ipv6_mapped().into(), v4.port()),
            v6 => v6,
        };
        let drops = self.drops.clone();
        let hub = self
            .hub
            .get_or_try_init(|| async {
                let socket = socket2::Socket::new(
                    socket2::Domain::IPV6,
                    socket2::Type::DGRAM,
                    Some(socket2::Protocol::UDP),
                )
                .map_err(|_| "udp_socket")?;
                socket.set_only_v6(false).map_err(|_| "udp_dual_stack")?;
                socket
                    .set_nonblocking(true)
                    .map_err(|_| "udp_nonblocking")?;
                socket
                    .bind(&"[::]:0".parse::<SocketAddr>().unwrap().into())
                    .map_err(|_| "udp_bind")?;
                let socket =
                    Arc::new(UdpSocket::from_std(socket.into()).map_err(|_| "udp_socket")?);
                let routes: Arc<Mutex<HashMap<DeviceId, Route>>> =
                    Arc::new(Mutex::new(HashMap::new()));
                let (s, r) = (socket.clone(), routes.clone());
                let reader = tokio::spawn(async move {
                    let mut buf = [0; wire::MAX_FRAME + 1];
                    while let Ok((n, src)) = s.recv_from(&mut buf).await {
                        let msg = wire::parse(&buf[..n]);
                        let id = match &msg {
                            Ok(
                                wire::Message::Data { alloc_id, .. }
                                | wire::Message::Pong { alloc_id, .. }
                                | wire::Message::PeerState { alloc_id, .. },
                            ) => Some(*alloc_id),
                            _ => None,
                        };
                        let routes = r.lock().unwrap();
                        let route = routes.values().find(|route| {
                            route.addr == src
                                && if let Some(id) = id {
                                    route.id == Some(id)
                                } else {
                                    route.awaiting
                                }
                        });
                        if let Some(route) = route {
                            if route.input.try_send(buf[..n].to_vec()).is_err() {
                                count(&mut route.status.lock().unwrap().drops, "input_queue_full");
                            }
                        } else {
                            count(
                                &mut drops.lock().unwrap(),
                                if msg.is_err() {
                                    "malformed"
                                } else {
                                    "unknown_origin_or_allocation"
                                },
                            );
                        }
                    }
                });
                Ok::<_, &'static str>(Hub {
                    socket,
                    routes,
                    gate: Arc::new(tokio::sync::Mutex::new(())),
                    reader,
                })
            })
            .await?;
        // ALLOC_OK no lleva audiencia: una negociación UDP a la vez por cliente.
        let _guard = hub.gate.lock().await;
        let (tx, input) = mpsc::channel(QUEUE);
        hub.routes.lock().unwrap().insert(
            self.peer,
            Route {
                addr,
                id: None,
                awaiting: true,
                input: tx,
                status: self.status.clone(),
            },
        );
        let mut link = Link::Udp {
            socket: hub.socket.clone(),
            addr,
            input,
        };
        let mut cookie = None;
        for _ in 0..3 {
            link.send(&wire::encode_cookie_req(&self.relay_id)).await?;
            let until = Instant::now() + self.timing.retry;
            loop {
                let body = match tokio::time::timeout_at(until, link.recv()).await {
                    Ok(Ok(body)) => body,
                    _ => break,
                };
                match wire::parse(&body) {
                    Ok(wire::Message::Cookie {
                        relay_id,
                        cookie: c,
                    }) if relay_id == self.relay_id => {
                        cookie = Some(c);
                        link.send(&self.alloc(token, &c)).await?;
                    }
                    Ok(wire::Message::AllocOk { alloc_id, .. }) if let Some(cookie) = cookie => {
                        let mut routes = hub.routes.lock().unwrap();
                        let route = routes.get_mut(&self.peer).ok_or("closed")?;
                        route.id = Some(alloc_id);
                        route.awaiting = false;
                        return Ok((link, body, cookie));
                    }
                    Ok(wire::Message::AllocErr { code }) if cookie.is_some() => {
                        return Err(alloc_error(code));
                    }
                    _ => count(&mut self.status.lock().unwrap().drops, "unexpected"),
                }
            }
        }
        hub.routes.lock().unwrap().remove(&self.peer);
        Err("udp_timeout")
    }

    async fn tls(
        &self,
        endpoint: &RelayEndpoint,
        token: &[u8],
    ) -> Result<(Link, Vec<u8>, [u8; 16]), &'static str> {
        let addr = endpoint.tls.as_ref().ok_or("tls_unavailable")?;
        let name =
            rustls::pki_types::ServerName::try_from(endpoint.tls_name.clone().ok_or("tls_name")?)
                .map_err(|_| "tls_name")?;
        let roots = load_roots(self.cfg.ca_file.as_deref()).map_err(|_| "tls_ca")?;
        let mut cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| "tls_config")?
        .with_root_certificates(roots)
        .with_no_client_auth();
        cfg.alpn_protocols = vec![wire::ALPN.to_vec()];
        let stream = TcpStream::connect(addr).await.map_err(|_| "tcp_connect")?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(cfg))
            .connect(name, stream)
            .await
            .map_err(|e| {
                if e.to_string().contains("NoApplicationProtocol") {
                    "relay incompatible: se requiere relay-v2"
                } else {
                    "tls_handshake"
                }
            })?;
        if stream.get_ref().1.alpn_protocol() != Some(wire::ALPN) {
            return Err("relay incompatible: se requiere relay-v2");
        }
        let mut link = Link::Tls {
            stream: Box::new(stream),
            decoder: wire::FrameDecoder::new(),
        };
        link.send(&self.alloc(token, &[0; 16])).await?;
        let first = link.recv().await?;
        Ok((link, first, [0; 16]))
    }

    async fn run(
        mut self,
        mut grants: watch::Receiver<Vec<RelayEndpoint>>,
        mut out: mpsc::Receiver<([u8; 16], RelayKind, Vec<u8>)>,
    ) {
        let relays = grants.borrow_and_update().clone();
        if relays.is_empty()
            || relays.len() > 4
            || relays.iter().enumerate().any(|(i, r)| {
                r.relay_id == [0; 16]
                    || relays[..i].iter().any(|other| other.relay_id == r.relay_id)
            })
        {
            self.down("concesión relay-v2 inválida");
            return;
        }
        for endpoint in &relays {
            self.relay_id = endpoint.relay_id;
            if let Err(e) = self.grant_token(&relays) {
                self.down(e);
                return;
            }
        }
        let mut error = "relay_unavailable";
        for endpoint in relays {
            self.relay_id = endpoint.relay_id;
            let bytes = match self.grant_token(&grants.borrow()) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.down(e);
                    return;
                }
            };
            {
                let mut status = self.status.lock().unwrap();
                status.relay = endpoint.name.clone();
                status.relay_id = atenea_crypto::hex::encode(&self.relay_id);
                status.state = "connecting".into();
            }
            if let Some(addr) = &endpoint.udp {
                self.status.lock().unwrap().transport = RelayKind::Udp;
                match self.udp(addr, &bytes).await {
                    Ok((link, first, cookie)) => {
                        error = self
                            .connected(link, first, cookie, &mut grants, &mut out)
                            .await;
                        self.down(error);
                    }
                    Err(e) => {
                        error = e;
                        self.down(error);
                    }
                }
                if let Some(hub) = self.hub.get() {
                    hub.routes.lock().unwrap().remove(&self.peer);
                }
                if matches!(error, "token_expired" | "token_identity") {
                    return;
                }
            }
            if endpoint.tls.is_some() {
                {
                    let mut s = self.status.lock().unwrap();
                    s.transport = RelayKind::Tls;
                    s.state = "connecting".into();
                }
                let bytes = match self.grant_token(&grants.borrow()) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        self.down(e);
                        return;
                    }
                };
                match timeout(self.timing.connect, self.tls(&endpoint, &bytes)).await {
                    Ok(Ok((link, first, cookie))) => {
                        error = self
                            .connected(link, first, cookie, &mut grants, &mut out)
                            .await;
                    }
                    Ok(Err(e)) => error = e,
                    Err(_) => error = "tls_timeout",
                }
                self.down(error);
                if matches!(error, "token_expired" | "token_identity") {
                    return;
                }
            }
        }
        self.down(error);
    }

    fn down(&self, error: &str) {
        let kind = {
            let mut s = self.status.lock().unwrap();
            if s.state == "down" {
                return;
            }
            s.state = "down".into();
            s.peer_ready = false;
            s.last_error = Some(error.into());
            s.transport
        };
        self.event(RelayEvent::Down {
            peer: self.peer,
            relay_id: self.relay_id,
            kind,
        });
    }

    async fn renew(
        &self,
        link: &mut Link,
        bytes: &[u8],
    ) -> Result<Option<tokio::sync::OwnedMutexGuard<()>>, &'static str> {
        if matches!(link, Link::Udp { .. }) {
            let hub = self.hub.get().ok_or("closed")?;
            let guard = timeout(self.timing.connect, hub.gate.clone().lock_owned())
                .await
                .map_err(|_| "renew_busy")?;
            hub.routes
                .lock()
                .unwrap()
                .get_mut(&self.peer)
                .ok_or("closed")?
                .awaiting = true;
            timeout(
                self.timing.connect,
                link.send(&wire::encode_cookie_req(&self.relay_id)),
            )
            .await
            .map_err(|_| "renew_timeout")??;
            Ok(Some(guard))
        } else {
            timeout(self.timing.connect, link.send(&self.alloc(bytes, &[0; 16])))
                .await
                .map_err(|_| "renew_timeout")??;
            Ok(None)
        }
    }

    async fn connected(
        &self,
        mut link: Link,
        first: Vec<u8>,
        _cookie: [u8; 16],
        token: &mut watch::Receiver<Vec<RelayEndpoint>>,
        out: &mut mpsc::Receiver<([u8; 16], RelayKind, Vec<u8>)>,
    ) -> &'static str {
        let (mut id, mut deadline) = match wire::parse(&first) {
            Ok(wire::Message::AllocOk {
                alloc_id,
                ttl_secs,
                peer_ready,
            }) if ttl_secs > 0 && ttl_secs <= 120 => {
                let mut s = self.status.lock().unwrap();
                s.state = "ready".into();
                s.peer_ready = peer_ready;
                s.last_error = None;
                (
                    alloc_id,
                    Instant::now() + Duration::from_secs(ttl_secs.into()),
                )
            }
            Ok(wire::Message::AllocErr { code }) => return alloc_error(code),
            _ => return "alloc_response",
        };
        let kind = self.status.lock().unwrap().transport;
        self.event(RelayEvent::Ready {
            peer: self.peer,
            relay_id: self.relay_id,
            kind,
        });
        let mut ping = tokio::time::interval(self.timing.ping);
        let mut renew =
            tokio::time::interval_at(Instant::now() + self.timing.renew, self.timing.renew);
        let mut pending: Option<(u64, Instant)> = None;
        let mut nonce = 0u64;
        let mut missed = 0;
        let mut renewal = None;
        loop {
            let bytes = match self.grant_token(&token.borrow()) {
                Ok(bytes) => bytes,
                Err(e) => return e,
            };
            let expiry = match self.valid_token(&bytes) {
                Ok(t) => t,
                Err(e) => return e,
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(expiry.saturating_sub(now()))) => return "token_expired",
                _ = tokio::time::sleep_until(deadline) => return "allocation_expired",
                changed = token.changed() => {
                    if changed.is_err() { return "closed"; }
                    token.borrow_and_update();
                    let bytes = match self.grant_token(&token.borrow()) { Ok(bytes) => bytes, Err(e) => return e };
                    if let Err(e) = self.valid_token(&bytes) { return e; }
                    if renewal.is_none() { match self.renew(&mut link, &bytes).await { Ok(guard) => renewal = guard, Err(e) => return e } }
                }
                _ = renew.tick() => {
                    let bytes = match self.grant_token(&token.borrow()) { Ok(bytes) => bytes, Err(e) => return e };
                    if renewal.is_none() { match self.renew(&mut link, &bytes).await { Ok(guard) => renewal = guard, Err(e) => return e } }
                }
                _ = ping.tick() => {
                    { let mut s = self.status.lock().unwrap(); if pending.is_some() { s.pings_lost += 1; missed += 1; } s.pings_sent += 1; }
                    nonce = nonce.wrapping_add(1); pending = Some((nonce, Instant::now()));
                    if timeout(self.timing.connect, link.send(&wire::encode_ping(id, nonce, false))).await != Ok(Ok(())) { return "ping_write"; }
                    if missed >= 3 { return "ping_timeout"; }
                }
                data = out.recv() => {
                    let Some((relay_id, requested, data)) = data else { return "closed"; };
                    if relay_id != self.relay_id || requested != kind { count(&mut self.status.lock().unwrap().drops, "not_ready"); continue; }
                    if timeout(self.timing.connect, link.send(&wire::encode_data(id, &data))).await != Ok(Ok(())) { return "data_write"; }
                    let mut s = self.status.lock().unwrap(); s.tx_packets += 1; s.tx_bytes += data.len() as u64;
                }
                body = link.recv() => {
                    let body = match body { Ok(b) => b, Err(e) => return e };
                    match wire::parse(&body) {
                        Ok(wire::Message::Cookie { relay_id, cookie }) if renewal.is_some() && relay_id == self.relay_id => {
                            let bytes = match self.grant_token(&token.borrow()) { Ok(bytes) => bytes, Err(e) => return e };
                            if timeout(self.timing.connect, link.send(&self.alloc(&bytes, &cookie))).await != Ok(Ok(())) { return "renew_write"; }
                        }
                        Ok(wire::Message::AllocOk { alloc_id, ttl_secs, peer_ready }) if ttl_secs > 0 && ttl_secs <= 120 => {
                            id = alloc_id; deadline = Instant::now() + Duration::from_secs(ttl_secs.into()); self.status.lock().unwrap().peer_ready = peer_ready;
                            if let Some(hub) = self.hub.get() && let Some(route) = hub.routes.lock().unwrap().get_mut(&self.peer) { route.id = Some(id); route.awaiting = false; }
                            renewal = None;
                        }
                        Ok(wire::Message::AllocErr { code }) => return alloc_error(code),
                        Ok(wire::Message::Data { alloc_id, datagram }) if alloc_id == id && datagram.len() <= wire::MAX_RELAYED_DATAGRAM => {
                            { let mut s = self.status.lock().unwrap(); s.rx_packets += 1; s.rx_bytes += datagram.len() as u64; }
                            self.event(RelayEvent::Rx { peer: self.peer, relay_id: self.relay_id, kind, datagram: datagram.to_vec() });
                        }
                        Ok(wire::Message::PeerState { alloc_id, ready }) if alloc_id == id => self.status.lock().unwrap().peer_ready = ready,
                        Ok(wire::Message::Pong { alloc_id, nonce }) if alloc_id == id => {
                            if let Some((expected, sent)) = pending { if expected == nonce {
                                let measured = sent.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                                let ms = { let mut s = self.status.lock().unwrap(); let ms = s.rtt_ms.map_or(measured, |old| ((u64::from(old) * 7 + u64::from(measured)) / 8) as u32); s.rtt_ms = Some(ms); ms };
                                pending = None; missed = 0; self.event(RelayEvent::Rtt { peer: self.peer, relay_id: self.relay_id, kind, ms });
                            } else { count(&mut self.status.lock().unwrap().drops, "unexpected_pong"); } }
                        }
                        _ => count(&mut self.status.lock().unwrap().drops, "invalid_message_or_allocation"),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn config_validates_relay_ca_file() {
        let base = "interface = \"a0\"\nlisten = \"0.0.0.0:1\"\nipc_socket = \"/s\"\nnoise_key_file = \"/n\"\nidentity_key_file = \"/i\"\nconfig_file = \"/c\"\nstate_dir = \"/d\"\nnetwork_id = \"00112233445566778899aabbccddeeff\"\ndevice_id = \"ffeeddccbbaa99887766554433221100\"\ntrust_anchor = \"0000000000000000000000000000000000000000000000000000000000000001\"\n[coordinator]\nurl = \"https://coord.test\"\nstun = [\"s:1\", \"s:2\"]\n";
        let path =
            std::env::temp_dir().join(format!("atenea-config-ca-{}.pem", std::process::id()));
        std::fs::write(&path, "not a certificate").unwrap();
        let text = format!("{base}relay_ca_file = {:?}", path.to_string_lossy());
        assert!(crate::config::Config::parse(&text).is_err());
        let cert = rcgen::generate_simple_self_signed(vec!["relay.test".into()]).unwrap();
        std::fs::write(&path, cert.cert.pem()).unwrap();
        assert!(crate::config::Config::parse(&text).is_ok());
        std::fs::remove_file(path).unwrap();
    }

    fn identity() -> SigningKey {
        SigningKey::from_bytes(&[42; 32])
    }
    fn token(peer: DeviceId) -> Vec<u8> {
        wire::RelayToken::issue(
            &SigningKey::from_bytes(&[3; 32]),
            [6; 16],
            [1; 16],
            [2; 16],
            peer,
            identity().verifying_key().to_bytes(),
            now().saturating_sub(1),
            now() + 300,
            [4; 8],
        )
        .encode()
        .to_vec()
    }
    fn token_b64(peer: DeviceId) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(token(peer))
    }

    fn datagram_bytes() -> Vec<u8> {
        let mut bytes = vec![0; atenea_protocol::DATA_OVERHEAD];
        bytes[0] = atenea_protocol::VERSION;
        bytes[1] = atenea_protocol::TYPE_DATA;
        bytes
    }

    #[tokio::test]
    async fn grants_reject_incoherent_context_before_connecting() {
        use base64::Engine as _;
        let peer = [5; 16];
        let good = RelayEndpoint {
            relay_id: [6; 16],
            token: token_b64(peer),
            name: "lab".into(),
            udp: Some("198.51.100.1:443".into()),
            tls: None,
            tls_name: None,
        };
        for offset in [4, 5, 21, 37, 53, 69] {
            let (client, mut events) = client(None);
            let mut bad = good.clone();
            let mut bytes = token(peer);
            bytes[offset] ^= 1;
            bad.token = base64::engine::general_purpose::STANDARD.encode(bytes);
            client.open(peer, vec![bad]);
            event(&mut events, |e| matches!(e, RelayEvent::Down { .. })).await;
            assert!(client.hub.get().is_none());
        }
        for grants in [Vec::new(), vec![good.clone(); 2], vec![good; 5]] {
            let (client, mut events) = client(None);
            client.open(peer, grants);
            event(&mut events, |e| matches!(e, RelayEvent::Down { .. })).await;
            assert!(client.hub.get().is_none());
        }
    }

    #[tokio::test]
    async fn fallback_uses_the_next_relays_own_grant() {
        use base64::Engine as _;
        let (addr, _, server) = udp_server([9; 16], false).await;
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (client, mut events) = client(None);
        let second_token = wire::RelayToken::issue(
            &SigningKey::from_bytes(&[3; 32]),
            [9; 16],
            [1; 16],
            [2; 16],
            [5; 16],
            identity().verifying_key().to_bytes(),
            now().saturating_sub(1),
            now() + 300,
            [4; 8],
        )
        .encode();
        client.open(
            [5; 16],
            vec![
                RelayEndpoint {
                    relay_id: [6; 16],
                    token: token_b64([5; 16]),
                    name: "silencioso".into(),
                    udp: Some(silent.local_addr().unwrap().to_string()),
                    tls: None,
                    tls_name: None,
                },
                RelayEndpoint {
                    relay_id: [9; 16],
                    token: base64::engine::general_purpose::STANDARD.encode(second_token),
                    name: "segundo".into(),
                    udp: Some(addr),
                    tls: None,
                    tls_name: None,
                },
            ],
        );
        event(
            &mut events,
            |e| matches!(e, RelayEvent::Ready { relay_id, .. } if *relay_id == [9; 16]),
        )
        .await;
        client.send([5; 16], [6; 16], RelayKind::Udp, datagram_bytes());
        assert_eq!(client.status().entries[0].drops["not_ready"], 1);
        server.abort();
    }

    #[test]
    fn allocation_errors_are_explicit() {
        assert_eq!(
            alloc_error(wire::ErrCode::WrongRelay),
            "relay_id incorrecto"
        );
        assert_eq!(
            alloc_error(wire::ErrCode::UnsupportedVersion),
            "relay incompatible: se requiere relay-v2"
        );
        assert_eq!(
            alloc_error(wire::ErrCode::Malformed),
            "mensaje de relay inválido"
        );
    }

    #[tokio::test]
    async fn tls_reports_v2_errors_and_refuses_v1_alpn() {
        for code in [
            wire::ErrCode::WrongRelay,
            wire::ErrCode::UnsupportedVersion,
            wire::ErrCode::Malformed,
        ] {
            let (cfg, ca) = tls_config();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut stream = tokio_rustls::TlsAcceptor::from(cfg)
                    .accept(tcp)
                    .await
                    .unwrap();
                let mut length = [0; 2];
                stream.read_exact(&mut length).await.unwrap();
                let mut body = vec![0; usize::from(u16::from_be_bytes(length))];
                stream.read_exact(&mut body).await.unwrap();
                assert!(
                    matches!(wire::parse(&body), Ok(wire::Message::Alloc { relay_id, .. }) if relay_id == [6; 16])
                );
                stream
                    .write_all(&wire::encode_frame(&wire::encode_alloc_err(code)).unwrap())
                    .await
                    .unwrap();
            });
            let (client, mut events) = client(Some(ca.0.clone()));
            client.open(
                [5; 16],
                vec![RelayEndpoint {
                    relay_id: [6; 16],
                    token: token_b64([5; 16]),
                    name: "lab".into(),
                    udp: None,
                    tls: Some(addr),
                    tls_name: Some("relay.test".into()),
                }],
            );
            event(&mut events, |e| matches!(e, RelayEvent::Down { .. })).await;
            assert_eq!(
                client.status().entries[0].last_error.as_deref(),
                Some(alloc_error(code))
            );
            server.await.unwrap();
        }
        let (cfg, ca) = tls_config();
        let mut cfg = (*cfg).clone();
        cfg.alpn_protocols = vec![b"atenea-relay/1".to_vec()];
        let (addr, _, server) = tls_server(Arc::new(cfg), false).await;
        let (client, mut events) = client(Some(ca.0.clone()));
        client.open(
            [5; 16],
            vec![RelayEndpoint {
                relay_id: [6; 16],
                token: token_b64([5; 16]),
                name: "lab".into(),
                udp: None,
                tls: Some(addr),
                tls_name: Some("relay.test".into()),
            }],
        );
        event(&mut events, |e| matches!(e, RelayEvent::Down { .. })).await;
        assert_eq!(
            client.status().entries[0].last_error.as_deref(),
            Some("relay incompatible: se requiere relay-v2")
        );
        server.abort();
    }
    fn client(ca_file: Option<std::path::PathBuf>) -> (RelayClient, mpsc::Receiver<RelayEvent>) {
        let (mut client, events) =
            RelayClient::new(RelayClientConfig { ca_file }, identity(), [2; 16], [1; 16]);
        client.timing = Timing {
            retry: Duration::from_millis(25),
            ping: Duration::from_millis(30),
            renew: Duration::from_millis(70),
            connect: Duration::from_secs(2),
        };
        (client, events)
    }
    async fn event(
        rx: &mut mpsc::Receiver<RelayEvent>,
        matches: impl Fn(&RelayEvent) -> bool,
    ) -> RelayEvent {
        timeout(Duration::from_secs(4), async {
            loop {
                let e = rx.recv().await.expect("event channel");
                if matches(&e) {
                    return e;
                }
            }
        })
        .await
        .expect("relay event timeout")
    }

    async fn udp_server(
        relay_id: [u8; 16],
        reject: bool,
    ) -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap().to_string();
        let allocations = Arc::new(AtomicUsize::new(0));
        let count = allocations.clone();
        let task = tokio::spawn(async move {
            let cookies = wire::CookieMaker::new([9; 32], relay_id);
            let mut buf = [0; wire::MAX_FRAME];
            loop {
                let (n, src) = socket.recv_from(&mut buf).await.unwrap();
                let response = match wire::parse(&buf[..n]).unwrap() {
                    wire::Message::CookieReq {
                        relay_id: requested,
                    } => {
                        assert_eq!(requested, relay_id);
                        assert_eq!(n, 64);
                        assert!(buf[19..n].iter().all(|b| *b == 0));
                        socket
                            .send_to(&wire::encode_cookie(&[99; 16], &[1; 16]), src)
                            .await
                            .unwrap();
                        let attacker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                        attacker
                            .send_to(&wire::encode_cookie(&relay_id, &[1; 16]), src)
                            .await
                            .unwrap();
                        wire::encode_cookie(&relay_id, &cookies.cookie(src, now()))
                    }
                    wire::Message::Alloc {
                        relay_id,
                        cookie,
                        ts,
                        token,
                        sig,
                    } => {
                        assert!(cookies.verify(src, &cookie, now()));
                        let t = wire::RelayToken::parse(token).unwrap();
                        assert_eq!(t.relay_id, relay_id);
                        t.verify(
                            &[SigningKey::from_bytes(&[3; 32]).verifying_key().to_bytes()],
                            now(),
                        )
                        .unwrap();
                        assert!(wire::verify_alloc(
                            &t.src_identity_pub,
                            &relay_id,
                            &cookie,
                            ts,
                            token,
                            &sig
                        ));
                        count.fetch_add(1, Ordering::SeqCst);
                        if reject {
                            wire::encode_alloc_err(wire::ErrCode::Unauthorized)
                        } else {
                            wire::encode_alloc_ok(77, 120, true)
                        }
                    }
                    wire::Message::Ping { alloc_id, nonce } => {
                        wire::encode_ping(alloc_id, nonce, true)
                    }
                    wire::Message::Data { alloc_id, datagram } => {
                        assert_eq!(alloc_id, 77);
                        socket
                            .send_to(&wire::encode_data(999, &datagram_bytes()), src)
                            .await
                            .unwrap();
                        let attacker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                        attacker
                            .send_to(&wire::encode_data(77, &datagram_bytes()), src)
                            .await
                            .unwrap();
                        socket
                            .send_to(&wire::encode_peer_state(77, false), src)
                            .await
                            .unwrap();
                        wire::encode_data(77, datagram)
                    }
                    _ => panic!("unexpected message"),
                };
                socket.send_to(&response, src).await.unwrap();
            }
        });
        (addr, allocations, task)
    }

    #[tokio::test]
    async fn udp_cookie_proof_data_rtt_renewal_and_close() {
        let (addr, allocations, server) = udp_server([6; 16], false).await;
        let (client, mut events) = client(None);
        let peer = [5; 16];
        client.open(
            peer,
            vec![RelayEndpoint {
                relay_id: [6; 16],
                token: token_b64(peer),
                name: "lab".into(),
                udp: Some(addr),
                tls: None,
                tls_name: None,
            }],
        );
        event(&mut events, |e| {
            matches!(
                e,
                RelayEvent::Ready {
                    kind: RelayKind::Udp,
                    ..
                }
            )
        })
        .await;
        client.send(peer, [6; 16], RelayKind::Udp, datagram_bytes());
        assert!(
            matches!(event(&mut events, |e| matches!(e, RelayEvent::Rx { .. })).await, RelayEvent::Rx { relay_id, datagram, .. } if relay_id == [6; 16] && datagram == datagram_bytes())
        );
        let mut legacy = datagram_bytes();
        legacy[0] = 1;
        client.send(peer, [6; 16], RelayKind::Udp, legacy);
        client.send(peer, [6; 16], RelayKind::Udp, vec![2, 3]);
        client.send(
            peer,
            [6; 16],
            RelayKind::Udp,
            vec![0; wire::MAX_RELAYED_DATAGRAM + 1],
        );
        client.send(peer, [6; 16], RelayKind::Tls, vec![1, 2]);
        timeout(Duration::from_secs(2), async {
            while allocations.load(Ordering::SeqCst) < 2 {
                event(&mut events, |e| matches!(e, RelayEvent::Rtt { .. })).await;
            }
        })
        .await
        .unwrap();
        let status = client.status();
        assert_eq!(status.allocations, 1);
        assert!(status.entries[0].rtt_ms.is_some());
        assert_eq!(status.entries[0].tx_packets, 1);
        assert_eq!(status.entries[0].rx_packets, 1);
        assert_eq!(status.entries[0].drops["oversized"], 1);
        assert_eq!(status.entries[0].drops["not_ready"], 1);
        assert_eq!(status.entries[0].drops["invalid_datagram"], 2);
        assert!(status.drops["unknown_origin_or_allocation"] >= 2);
        assert!(serde_json::to_value(status).unwrap()["entries"][0]["peer"].is_string());
        client.close(peer);
        assert_eq!(client.status().allocations, 0);
        assert!(client.hub.get().unwrap().routes.lock().unwrap().is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn allocation_error_emits_down() {
        let (addr, _, server) = udp_server([6; 16], true).await;
        let (client, mut events) = client(None);
        client.open(
            [5; 16],
            vec![RelayEndpoint {
                relay_id: [6; 16],
                token: token_b64([5; 16]),
                name: "lab".into(),
                udp: Some(addr),
                tls: None,
                tls_name: None,
            }],
        );
        event(&mut events, |e| {
            matches!(
                e,
                RelayEvent::Down {
                    kind: RelayKind::Udp,
                    ..
                }
            )
        })
        .await;
        assert_eq!(
            client.status().entries[0].last_error.as_deref(),
            Some("alloc_rejected")
        );
        server.abort();
    }

    struct TestCa(std::path::PathBuf);
    impl Drop for TestCa {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn tls_config() -> (Arc<rustls::ServerConfig>, TestCa) {
        let cert = rcgen::generate_simple_self_signed(vec!["relay.test".into()]).unwrap();
        let path = std::env::temp_dir().join(format!(
            "atenea-relay-ca-{}-{}.pem",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, cert.cert.pem()).unwrap();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key.into())
        .unwrap();
        cfg.alpn_protocols = vec![wire::ALPN.to_vec()];
        (Arc::new(cfg), TestCa(path))
    }
    async fn tls_server(
        cfg: Arc<rustls::ServerConfig>,
        reject: bool,
    ) -> (String, watch::Receiver<usize>, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (allocations, observed) = watch::channel(0);
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let Ok(stream) = tokio_rustls::TlsAcceptor::from(cfg).accept(tcp).await else {
                return;
            };
            assert_eq!(
                stream.get_ref().1.protocol_version(),
                Some(rustls::ProtocolVersion::TLSv1_3)
            );
            assert_eq!(stream.get_ref().1.alpn_protocol(), Some(wire::ALPN));
            let mut decoder = wire::FrameDecoder::new();
            let mut stream = stream;
            let mut buf = [0; 2048];
            let mut first = true;
            loop {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                decoder.push(&buf[..n]);
                while let Some(body) = decoder.next_frame().unwrap() {
                    let message = wire::parse(&body).unwrap();
                    if first {
                        assert!(matches!(message, wire::Message::Alloc { .. }));
                        first = false;
                    }
                    let response = match message {
                        wire::Message::Alloc {
                            relay_id,
                            cookie,
                            ts,
                            token,
                            sig,
                        } => {
                            assert_eq!(cookie, [0; 16]);
                            assert!(wire::verify_alloc(
                                &identity().verifying_key().to_bytes(),
                                &relay_id,
                                &cookie,
                                ts,
                                token,
                                &sig
                            ));
                            // Se notifica después de escribir la respuesta completa.
                            if reject {
                                wire::encode_alloc_err(wire::ErrCode::BadToken)
                            } else {
                                wire::encode_alloc_ok(88, 120, true)
                            }
                        }
                        wire::Message::Ping { alloc_id, nonce } => {
                            wire::encode_ping(alloc_id, nonce, true)
                        }
                        wire::Message::Data { alloc_id, datagram } => {
                            if stream
                                .write_all(
                                    &wire::encode_frame(&wire::encode_data(999, &datagram_bytes()))
                                        .unwrap(),
                                )
                                .await
                                .is_err()
                            {
                                return;
                            }
                            wire::encode_data(alloc_id, datagram)
                        }
                        _ => panic!("unexpected message"),
                    };
                    // Lecturas parciales reales, no solo el decoder aislado.
                    let frame = wire::encode_frame(&response).unwrap();
                    if stream.write_all(&frame[..1]).await.is_err()
                        || stream.write_all(&frame[1..]).await.is_err()
                    {
                        return;
                    }
                    if matches!(message, wire::Message::Alloc { .. }) {
                        allocations.send_modify(|count| *count += 1);
                    }
                }
            }
        });
        (addr, observed, task)
    }

    #[tokio::test]
    async fn udp_timeout_falls_back_to_tls_custom_ca_queue_and_renewal() {
        let (cfg, ca) = tls_config();
        let (addr, mut allocations, server) = tls_server(cfg, false).await;
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (mut client, mut events) = client(Some(ca.0.clone()));
        // Esta prueba mide fallback y renovación, no la pérdida de heartbeats. La fragmentación
        // TLS y la cola no deben competir con los ticks acelerados de las pruebas de timeout.
        client.timing.ping = Timing::default().ping;
        client.open(
            [5; 16],
            vec![RelayEndpoint {
                relay_id: [6; 16],
                token: token_b64([5; 16]),
                name: "lab".into(),
                udp: Some(silent.local_addr().unwrap().to_string()),
                tls: Some(addr),
                tls_name: Some("relay.test".into()),
            }],
        );
        event(&mut events, |e| {
            matches!(
                e,
                RelayEvent::Ready {
                    kind: RelayKind::Tls,
                    ..
                }
            )
        })
        .await;
        client.send([5; 16], [6; 16], RelayKind::Tls, datagram_bytes());
        assert!(
            matches!(event(&mut events, |e| matches!(e, RelayEvent::Rx { .. })).await, RelayEvent::Rx { relay_id, datagram, .. } if relay_id == [6; 16] && datagram == datagram_bytes())
        );
        for _ in 0..QUEUE + 10 {
            client.send([5; 16], [6; 16], RelayKind::Tls, datagram_bytes());
        }
        assert!(client.status().entries[0].drops["queue_full"] >= 10);
        timeout(
            Duration::from_secs(3),
            allocations.wait_for(|count| *count >= 2),
        )
        .await
        .expect("renovación TLS no observada")
        .expect("servidor TLS cerrado antes de renovar");
        assert!(client.status().entries[0].drops["invalid_message_or_allocation"] > 0);
        server.abort();
        event(&mut events, |e| {
            matches!(
                e,
                RelayEvent::Down {
                    kind: RelayKind::Tls,
                    ..
                }
            )
        })
        .await;
        client.close([5; 16]);
    }

    #[tokio::test]
    async fn tls_rejects_wrong_name_and_untrusted_ca_and_alloc_error() {
        for (name, trusted, reject, expected) in [
            ("wrong.test", true, false, "tls_handshake"),
            ("relay.test", false, false, "tls_handshake"),
            ("relay.test", true, true, "alloc_rejected"),
        ] {
            let (cfg, ca) = tls_config();
            let (addr, _, server) = tls_server(cfg, reject).await;
            let (client, mut events) = client(trusted.then(|| ca.0.clone()));
            client.open(
                [5; 16],
                vec![RelayEndpoint {
                    relay_id: [6; 16],
                    token: token_b64([5; 16]),
                    name: "lab".into(),
                    udp: None,
                    tls: Some(addr),
                    tls_name: Some(name.into()),
                }],
            );
            event(&mut events, |e| matches!(e, RelayEvent::Down { .. })).await;
            assert_eq!(
                client.status().entries[0].last_error.as_deref(),
                Some(expected)
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn token_refresh_keeps_allocation_and_expiry_emits_down() {
        let (addr, allocations, server) = udp_server([6; 16], false).await;
        let (client, mut events) = client(None);
        let mut endpoint = RelayEndpoint {
            relay_id: [6; 16],
            token: token_b64([5; 16]),
            name: "lab".into(),
            udp: Some(addr),
            tls: None,
            tls_name: None,
        };
        client.open([5; 16], vec![endpoint.clone()]);
        event(&mut events, |e| matches!(e, RelayEvent::Ready { .. })).await;
        client.open([5; 16], vec![endpoint.clone()]);
        timeout(Duration::from_secs(2), async {
            while allocations.load(Ordering::SeqCst) < 2 {
                event(&mut events, |e| matches!(e, RelayEvent::Rtt { .. })).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(client.status().allocations, 1);
        let expired = wire::RelayToken::issue(
            &SigningKey::from_bytes(&[3; 32]),
            [6; 16],
            [1; 16],
            [2; 16],
            [5; 16],
            identity().verifying_key().to_bytes(),
            now() - 10,
            now() - 1,
            [4; 8],
        )
        .encode()
        .to_vec();
        use base64::Engine as _;
        endpoint.token = base64::engine::general_purpose::STANDARD.encode(expired);
        client.open([5; 16], vec![endpoint]);
        event(&mut events, |e| matches!(e, RelayEvent::Down { .. })).await;
        assert_eq!(
            client.status().entries[0].last_error.as_deref(),
            Some("token_expired")
        );
        server.abort();
    }
}
