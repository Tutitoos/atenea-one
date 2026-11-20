// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, api::ApiError};
use atenea_identity::{
    AdvertisedRoute, DeviceConfig, DnsConfig, IssuerCert, MAX_PEERS, MAX_RULES, MAX_V3_PEERS,
    MAX_V3_RULES, Peer, Prefix, Rule, Selector, api_auth,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
};
use uuid::Uuid;
/// Un informe de servicios más antiguo que esto (hora de la base de datos) no se entrega a los pares (ADR-024).
const SERVICES_FRESH_SECS: f64 = 120.0;
pub struct Device {
    pub id: Uuid,
    pub noise: [u8; 32],
    pub identity: [u8; 32],
    pub addrs: Vec<IpAddr>,
    pub endpoints: Vec<SocketAddr>,
    pub advertised: Vec<AdvertisedRoute>,
    /// Posición en el orden de aprobación de la red; 0 si nunca se aprobó (ADR-031).
    pub approval_order: i64,
    /// Valor del ajuste de la red en el momento de su aprobación.
    pub auto_on_approval: bool,
    /// Capacidad declarada por el agente: sin ella, sus límites son los de v1/v2.
    pub config_v3: bool,
}
/// Dispositivos aprobados de la red, en orden de id. Tres consultas para toda la red, sin importar cuántos sean.
pub async fn load_devices(c: &mut PgConnection, net: Uuid) -> Result<Vec<Device>, ApiError> {
    let rows = sqlx::query("SELECT id,noise_pubkey,identity_pubkey,endpoints,approval_order,auto_connect_on_approval,config_v3 FROM devices WHERE network_id=$1 AND status='approved' ORDER BY id")
        .bind(net)
        .fetch_all(&mut *c)
        .await?;
    let mut addrs: HashMap<Uuid, Vec<IpAddr>> = HashMap::new();
    for r in sqlx::query(
        "SELECT device_id,host(addr) addr FROM ip_allocations WHERE network_id=$1 ORDER BY family",
    )
    .bind(net)
    .fetch_all(&mut *c)
    .await?
    {
        let addr: IpAddr = r
            .get::<String, _>("addr")
            .parse()
            .map_err(|_| ApiError::internal())?;
        addrs.entry(r.get("device_id")).or_default().push(addr);
    }
    let mut routes: HashMap<Uuid, Vec<AdvertisedRoute>> = HashMap::new();
    for r in sqlx::query("SELECT device_id,prefix::text,snat FROM device_routes WHERE network_id=$1 AND status='approved' ORDER BY device_id,prefix")
        .bind(net)
        .fetch_all(&mut *c)
        .await?
    {
        routes
            .entry(r.get("device_id"))
            .or_default()
            .push(AdvertisedRoute {
                prefix: crate::routes::prefix(&r.get::<String, _>("prefix"))?,
                snat: r.get("snat"),
            });
    }
    let mut devices = Vec::with_capacity(rows.len());
    for r in rows {
        let id: Uuid = r.get("id");
        devices.push(Device {
            id,
            noise: r
                .get::<Vec<u8>, _>("noise_pubkey")
                .try_into()
                .map_err(|_| ApiError::internal())?,
            identity: r
                .get::<Vec<u8>, _>("identity_pubkey")
                .try_into()
                .map_err(|_| ApiError::internal())?,
            addrs: addrs.remove(&id).unwrap_or_default(),
            endpoints: atenea_nat_traversal::sanitize_candidates(
                serde_json::from_value::<Vec<SocketAddr>>(r.get("endpoints"))
                    .map_err(|_| ApiError::internal())?,
                4,
            ),
            advertised: routes.remove(&id).unwrap_or_default(),
            approval_order: r.get("approval_order"),
            auto_on_approval: r.get("auto_connect_on_approval"),
            config_v3: r.get("config_v3"),
        });
    }
    Ok(devices)
}
fn matches(s: &Selector, d: &Device) -> bool {
    match s {
        Selector::Any => true,
        Selector::Device(id) => id == d.id.as_bytes(),
        Selector::Prefix(p) => {
            p.valid()
                && (d.addrs.iter().any(|a| p.contains(*a))
                    || d.advertised.iter().any(|r| r.prefix.contains_prefix(p)))
        }
    }
}
/// Conjunto de dispositivos por índice, en palabras de 64 bits.
type Bits = Vec<u64>;
fn bits_new(len: usize) -> Bits {
    vec![0; len.div_ceil(64)]
}
fn bit_set(b: &mut Bits, i: usize) {
    b[i / 64] |= 1u64 << (i % 64);
}
fn bit_clear(b: &mut Bits, i: usize) {
    b[i / 64] &= !(1u64 << (i % 64));
}
fn bit_has(b: &Bits, i: usize) -> bool {
    (b[i / 64] >> (i % 64)) & 1 == 1
}
fn bit_or(a: &mut Bits, b: &Bits) {
    for (x, y) in a.iter_mut().zip(b) {
        *x |= *y;
    }
}
fn bit_count(b: &Bits) -> usize {
    b.iter().map(|w| w.count_ones() as usize).sum()
}
/// Dispositivos que casan con un selector.
fn members(s: &Selector, devices: &[Device]) -> Bits {
    let mut b = bits_new(devices.len());
    for (i, d) in devices.iter().enumerate() {
        if matches(s, d) {
            bit_set(&mut b, i);
        }
    }
    b
}
/// Clave canónica de una pareja: la misma en ambos sentidos.
fn pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}
/// Regla explícita entre dos dispositivos, sin protocolo ni puertos: una conexión automática en un sentido.
fn device_rule(from: &Device, to: &Device) -> Rule {
    Rule {
        src: Selector::Device(*from.id.as_bytes()),
        dst: Selector::Device(*to.id.as_bytes()),
        proto: 0,
        port_min: 0,
        port_max: 0,
    }
}
/// Red cargada una vez para emitirla o validarla. Las conexiones automáticas no se guardan: se deducen de la
/// aprobación (orden y ajuste) y de las excepciones (ADR-031).
pub struct Network {
    prefixes: Vec<Prefix>,
    devices: Vec<Device>,
    /// Dispositivos que casan con el origen y con el destino de cada política manual viable.
    sides: Vec<(Bits, Bits)>,
    /// Reglas manuales sin repetir, con su inversa, y los dispositivos para los que se emiten.
    concrete: Vec<(Rule, Bits)>,
    /// `dd0[i]` contiene `j` si la regla manual `Device(i) → Device(j)`, sin protocolo ni puertos, ya existe: la
    /// conexión automática entre ambos no la repite.
    dd0: Vec<Bits>,
    /// `excl[i]` contiene `j` si el par está excluido por una excepción.
    excl: Vec<Bits>,
}
/// Pares y reglas de un dispositivo. `links` es la única función que los calcula: la emisión los firma y la
/// validación solo los cuenta.
struct Links {
    /// Todos los pares, manuales y automáticos.
    all: Bits,
    /// Pares alcanzados por política manual: solo a ellos se publican sus subredes.
    manual: Bits,
    /// Reglas sin repetir.
    rules: Vec<Rule>,
}
impl Network {
    pub fn new(
        prefixes: Vec<Prefix>,
        devices: Vec<Device>,
        policies: Vec<Rule>,
        exceptions: HashSet<(Uuid, Uuid)>,
    ) -> Self {
        let n = devices.len();
        let index: HashMap<Uuid, usize> =
            devices.iter().enumerate().map(|(i, d)| (d.id, i)).collect();
        // Una política con un dispositivo ausente o un bloque no aprobado no tiene efecto: se descarta.
        let viable = |s: &Selector| match s {
            Selector::Device(id) => devices.iter().any(|d| d.id.as_bytes() == id),
            Selector::Prefix(p) => {
                prefixes.iter().any(|x| x.contains_prefix(p))
                    || devices
                        .iter()
                        .flat_map(|d| &d.advertised)
                        .any(|r| r.prefix.contains_prefix(p))
            }
            Selector::Any => true,
        };
        let mut sides = Vec::new();
        let mut concrete: Vec<(Rule, Bits)> = Vec::new();
        let mut at: HashMap<Rule, usize> = HashMap::new();
        for rule in policies
            .into_iter()
            .filter(|r| viable(&r.src) && viable(&r.dst))
        {
            let src = members(&rule.src, &devices);
            let dst = members(&rule.dst, &devices);
            let mut relevant = src.clone();
            bit_or(&mut relevant, &dst);
            let inverse = Rule {
                src: rule.dst.clone(),
                dst: rule.src.clone(),
                proto: rule.proto,
                port_min: 0,
                port_max: 0,
            };
            for emitted in [rule, inverse] {
                match at.get(&emitted).copied() {
                    Some(k) => bit_or(&mut concrete[k].1, &relevant),
                    None => {
                        at.insert(emitted.clone(), concrete.len());
                        concrete.push((emitted, relevant.clone()));
                    }
                }
            }
            sides.push((src, dst));
        }
        let mut dd0 = vec![bits_new(n); n];
        for (rule, _) in &concrete {
            if let (Selector::Device(a), Selector::Device(b)) = (&rule.src, &rule.dst)
                && rule.proto == 0
                && rule.port_min == 0
                && rule.port_max == 0
                && let (Some(&i), Some(&j)) = (
                    index.get(&Uuid::from_bytes(*a)),
                    index.get(&Uuid::from_bytes(*b)),
                )
            {
                bit_set(&mut dd0[i], j);
            }
        }
        let mut excl = vec![bits_new(n); n];
        for (x, y) in &exceptions {
            if let (Some(&i), Some(&j)) = (index.get(x), index.get(y)) {
                bit_set(&mut excl[i], j);
                bit_set(&mut excl[j], i);
            }
        }
        Self {
            prefixes,
            devices,
            sides,
            concrete,
            dd0,
            excl,
        }
    }
    /// Carga la red en bloque: seis consultas, sin importar cuántos dispositivos tenga.
    pub async fn load(c: &mut PgConnection, net: Uuid) -> Result<Self, ApiError> {
        let row =
            sqlx::query("SELECT ipv4_cidr::text v4,ipv6_cidr::text v6 FROM networks WHERE id=$1")
                .bind(net)
                .fetch_one(&mut *c)
                .await?;
        let prefixes = crate::networks::prefixes(&row)?;
        let devices = load_devices(c, net).await?;
        let policies = crate::policies::rules(c, net).await?;
        let exceptions: HashSet<(Uuid, Uuid)> = sqlx::query(
            "SELECT device_a,device_b FROM network_auto_exceptions WHERE network_id=$1",
        )
        .bind(net)
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(|r| (r.get("device_a"), r.get("device_b")))
        .collect();
        Ok(Self::new(prefixes, devices, policies, exceptions))
    }
    fn position(&self, dev: Uuid) -> Option<usize> {
        self.devices.iter().position(|d| d.id == dev)
    }
    /// Conexión automática entre dos aprobados: decide el más tardío, con el ajuste activo en su aprobación, salvo
    /// excepción (ADR-031). Dos aprobaciones con el mismo orden no se conectan.
    fn linked(&self, i: usize, j: usize) -> bool {
        let (a, b) = (&self.devices[i], &self.devices[j]);
        let later = match a.approval_order.cmp(&b.approval_order) {
            Ordering::Greater => a,
            Ordering::Less => b,
            Ordering::Equal => return false,
        };
        later.auto_on_approval && !bit_has(&self.excl[i], j)
    }
    /// Pares y reglas de `i`. Coste: O(D + P·D/64 + R) con D aprobados, P políticas viables y R reglas manuales.
    fn links(&self, i: usize) -> Links {
        let mut manual = bits_new(self.devices.len());
        for (src, dst) in &self.sides {
            if bit_has(src, i) {
                bit_or(&mut manual, dst);
            }
            if bit_has(dst, i) {
                bit_or(&mut manual, src);
            }
        }
        bit_clear(&mut manual, i);
        let mut all = manual.clone();
        let mut rules: Vec<Rule> = self
            .concrete
            .iter()
            .filter(|(_, relevant)| bit_has(relevant, i))
            .map(|(rule, _)| rule.clone())
            .collect();
        for j in 0..self.devices.len() {
            if j != i && self.linked(i, j) {
                bit_set(&mut all, j);
                if !bit_has(&self.dd0[i], j) {
                    rules.push(device_rule(&self.devices[i], &self.devices[j]));
                }
                if !bit_has(&self.dd0[j], i) {
                    rules.push(device_rule(&self.devices[j], &self.devices[i]));
                }
            }
        }
        Links { all, manual, rules }
    }
    /// Comprueba los límites de `i`: v1/v2 si su agente no declara `config_v3`, v3 si lo declara. Un error nombra al
    /// dispositivo; si solo supera los límites v1/v2 de un agente anterior, la causa es `config_v3_required`.
    fn check(&self, i: usize, links: &Links) -> Result<(), ApiError> {
        let d = &self.devices[i];
        let peers = bit_count(&links.all);
        let rules = links.rules.len();
        let (max_peers, max_rules) = if d.config_v3 {
            (MAX_V3_PEERS, MAX_V3_RULES)
        } else {
            (MAX_PEERS, MAX_RULES)
        };
        if peers <= max_peers && rules <= max_rules {
            return Ok(());
        }
        let code = if !d.config_v3 && peers <= MAX_V3_PEERS && rules <= MAX_V3_RULES {
            "config_v3_required"
        } else {
            "policy_overflow"
        };
        Err(ApiError::conflict(code).with_device(d.id))
    }
    /// Todos los dispositivos de la red, no solo el que cambia: un vecino anterior puede dejar de caber (ADR-031).
    fn check_all(&self) -> Result<(), ApiError> {
        (0..self.devices.len()).try_for_each(|i| self.check(i, &self.links(i)))
    }
    /// Convierte pares y reglas en el formato firmable. Solo la emisión lo llama.
    fn materialize(&self, links: Links) -> (Vec<Peer>, Vec<Rule>) {
        let peers = (0..self.devices.len())
            .filter(|&j| bit_has(&links.all, j))
            .map(|j| {
                let d = &self.devices[j];
                Peer {
                    device_id: *d.id.as_bytes(),
                    noise_pubkey: d.noise,
                    addrs: d.addrs.clone(),
                    endpoints: d.endpoints.clone(),
                    // La conexión automática no concede subredes: solo las publica una política manual.
                    routes: if bit_has(&links.manual, j) {
                        d.advertised.iter().map(|r| r.prefix.clone()).collect()
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect();
        (peers, links.rules)
    }
    /// Pares y reglas de `i` listos para firmar, si caben en lo que declara su agente.
    pub fn config(&self, i: usize) -> Result<(Vec<Peer>, Vec<Rule>), ApiError> {
        let links = self.links(i);
        self.check(i, &links)?;
        Ok(self.materialize(links))
    }
    /// Dispositivos con conexión automática con `i`, en orden de id.
    fn auto_links(&self, i: usize) -> Vec<Uuid> {
        (0..self.devices.len())
            .filter(|&j| j != i && self.linked(i, j))
            .map(|j| self.devices[j].id)
            .collect()
    }
}
/// Valida la red entera tras un cambio que puede añadir pares o reglas (aprobación, política manual, ruta aprobada).
/// Carga una vez y por dispositivo solo cuenta: no construye ni firma configuraciones.
pub async fn validate_network(tx: &mut PgConnection, net: Uuid) -> Result<(), ApiError> {
    Network::load(tx, net).await?.check_all()
}
/// Fija el orden de aprobación y el ajuste automático de un dispositivo recién aprobado, y valida la red. Toma el
/// bloqueo de la red dentro de `tx`; el estado `approved` ya debe estar escrito en esa transacción. Un error deshace
/// la aprobación al descartarse `tx`.
pub async fn record_approval(tx: &mut PgConnection, net: Uuid, dev: Uuid) -> Result<(), ApiError> {
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE devices SET approval_order=(SELECT COALESCE(max(approval_order),0)+1 FROM devices WHERE network_id=$1), auto_connect_on_approval=(SELECT auto_connect FROM networks WHERE id=$1) WHERE id=$2 AND approval_order=0")
        .bind(net)
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    validate_network(tx, net).await
}
/// Pares automáticos de `dev`, en orden de id. `[]` si aún no está aprobado; 404 si no pertenece a la red.
pub async fn auto_peers(c: &mut PgConnection, net: Uuid, dev: Uuid) -> Result<Vec<Uuid>, ApiError> {
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM devices WHERE id=$1 AND network_id=$2)")
            .bind(dev)
            .bind(net)
            .fetch_one(&mut *c)
            .await?;
    if !known {
        return Err(ApiError::not_found());
    }
    let network = Network::load(c, net).await?;
    Ok(network
        .position(dev)
        .map_or_else(Vec::new, |i| network.auto_links(i)))
}
/// Quita la conexión automática entre `dev` y `peer` con una excepción. Solo cabe si hoy es automática; si no, 404.
/// Bloquea la red dentro de `tx`.
pub async fn block_auto_peer(
    tx: &mut PgConnection,
    net: Uuid,
    dev: Uuid,
    peer: Uuid,
) -> Result<(), ApiError> {
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut *tx)
        .await?;
    let network = Network::load(tx, net).await?;
    let (Some(i), Some(j)) = (network.position(dev), network.position(peer)) else {
        return Err(ApiError::not_found());
    };
    if !network.linked(i, j) {
        return Err(ApiError::not_found());
    }
    let (a, b) = pair(dev, peer);
    sqlx::query(
        "INSERT INTO network_auto_exceptions(network_id,device_a,device_b) VALUES($1,$2,$3)",
    )
    .bind(net)
    .bind(a)
    .bind(b)
    .execute(&mut *tx)
    .await?;
    Ok(())
}
#[derive(Clone, Copy)]
enum Output {
    Config,
    Peers,
    ServerInfo,
}
pub async fn get(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    respond(s, h, uri, body, Output::Config).await
}
pub async fn peers(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    respond(s, h, uri, body, Output::Peers).await
}
pub async fn server_info(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    respond(s, h, uri, body, Output::ServerInfo).await
}
async fn respond(
    s: AppState,
    h: HeaderMap,
    uri: Uri,
    body: axum::body::Bytes,
    output: Output,
) -> Result<Response, ApiError> {
    let invalid = || ApiError::new(StatusCode::UNAUTHORIZED, "invalid_signature");
    let signed = api_auth::parse_authorization(
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
    let row = sqlx::query("SELECT network_id,identity_pubkey,status FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(invalid)?;
    let key: [u8; 32] = row
        .get::<Vec<u8>, _>("identity_pubkey")
        .try_into()
        .map_err(|_| invalid())?;
    api_auth::verify_request(
        &signed,
        &key,
        "GET",
        uri.path_and_query().map_or(uri.path(), |p| p.as_str()),
        &body,
    )
    .map_err(|_| invalid())?;
    crate::take(&s.device_rate, dev, s.config.device_per_minute)?;
    // Un pendiente o revocado se rechaza sin ocupar la caché de nonces compartida.
    if row.get::<String, _>("status") != "approved" {
        return Err(ApiError::forbidden("device_not_approved"));
    }
    let net: Uuid = row.get("network_id");
    crate::claim_nonce(&s.nonces, net, dev, signed.nonce, now)?;
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("SELECT org_id,issuer_cert,ipv4_cidr::text v4,ipv6_cidr::text v6 FROM networks WHERE id=$1 FOR UPDATE").bind(net).fetch_one(&mut *tx).await?;
    let status: String = sqlx::query_scalar("SELECT status FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_one(&mut *tx)
        .await?;
    if status != "approved" {
        return Err(ApiError::forbidden("device_not_approved"));
    }
    if matches!(output, Output::ServerInfo) {
        let expires = n
            .get::<Option<Vec<u8>>, _>("issuer_cert")
            .map(|bytes| IssuerCert::parse(&bytes).map(|cert| cert.not_after))
            .transpose()
            .map_err(|_| ApiError::internal())?;
        let info = serde_json::json!({
            "control_plane": {
                "version": env!("CARGO_PKG_VERSION"),
                "commit": option_env!("ATENEA_BUILD_COMMIT").unwrap_or("unknown"),
            },
            "server_time_unix": crate::api::now(),
            "issuer_cert_not_after_unix": expires,
        });
        tx.commit().await?;
        return Ok(([("cache-control", "no-store")], axum::Json(info)).into_response());
    }
    let network = Network::load(&mut tx, net).await?;
    // Rechazo cerrado: una red heredada fuera de ADR-007 no se firma.
    if !network.prefixes.iter().all(Prefix::valid_route) {
        return Err(ApiError::conflict("invalid_network_prefix"));
    }
    let me = network.position(dev).ok_or_else(ApiError::internal)?;
    let own = &network.devices[me];
    let (peers, rules) = network.config(me)?;
    if matches!(output, Output::Peers) {
        let ids: Vec<Uuid> = peers
            .iter()
            .map(|p| Uuid::from_bytes(p.device_id))
            .collect();
        // `versions` no caduca como `services` (ADR-025, P3): se devuelve la última informada con su fecha, y `null` en
        // `versions_at` significa «nunca informó» (agente anterior o sin informe).
        let rows = sqlx::query("SELECT id,name,role,CASE WHEN services_at>now()-make_interval(secs=>$2) THEN services ELSE '[]'::jsonb END services,versions,versions_at FROM devices WHERE id=ANY($1) ORDER BY id")
            .bind(ids)
            .bind(SERVICES_FRESH_SECS)
            .fetch_all(&mut *tx)
            .await?;
        let names: Vec<_> = rows
            .iter()
            .map(|r| serde_json::json!({"device_id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"role":r.get::<String,_>("role"),"services":r.get::<serde_json::Value,_>("services"),"versions":r.get::<serde_json::Value,_>("versions"),"versions_at":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("versions_at")}))
            .collect();
        tx.commit().await?;
        return Ok(([("cache-control", "no-store")], axum::Json(names)).into_response());
    }
    let unavailable = || ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "issuer_unavailable");
    let cert = IssuerCert::parse(
        &n.get::<Option<Vec<u8>>, _>("issuer_cert")
            .ok_or_else(unavailable)?,
    )
    .map_err(|_| unavailable())?;
    let end = now
        .checked_add(s.config.config_validity_secs)
        .ok_or_else(unavailable)?;
    if cert.not_before > now
        || cert.not_after < end
        || s.config.config_validity_secs == 0
        || s.config.config_validity_secs > 3600
    {
        return Err(unavailable());
    }
    let mut dns = crate::dns::load(&mut tx, net).await?;
    dns.servers.retain(|a| {
        network.prefixes.iter().any(|p| p.contains(*a))
            || own.advertised.iter().any(|r| r.prefix.contains(*a))
            || peers.iter().flat_map(|p| &p.routes).any(|p| p.contains(*a))
    });
    let last: Option<i64> =
        sqlx::query_scalar("SELECT max(version) FROM configuration_versions WHERE device_id=$1")
            .bind(dev)
            .fetch_one(&mut *tx)
            .await?;
    let version = chrono::Utc::now().timestamp_millis().max(
        last.unwrap_or(0)
            .checked_add(1)
            .ok_or_else(ApiError::internal)?,
    );
    let mut cfg = DeviceConfig {
        network_id: *net.as_bytes(),
        device_id: *dev.as_bytes(),
        issuer_serial: cert.issuer_serial,
        config_version: version as u64,
        not_before: now,
        not_after: end,
        noise_pubkey: own.noise,
        identity_pubkey: own.identity,
        mtu: 1400,
        tunnel_addrs: own.addrs.clone(),
        network_prefixes: network.prefixes.clone(),
        peers,
        rules,
        advertised: own.advertised.clone(),
        dns: DnsConfig {
            servers: dns.servers,
            domains: dns.domains,
        },
    };
    cfg.canonicalize();
    // Guardia de emisión: el codificador firma en v3 cuando hay más de 256 pares o reglas; un agente sin
    // `config_v3` no puede recibirla.
    if cfg.wire_version() == 3 && !own.config_v3 {
        return Err(ApiError::conflict("config_v3_required").with_device(dev));
    }
    let file =
        atenea_identity::issue_config(&s.issuer, &cert, &cfg).map_err(|_| ApiError::internal())?;
    let hash = Sha256::digest(&file);
    sqlx::query("INSERT INTO configuration_versions(device_id,version,config_hash,not_before,not_after,issuer_serial) VALUES($1,$2,$3,to_timestamp($4),to_timestamp($5),$6)").bind(dev).bind(version).bind(hash.as_slice()).bind(now as f64).bind(end as f64).bind(cert.issuer_serial as i64).execute(&mut *tx).await?;
    sqlx::query("UPDATE devices SET last_config_at=now() WHERE id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    let org: Uuid = n.get("org_id");
    // Auditoría acotada: una entrada por dispositivo y hora; el resto queda en configuration_versions.
    let audited: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE network_id=$1 AND target=$2 AND action='config.issue' AND at>now()-interval '1 hour')").bind(net).bind(dev.to_string()).fetch_one(&mut *tx).await?;
    if !audited {
        crate::audit::record(
            &mut tx,
            "device",
            dev,
            Some(org),
            Some(net),
            "config.issue",
            dev,
            serde_json::json!({"version":version,"hash":hex::encode(hash)}),
        )
        .await?;
    }
    tx.commit().await?;
    atenea_observability::event(
        if last.is_some() {
            atenea_observability::Event::LeaseRenewed
        } else {
            atenea_observability::Event::LeaseIssued
        },
        1,
    );
    s.panel.changed(net, "devices");
    s.panel.changed(net, "audit");
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("cache-control", "no-store"),
            ("x-config-version", &version.to_string()),
        ],
        file,
    )
        .into_response())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// Dispositivo aprobado de prueba. El id crece con `n`, así que el índice coincide con el orden por id.
    fn dispositivo(n: u32, orden: i64, auto: bool, v3: bool) -> Device {
        Device {
            id: Uuid::from_u128(u128::from(n)),
            noise: [1; 32],
            identity: [2; 32],
            addrs: vec![IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + n))],
            endpoints: Vec::new(),
            advertised: Vec::new(),
            approval_order: orden,
            auto_on_approval: auto,
            config_v3: v3,
        }
    }
    /// `n` aprobados en orden con el ajuste encendido: malla completa.
    fn malla(n: u32, v3: bool) -> Vec<Device> {
        (1..=n)
            .map(|k| dispositivo(k, i64::from(k), true, v3))
            .collect()
    }
    fn red_de(
        devices: Vec<Device>,
        policies: Vec<Rule>,
        exceptions: HashSet<(Uuid, Uuid)>,
    ) -> Network {
        Network::new(Vec::new(), devices, policies, exceptions)
    }

    #[test]
    fn el_mas_tardio_decide_y_el_ajuste_apagado_no_conecta() {
        // A y B se aprobaron con el ajuste apagado; C con él encendido; D después de apagarlo (ejemplo de ADR-031).
        let ds = vec![
            dispositivo(1, 1, false, false),
            dispositivo(2, 2, false, false),
            dispositivo(3, 3, true, false),
            dispositivo(4, 4, false, false),
        ];
        let n = red_de(ds, Vec::new(), HashSet::new());
        assert_eq!(n.auto_links(0), vec![Uuid::from_u128(3)]);
        assert_eq!(n.auto_links(1), vec![Uuid::from_u128(3)]);
        assert_eq!(
            n.auto_links(2),
            vec![Uuid::from_u128(1), Uuid::from_u128(2)]
        );
        assert!(n.auto_links(3).is_empty());
    }

    #[test]
    fn excepcion_quita_la_automatica_y_la_manual_sigue() {
        let ds = vec![
            dispositivo(1, 1, false, false),
            dispositivo(2, 2, false, false),
            dispositivo(3, 3, true, false),
        ];
        let (a, c) = (ds[0].id, ds[2].id);
        let manual = Rule {
            src: Selector::Device(*a.as_bytes()),
            dst: Selector::Device(*c.as_bytes()),
            proto: 6,
            port_min: 443,
            port_max: 443,
        };
        let n = red_de(ds, vec![manual.clone()], HashSet::from([pair(c, a)]));
        assert!(n.auto_links(0).is_empty());
        let (peers, rules) = n.config(0).unwrap();
        assert_eq!(
            peers.iter().map(|p| p.device_id).collect::<Vec<_>>(),
            vec![*c.as_bytes()]
        );
        assert!(rules.contains(&manual));
        assert!(rules.iter().all(|r| r.proto != 0));
    }

    #[test]
    fn regla_manual_identica_a_la_automatica_cuenta_una_vez() {
        let ds = vec![
            dispositivo(1, 1, false, false),
            dispositivo(2, 2, true, false),
        ];
        let (a, c) = (ds[0].id, ds[1].id);
        // C → A sin puertos es exactamente la regla que daría la conexión automática.
        let manual = Rule {
            src: Selector::Device(*c.as_bytes()),
            dst: Selector::Device(*a.as_bytes()),
            proto: 0,
            port_min: 0,
            port_max: 0,
        };
        let n = red_de(ds, vec![manual.clone()], HashSet::new());
        let (peers, rules) = n.config(0).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(rules.len(), 2);
        assert!(rules.contains(&manual));
    }

    #[test]
    fn la_excepcion_es_simetrica() {
        let ds = vec![
            dispositivo(1, 1, false, false),
            dispositivo(2, 2, true, false),
        ];
        let n = red_de(
            ds,
            Vec::new(),
            HashSet::from([pair(Uuid::from_u128(2), Uuid::from_u128(1))]),
        );
        assert!(n.auto_links(0).is_empty());
        assert!(n.auto_links(1).is_empty());
    }

    #[test]
    fn cliente_anterior_con_mas_de_256_reglas_se_rechaza_con_su_identidad() {
        // Malla de 130: cada dispositivo tendría 129 pares y 258 reglas.
        let legacy = red_de(malla(130, false), Vec::new(), HashSet::new());
        let err = legacy.check_all().unwrap_err();
        assert_eq!(err.code, "config_v3_required");
        assert_eq!(err.device, Some(Uuid::from_u128(1)));
        // Con la capacidad declarada, la misma red cabe en v3.
        let v3 = red_de(malla(130, true), Vec::new(), HashSet::new());
        assert!(v3.check_all().is_ok());
        let (peers, rules) = v3.config(0).unwrap();
        assert_eq!((peers.len(), rules.len()), (129, 258));
    }

    #[test]
    fn un_vecino_anterior_rechaza_la_red_aunque_el_nuevo_declare_v3() {
        // El agente 1 no declara v3; los demás sí. Al aprobar al último, el vecino anterior es quien no cabe.
        let mut ds = malla(130, true);
        ds[0].config_v3 = false;
        let n = red_de(ds, Vec::new(), HashSet::new());
        let err = n.check_all().unwrap_err();
        assert_eq!(err.code, "config_v3_required");
        assert_eq!(err.device, Some(Uuid::from_u128(1)));
    }

    #[test]
    fn mas_pares_que_el_maximo_v3_se_rechaza_aunque_se_declare_v3() {
        // 501 aprobados en malla: 500 pares por dispositivo, por encima de los 499 de v3.
        let n = red_de(malla(501, true), Vec::new(), HashSet::new());
        assert_eq!(n.check_all().unwrap_err().code, "policy_overflow");
    }
}
