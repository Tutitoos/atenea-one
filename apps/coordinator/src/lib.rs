//! Presencia y rendezvous sin estado duradero; las pistas nunca autentican una ruta.
mod auth;
pub mod config;
mod presence;
mod rate;
pub mod stun;
use atenea_identity::{DeviceId, PublicKey};
use axum::{
    Json, Router,
    body::{Bytes, to_bytes},
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use config::Config;
use presence::{Event, Presence};
use rate::Rates;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, watch};

#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str);
impl ApiError {
    fn auth() -> Self {
        Self(StatusCode::UNAUTHORIZED, "invalid_signature")
    }
    fn invalid() -> Self {
        Self(StatusCode::FORBIDDEN, "invalid_config")
    }
    fn rate() -> Self {
        Self(StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    }
    fn unavailable() -> Self {
        Self(StatusCode::NOT_FOUND, "peer_unavailable")
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error":{"code":self.1,"message":self.1}})),
        )
            .into_response()
    }
}
struct Memory {
    presences: HashMap<DeviceId, Presence>,
    nonces: auth::Nonces,
    announce: Rates<DeviceId>,
    pair: Rates<(DeviceId, DeviceId)>,
    device: Rates<DeviceId>,
    seq: u64,
}
pub struct Coordinator {
    config: Config,
    anchors: Vec<PublicKey>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    memory: Mutex<Memory>,
    polls: Semaphore,
}
impl Coordinator {
    pub fn new(config: Config) -> Result<Arc<Self>, &'static str> {
        Self::with_clock(
            config,
            Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
        )
    }
    pub fn with_clock(
        config: Config,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Result<Arc<Self>, &'static str> {
        let anchors = config.validate()?;
        Ok(Arc::new(Self {
            memory: Mutex::new(Memory {
                presences: HashMap::new(),
                nonces: auth::Nonces::new(config.max_nonces),
                announce: Rates::new(config.max_rate_entries),
                pair: Rates::new(config.max_rate_entries),
                device: Rates::new(config.max_rate_entries),
                seq: 0,
            }),
            polls: Semaphore::new(config.max_long_polls),
            config,
            anchors,
            clock,
        }))
    }
    pub fn sweep(&self) {
        presence::sweep(
            &mut self.memory.lock().expect("memoria de presencia").presences,
            (self.clock)(),
        );
    }
    pub async fn run_sweeper(self: Arc<Self>, mut stop: watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! { _=interval.tick()=>self.sweep(), changed=stop.changed()=>{if changed.is_err() || *stop.borrow(){break;}} }
        }
    }
}
pub fn router(state: Arc<Coordinator>) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/announce", post(announce))
        .route("/v1/rendezvous", post(rendezvous))
        .route("/v1/events", get(events))
        .fallback(|| async { ApiError(StatusCode::NOT_FOUND, "not_found") })
        .method_not_allowed_fallback(|| async {
            ApiError(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
        })
        .with_state(state)
}
async fn body(request: Request, max: usize) -> Result<(Method, Uri, HeaderMap, Bytes), ApiError> {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, max)
        .await
        .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"))?;
    Ok((parts.method, parts.uri, parts.headers, bytes))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(bytes).map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_request"))
}
async fn health(State(s): State<Arc<Coordinator>>) -> Json<Value> {
    s.sweep();
    Json(json!({"status":"ok","presence":s.memory.lock().expect("memoria").presences.len()}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Announce {
    config: String,
    candidates: Vec<SocketAddr>,
}
async fn announce(
    State(s): State<Arc<Coordinator>>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let observed = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|v| v.0.ip())
        .ok_or(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "missing_remote_address",
        ))?;
    let (method, uri, headers, bytes) = body(request, s.config.max_body_bytes).await?;
    let now = (s.clock)();
    let req = auth::parse(&headers, now)?;
    let data: Announce = decode(&bytes)?;
    let file = STANDARD
        .decode(data.config)
        .map_err(|_| ApiError::invalid())?;
    let (_, config) =
        atenea_identity::inspect_config(&file, &s.anchors, now).map_err(|_| ApiError::invalid())?;
    if config.device_id != req.device_id {
        return Err(ApiError::auth());
    }
    let candidates = atenea_nat_traversal::sanitize_candidates(data.candidates, 8);
    let mut m = s.memory.lock().expect("memoria");
    m.nonces
        .verify(&req, &config.identity_pubkey, &method, &uri, &bytes, now)?;
    if !m.announce.allow(
        req.device_id,
        now,
        1.0 / s.config.announce_interval_secs as f64,
        1,
    ) {
        return Err(ApiError::rate());
    }
    presence::sweep(&mut m.presences, now);
    if !m.presences.contains_key(&req.device_id) && m.presences.len() >= s.config.max_presences {
        return Err(ApiError::rate());
    }
    let expires = now
        .saturating_add(s.config.presence_ttl_secs)
        .min(config.not_after.saturating_add(30));
    let notify = m
        .presences
        .get(&req.device_id)
        .map(|p| p.notify.clone())
        .unwrap_or_default();
    notify.notify_waiters();
    m.presences.insert(
        req.device_id,
        Presence {
            config,
            candidates,
            expires,
            events: VecDeque::new(),
            notify,
        },
    );
    Ok(Json(
        json!({"ttl_secs":expires.saturating_sub(now),"observed_ip":observed}),
    ))
}
fn authenticate(
    m: &mut Memory,
    headers: &HeaderMap,
    method: &Method,
    uri: &Uri,
    bytes: &[u8],
    now: u64,
) -> Result<DeviceId, ApiError> {
    let req = auth::parse(headers, now)?;
    presence::sweep(&mut m.presences, now);
    let key = m
        .presences
        .get(&req.device_id)
        .ok_or(ApiError(StatusCode::CONFLICT, "not_announced"))?
        .config
        .identity_pubkey;
    m.nonces.verify(&req, &key, method, uri, bytes, now)?;
    Ok(req.device_id)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rendezvous {
    peer: String,
}
async fn rendezvous(
    State(s): State<Arc<Coordinator>>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let (method, uri, headers, bytes) = body(request, s.config.max_body_bytes).await?;
    let now = (s.clock)();
    let data: Rendezvous = decode(&bytes)?;
    let peer: DeviceId = hex::decode(data.peer)
        .ok()
        .and_then(|p| p.try_into().ok())
        .ok_or_else(ApiError::unavailable)?;
    let mut m = s.memory.lock().expect("memoria");
    let id = authenticate(&mut m, &headers, &method, &uri, &bytes, now)?;
    if !m.device.allow(
        id,
        now,
        f64::from(s.config.rendezvous_per_minute) / 60.0,
        s.config.rendezvous_per_minute,
    ) || !m.pair.allow(
        (id, peer),
        now,
        1.0 / s.config.rendezvous_pair_interval_secs as f64,
        1,
    ) {
        return Err(ApiError::rate());
    }
    let own = m.presences.get(&id).expect("autenticado");
    let target = m
        .presences
        .get(&peer)
        .filter(|target| {
            target.config.network_id == own.config.network_id
                && own.config.peers.iter().any(|p| p.device_id == peer)
                && target.config.peers.iter().any(|p| p.device_id == id)
        })
        .ok_or_else(ApiError::unavailable)?;
    let candidates = target.candidates.clone();
    let own_candidates = own.candidates.clone();
    m.seq = m.seq.wrapping_add(1);
    let event = Event {
        kind: "connect",
        peer: hex::encode(id),
        candidates: own_candidates,
        seq: m.seq,
    };
    m.presences
        .get_mut(&peer)
        .expect("par autorizado")
        .enqueue(event, now);
    Ok(Json(
        json!({"peer":hex::encode(peer),"candidates":candidates}),
    ))
}
async fn events(
    State(s): State<Arc<Coordinator>>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let (method, uri, headers, bytes) = body(request, s.config.max_body_bytes).await?;
    let wait = match uri.query() {
        None => 25,
        Some(q) => q
            .strip_prefix("wait=")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n <= 30)
            .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_request"))?,
    };
    let _permit = s.polls.try_acquire().map_err(|_| ApiError::rate())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
    let id = {
        let mut m = s.memory.lock().expect("memoria");
        authenticate(&mut m, &headers, &method, &uri, &bytes, (s.clock)())?
    };
    loop {
        let notify = {
            let m = s.memory.lock().expect("memoria");
            m.presences
                .get(&id)
                .map(|p| p.notify.clone())
                .ok_or(ApiError(StatusCode::CONFLICT, "not_announced"))?
        };
        // Registrar antes de leer la cola evita perder una notificación entre lectura y espera.
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let (queued, expires) = {
            let mut m = s.memory.lock().expect("memoria");
            presence::sweep(&mut m.presences, (s.clock)());
            let p = m
                .presences
                .get_mut(&id)
                .ok_or(ApiError(StatusCode::CONFLICT, "not_announced"))?;
            (p.drain((s.clock)()), p.expires)
        };
        if !queued.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(Json(json!({"events":queued})));
        }
        let remaining = Duration::from_secs(expires.saturating_sub((s.clock)()));
        let until = deadline.min(tokio::time::Instant::now() + remaining);
        tokio::select! { _=notified=>{}, _=tokio::time::sleep_until(until)=>{} }
    }
}
