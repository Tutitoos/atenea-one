// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Informes declarados, muestras reales y notificaciones efímeras del panel.
use crate::{AppState, api::ApiError};
use axum::{
    Json,
    body::Bytes,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub struct Live {
    tx: broadcast::Sender<(Uuid, Value)>,
    /// `seq` por red, no global: un miembro no debe ver la actividad de otras redes.
    /// shortcut: una entrada por red con cambios y sin purga; purgarla al borrar la red.
    seq: Mutex<HashMap<Uuid, u64>>,
    connections: Mutex<HashMap<Uuid, usize>>,
}
impl Default for Live {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(256).0,
            seq: Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
        }
    }
}
impl Live {
    pub fn connection_count(&self) -> usize {
        self.connections.lock().expect("conexiones").values().sum()
    }
    pub fn changed(&self, net: Uuid, scope: &str) {
        let Ok(mut seqs) = self.seq.lock() else {
            return;
        };
        let seq = seqs.entry(net).or_default();
        *seq += 1;
        // Enviar con el cerrojo tomado mantiene el orden de `seq` dentro de la red.
        let _ = self
            .tx
            .send((net, json!({"type":"changed","scope":scope,"seq":*seq})));
    }
    fn current_seq(&self, net: Uuid) -> u64 {
        self.seq
            .lock()
            .map_or(0, |seqs| seqs.get(&net).copied().unwrap_or(0))
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathKind {
    Direct,
    RelayUdp,
    RelayTls,
    Probing,
    None,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NatMapping {
    Independent,
    Dependent,
    Unknown,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePath {
    peer: Uuid,
    path: PathKind,
    last_rx_secs: Option<u64>,
    rtt_ms: Option<u64>,
    relay: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DnsState {
    Applied,
    Unsupported,
    None,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteState {
    Installed,
    Conflict,
    Pending,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RouteReport {
    prefix: String,
    state: RouteState,
    reason: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    nat_mapping: Option<NatMapping>,
    paths: Vec<DevicePath>,
    forwarding_ok: Option<bool>,
    dns: Option<DnsState>,
    routes: Option<Vec<RouteReport>>,
    hostname: Option<String>,
    services: Option<Vec<Service>>,
}
/// Máximo de servicios publicados por dispositivo (ADR-024).
const MAX_SERVICES: usize = 64;
const MAX_LABEL_CHARS: usize = 32;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ServiceProto {
    Tcp,
}
#[derive(Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
enum ServiceScheme {
    #[default]
    Http,
    Https,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Service {
    port: u16,
    proto: ServiceProto,
    #[serde(default)]
    scheme: ServiceScheme,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}
/// Valida la lista y la devuelve normalizada (esquema por defecto aplicado, etiqueta sin espacios sobrantes).
fn validate_services(mut list: Vec<Service>) -> Result<Vec<Service>, ApiError> {
    let bad = || ApiError::bad("invalid_request");
    if list.len() > MAX_SERVICES {
        return Err(bad());
    }
    let mut ports = std::collections::HashSet::new();
    for s in &mut list {
        if s.port == 0 || !ports.insert(s.port) {
            return Err(bad());
        }
        if let Some(label) = &s.label {
            let label = crate::devices::name(label).map_err(|_| bad())?;
            if label.chars().count() > MAX_LABEL_CHARS {
                return Err(bad());
            }
            s.label = Some(label.to_owned());
        }
    }
    Ok(list)
}
pub async fn status(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if body.len() > 65536 {
        return Err(ApiError::bad("invalid_report"));
    }
    let invalid = || ApiError(StatusCode::UNAUTHORIZED, "invalid_signature");
    let signed = atenea_identity::api_auth::parse_authorization(
        h.get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    let now = crate::api::now() as u64;
    if now.abs_diff(signed.ts) > 60 {
        return Err(invalid());
    }
    let dev = Uuid::from_bytes(signed.device_id);
    let row = sqlx::query("SELECT network_id,status,identity_pubkey FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(invalid)?;
    let key: [u8; 32] = row
        .get::<Vec<u8>, _>("identity_pubkey")
        .try_into()
        .map_err(|_| invalid())?;
    atenea_identity::api_auth::verify_request(
        &signed,
        &key,
        "POST",
        uri.path_and_query().map_or(uri.path(), |p| p.as_str()),
        &body,
    )
    .map_err(|_| invalid())?;
    crate::take(&s.device_rate, dev, s.config.device_per_minute)?;
    // Un pendiente o revocado se rechaza sin ocupar la caché de nonces compartida.
    if row.get::<String, _>("status") != "approved" {
        return Err(ApiError::forbidden("device_not_approved"));
    }
    crate::claim_nonce(&s.nonces, row.get("network_id"), dev, signed.nonce, now)?;
    let report: Report =
        serde_json::from_slice(&body).map_err(|_| ApiError::bad("invalid_report"))?;
    let hostname = report
        .hostname
        .as_deref()
        .map(crate::devices::name)
        .transpose()
        .map_err(|_| ApiError::bad("invalid_report"))?;
    let services = report.services.map(validate_services).transpose()?;
    if report.routes.as_ref().is_some_and(|routes| {
        routes.len() > 256
            || routes.iter().any(|r| {
                crate::routes::prefix(&r.prefix).is_err()
                    || r.reason
                        .as_ref()
                        .is_some_and(|s| s.len() > 255 || s.chars().any(char::is_control))
            })
    }) {
        return Err(ApiError::bad("invalid_report"));
    }
    if report.paths.len() > 256
        || report.paths.iter().any(|p| {
            p.relay
                .as_ref()
                .is_some_and(|r| r.len() > 255 || r.chars().any(char::is_control))
        })
    {
        return Err(ApiError::bad("invalid_report"));
    }
    let mut tx = s.pool.begin().await?;
    // NO KEY UPDATE y no FOR UPDATE: la emisión bloquea primero la red y después referencia este dispositivo
    // (clave ajena); con FOR UPDATE aquí y la comprobación de la red más abajo, las dos se interbloqueaban.
    let status: String =
        sqlx::query_scalar("SELECT status FROM devices WHERE id=$1 FOR NO KEY UPDATE")
            .bind(dev)
            .fetch_one(&mut *tx)
            .await?;
    if status != "approved" {
        return Err(ApiError::forbidden("device_not_approved"));
    }
    let net: Uuid = row.get("network_id");
    let peers: Vec<Uuid> = report.paths.iter().map(|p| p.peer).collect();
    let valid:bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM unnest($1::uuid[]) p WHERE NOT EXISTS(SELECT 1 FROM devices d WHERE d.id=p AND d.network_id=$2))").bind(peers).bind(net).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(ApiError::bad("invalid_peer"));
    }
    let recent:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM device_reports WHERE device_id=$1 AND reported_at>now()-interval '5 seconds')").bind(dev).fetch_one(&mut *tx).await?;
    if recent {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    let mapping = report.nat_mapping.map(|m| match m {
        NatMapping::Independent => "independent",
        NatMapping::Dependent => "dependent",
        NatMapping::Unknown => "unknown",
    });
    let dns = report.dns.map(|d| match d {
        DnsState::Applied => "applied",
        DnsState::Unsupported => "unsupported",
        DnsState::None => "none",
    });
    sqlx::query("INSERT INTO device_reports(device_id,reported_at,nat_mapping,paths,forwarding_ok,dns,routes) VALUES($1,now(),$2,$3,$4,$5,$6) ON CONFLICT(device_id) DO UPDATE SET reported_at=excluded.reported_at,nat_mapping=excluded.nat_mapping,paths=excluded.paths,forwarding_ok=excluded.forwarding_ok,dns=excluded.dns,routes=excluded.routes").bind(dev).bind(mapping).bind(json!(report.paths)).bind(report.forwarding_ok).bind(dns).bind(report.routes.map(|r|json!(r))).execute(&mut *tx).await?;
    if let Some(hostname) = hostname {
        sqlx::query("UPDATE devices SET hostname=$2,name=CASE WHEN name_source='device' THEN $2 ELSE name END WHERE id=$1")
            .bind(dev)
            .bind(hostname)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(services) = services {
        sqlx::query("UPDATE devices SET services=$2,services_at=now() WHERE id=$1")
            .bind(dev)
            .bind(json!(services))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    s.panel.changed(net, "devices");
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct MetricsQuery {
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: Option<i64>,
}
pub async fn metrics(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Query(q): Query<MetricsQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::networks::access(&s, &h, net, false).await?;
    let limit = q.limit.unwrap_or(360);
    if !(1..=1000).contains(&limit) {
        return Err(ApiError::bad("invalid_limit"));
    }
    let rows=sqlx::query("SELECT at,total,direct,relay,probing,unknown,offline FROM (SELECT * FROM network_samples WHERE network_id=$1 AND ($2::timestamptz IS NULL OR at >= $2) ORDER BY at DESC LIMIT $3) samples ORDER BY at").bind(net).bind(q.since).bind(limit).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"at":r.get::<chrono::DateTime<chrono::Utc>,_>("at"),"total":r.get::<i64,_>("total"),"direct":r.get::<i64,_>("direct"),"relay":r.get::<i64,_>("relay"),"probing":r.get::<i64,_>("probing"),"unknown":r.get::<i64,_>("unknown"),"offline":r.get::<i64,_>("offline")})).collect::<Vec<_>>())))
}
/// Reloj explícito para probar muestreo y retención sin esperar un minuto.
pub async fn sample(s: &AppState, at: chrono::DateTime<chrono::Utc>) -> Result<(), ApiError> {
    let mut tx = s.pool.begin().await?;
    let nets: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM networks")
        .fetch_all(&mut *tx)
        .await?;
    for net in &nets {
        sqlx::query("WITH classified AS (SELECT CASE WHEN r.device_id IS NULL THEN 'unknown' WHEN r.reported_at <= $2::timestamptz-interval '10 minutes' THEN 'offline' ELSE COALESCE((SELECT CASE WHEN p->>'path' IN ('relay-udp','relay-tls') THEN 'relay' WHEN p->>'path'='direct' THEN 'direct' WHEN p->>'path'='probing' THEN 'probing' ELSE 'unknown' END AS kind FROM jsonb_array_elements(r.paths) p GROUP BY kind ORDER BY count(*) DESC,kind LIMIT 1),'unknown') END AS kind FROM devices d LEFT JOIN device_reports r ON r.device_id=d.id WHERE d.network_id=$1 AND d.status='approved') INSERT INTO network_samples(network_id,at,total,direct,relay,probing,unknown,offline) SELECT $1,$2,count(*),count(*) FILTER(WHERE kind='direct'),count(*) FILTER(WHERE kind='relay'),count(*) FILTER(WHERE kind='probing'),count(*) FILTER(WHERE kind='unknown'),count(*) FILTER(WHERE kind='offline') FROM classified ON CONFLICT(network_id,at) DO NOTHING").bind(net).bind(at).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM network_samples WHERE at<$1::timestamptz-interval '7 days'")
        .bind(at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    for net in nets {
        s.panel.changed(net, "metrics");
    }
    Ok(())
}
/// Retención de `configuration_versions`: 7 días tras caducar el lease, salvo la última versión de
/// cada dispositivo (de ella deriva `issue` la siguiente `config_version`). Por lotes: el primer
/// barrido tras desplegar puede ser de millones de filas y no debe superar `statement_timeout`.
pub async fn purge_versions(
    pool: &sqlx::PgPool,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM configuration_versions WHERE id IN (SELECT c.id FROM configuration_versions c WHERE c.not_after<$1::timestamptz-interval '7 days' AND EXISTS(SELECT 1 FROM configuration_versions n WHERE n.device_id=c.device_id AND n.version>c.version) LIMIT 20000)").bind(at).execute(pool).await?;
    Ok(())
}
pub async fn sampler(s: AppState, mut stop: tokio::sync::watch::Receiver<bool>) {
    let mut timer = tokio::time::interval(Duration::from_secs(60));
    let mut previous = chrono::Utc::now();
    loop {
        tokio::select! {
            _=stop.changed()=>break,
            _=timer.tick()=>{
                let current = chrono::Utc::now();
                if sample(&s,current).await.is_err(){tracing::warn!("No se pudo muestrear la red");}
                if crate::auth::purge(&s.pool).await.is_err(){tracing::warn!("No se pudo purgar sesiones caducadas");}
                if purge_versions(&s.pool,current).await.is_err(){tracing::warn!("No se pudo purgar versiones de configuración");}
                // Solo las filas vigentes en `previous` (índice `configuration_versions_not_after`): no lee la tabla entera.
                let expired = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM (SELECT max(not_after) AS expires FROM configuration_versions WHERE not_after > $1 GROUP BY device_id) leases WHERE expires <= $2")
                    .bind(previous).bind(current).fetch_one(&s.pool).await;
                match expired {
                    Ok(count) => {
                        atenea_observability::event(atenea_observability::Event::LeaseExpired, count as u64);
                        previous = current;
                    }
                    Err(_) => { atenea_observability::event(atenea_observability::Event::DatabaseError, 1); tracing::warn!("No se pudieron contar leases expirados"); }
                }
            }
        }
    }
}
struct Connection {
    live: Arc<Live>,
    user: Uuid,
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Ok(mut c) = self.live.connections.lock()
            && let Some(n) = c.get_mut(&self.user)
        {
            *n -= 1;
            if *n == 0 {
                c.remove(&self.user);
            }
        }
    }
}
pub async fn events(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let (user, _) = crate::networks::access(&s, &h, net, false).await?;
    crate::auth::browser_only(&h)?;
    let origin = reqwest::Url::parse(&s.config.public_url)
        .map_err(|_| ApiError::internal())?
        .origin()
        .ascii_serialization();
    if h.get("origin").and_then(|v| v.to_str().ok()) != Some(origin.as_str()) {
        return Err(ApiError::forbidden("invalid_origin"));
    }
    {
        let mut c = s
            .panel
            .connections
            .lock()
            .map_err(|_| ApiError::internal())?;
        if c.values().sum::<usize>() >= 64 || c.get(&user).copied().unwrap_or(0) >= 8 {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "connection_limit"));
        }
        *c.entry(user).or_default() += 1;
    }
    let guard = Connection {
        live: s.panel.clone(),
        user,
    };
    let rx = s.panel.tx.subscribe();
    Ok(ws
        .max_message_size(1024)
        .max_frame_size(1024)
        .on_upgrade(move |socket| stream(socket, s, net, h, rx, guard)))
}
async fn stream(
    mut socket: WebSocket,
    s: AppState,
    net: Uuid,
    h: HeaderMap,
    mut rx: broadcast::Receiver<(Uuid, Value)>,
    _guard: Connection,
) {
    let initial_seq = s.panel.current_seq(net);
    let hello = json!({"type":"hello","seq":initial_seq});
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(25),
        Duration::from_secs(25),
    );
    loop {
        tokio::select! {
            message=socket.recv()=>{if !matches!(message,Some(Ok(Message::Pong(_)|Message::Ping(_)|Message::Text(_)|Message::Binary(_)))){break;}}
            _=ping.tick()=>{if crate::networks::access(&s,&h,net,false).await.is_err() || socket.send(Message::Ping(Bytes::new())).await.is_err(){break;}}
            event=rx.recv()=>{match event {Ok((n,v)) if n==net && v["seq"].as_u64().is_some_and(|seq|seq>initial_seq)=>{if crate::networks::access(&s,&h,net,false).await.is_err() || socket.send(Message::Text(v.to_string().into())).await.is_err(){break;}},Err(broadcast::error::RecvError::Lagged(_))=>{break;},Err(broadcast::error::RecvError::Closed)=>break,_=>{}}}
        }
    }
}
pub async fn changed(
    State(s): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let write = !matches!(
        *req.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    );
    let path = req.uri().path().to_owned();
    let response = next.run(req).await;
    if write
        && response.status().is_success()
        && let Some(rest) = path.strip_prefix("/api/v1/networks/")
        && let Some(net) = rest.split('/').next().and_then(|n| n.parse().ok())
    {
        let scope = if rest.contains("/devices/") {
            "devices"
        } else if rest.contains("/policies") {
            "policies"
        } else {
            "network"
        };
        s.panel.changed(net, scope);
        s.panel.changed(net, "audit");
    }
    response
}
pub async fn static_content(State(s): State<AppState>, uri: Uri) -> Response {
    let path = uri.path();
    if ["/api", "/auth", "/healthz", "/metrics"]
        .iter()
        .any(|p| path.starts_with(p))
    {
        return ApiError::not_found().into_response();
    }
    let Some(root) = s.config.web_dir else {
        return ApiError::not_found().into_response();
    };
    let Ok(root) = tokio::fs::canonicalize(root).await else {
        return ApiError::not_found().into_response();
    };
    let requested = root.join(path.trim_start_matches('/'));
    let file = match tokio::fs::canonicalize(requested).await {
        Ok(p) if p.starts_with(&root) && p.is_file() => p,
        _ => root.join("index.html"),
    };
    let Ok(file) = tokio::fs::canonicalize(file).await else {
        return ApiError::not_found().into_response();
    };
    if !file.starts_with(&root) {
        return ApiError::not_found().into_response();
    }
    let Ok(bytes) = tokio::fs::read(&file).await else {
        return ApiError::not_found().into_response();
    };
    let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
    let content = match ext {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let hashed = file.file_stem().and_then(|s| s.to_str()).is_some_and(|n| {
        n.rsplit_once('-').is_some_and(|(_, hash)| {
            hash.len() >= 8 && hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    });
    ([("content-type",content),("content-security-policy","default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"),("x-content-type-options","nosniff"),("referrer-policy","same-origin"),("cache-control",if ext=="html"||!hashed{"no-cache"}else{"public, max-age=31536000, immutable"})],bytes).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_cuenta_por_red_y_no_revela_otras_redes() {
        let live = Live::default();
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut rx = live.tx.subscribe();
        live.changed(a, "devices");
        live.changed(b, "devices");
        live.changed(b, "audit");
        live.changed(a, "audit");
        let seqs: Vec<(Uuid, u64)> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|(net, event)| (net, event["seq"].as_u64().unwrap()))
            .collect();
        assert_eq!(seqs, vec![(a, 1), (b, 1), (b, 2), (a, 2)]);
        assert_eq!((live.current_seq(a), live.current_seq(b)), (2, 2));
        assert_eq!(live.current_seq(Uuid::from_u128(3)), 0);
    }
}
