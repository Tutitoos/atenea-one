// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
pub mod api;
pub mod audit;
pub mod auth;
pub mod config;
pub mod connectivity;
pub mod db;
pub mod devices;
pub mod dns;
pub mod internal;
pub mod issue;
pub mod networks;
mod openapi;
pub mod orgs;
pub mod panel;
pub mod policies;
pub mod routes;
pub mod service_tokens;
use axum::http::{HeaderMap, StatusCode};
use std::{
    collections::HashMap,
    hash::Hash,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Instant,
};
use uuid::Uuid;
/// Nonces firmados vigentes, agrupados por red: una red no agota la caché de las demás.
#[derive(Default)]
pub struct Nonces {
    by_network: HashMap<Uuid, HashMap<(Uuid, [u8; 16]), u64>>,
    total: usize,
}
pub type NonceCache = Arc<Mutex<Nonces>>;
pub type Rates<K> = Arc<Mutex<HashMap<K, (f64, Instant)>>>;
/// Tope duro de claves en cada caché en memoria.
pub const MAX_ENTRIES: usize = 65_536;
/// Tope de nonces vigentes de una sola red.
pub const MAX_NETWORK_NONCES: usize = MAX_ENTRIES / 4;
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: config::Config,
    pub issuer: atenea_identity::SigningKey,
    pub nonces: NonceCache,
    pub rate: Rates<IpAddr>,
    pub login_rate: Rates<IpAddr>,
    pub device_rate: Rates<Uuid>,
    pub session_rate: Rates<([u8; 32], &'static str)>,
    pub panel: Arc<panel::Live>,
    pub metrics: atenea_observability::Metrics,
    /// Conectividad que anuncia el código de unión (ADR-018); `None` = como la v0.1.8.
    pub connectivity: Option<Arc<connectivity::Connectivity>>,
}
impl AppState {
    pub fn new(
        pool: sqlx::PgPool,
        config: config::Config,
        issuer: atenea_identity::SigningKey,
    ) -> Self {
        Self {
            pool,
            config,
            issuer,
            nonces: Default::default(),
            rate: Default::default(),
            login_rate: Default::default(),
            device_rate: Default::default(),
            session_rate: Default::default(),
            panel: Arc::new(panel::Live::default()),
            metrics: Default::default(),
            connectivity: None,
        }
    }

    /// Activa la conectividad del código de unión. Lo llama `main` tras validar la configuración.
    pub fn with_connectivity(mut self, connectivity: Option<connectivity::Connectivity>) -> Self {
        self.connectivity = connectivity.map(Arc::new);
        self
    }
}
pub use api::{openapi, router};

/// Cubo de tokens por clave (ráfaga = `per_minute`). Al llenarse expulsa los cubos ya repuestos
/// (equivalentes a uno nuevo) y rechaza solo si sigue lleno.
pub fn take<K: Hash + Eq>(
    map: &Mutex<HashMap<K, (f64, Instant)>>,
    key: K,
    per_minute: u32,
) -> Result<(), api::ApiError> {
    let limit = f64::from(per_minute);
    let limited = || api::ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    let now = Instant::now();
    let mut m = map.lock().map_err(|_| api::ApiError::internal())?;
    if m.len() >= MAX_ENTRIES && !m.contains_key(&key) {
        m.retain(|_, (t, at)| *t + now.duration_since(*at).as_secs_f64() * limit / 60. < limit);
        if m.len() >= MAX_ENTRIES {
            return Err(limited());
        }
    }
    let e = m.entry(key).or_insert((limit, now));
    e.0 = (e.0 + now.duration_since(e.1).as_secs_f64() * limit / 60.).min(limit);
    e.1 = now;
    if e.0 < 1. {
        return Err(limited());
    }
    e.0 -= 1.;
    Ok(())
}

/// Registra un nonce firmado. Repetido → 401; cuota de la red o caché llenas de nonces vigentes → 429.
/// Los vigentes no se expulsan: conservan la protección antirrepetición.
pub fn claim_nonce(
    cache: &NonceCache,
    net: Uuid,
    dev: Uuid,
    nonce: [u8; 16],
    now: u64,
) -> Result<(), api::ApiError> {
    let live = |ts: &u64| now.saturating_sub(*ts) <= 120;
    let limited = || api::ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    let mut guard = cache.lock().map_err(|_| api::ApiError::internal())?;
    let Nonces { by_network, total } = &mut *guard;
    if by_network
        .get(&net)
        .and_then(|own| own.get(&(dev, nonce)))
        .is_some_and(live)
    {
        return Err(api::ApiError(StatusCode::UNAUTHORIZED, "invalid_signature"));
    }
    if *total >= MAX_ENTRIES {
        by_network.retain(|_, own| {
            own.retain(|_, ts| live(ts));
            !own.is_empty()
        });
        *total = by_network.values().map(HashMap::len).sum();
        if *total >= MAX_ENTRIES {
            return Err(limited());
        }
    }
    let own = by_network.entry(net).or_default();
    if own.len() >= MAX_NETWORK_NONCES {
        let before = own.len();
        own.retain(|_, ts| live(ts));
        *total -= before - own.len();
        if own.len() >= MAX_NETWORK_NONCES {
            return Err(limited());
        }
    }
    if own.insert((dev, nonce), now).is_none() {
        *total += 1;
    }
    Ok(())
}

/// IP de cliente: `X-Real-IP` solo si el peer TCP pertenece a `trusted_proxies`; si no, el peer.
pub fn client_ip(config: &config::Config, peer: Option<IpAddr>, headers: &HeaderMap) -> IpAddr {
    let trusted = peer.is_some_and(|ip| config.trusted_proxies.iter().any(|n| n.contains(&ip)));
    let peer = peer.unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    if trusted
        && headers.get_all("x-real-ip").iter().count() == 1
        && let Some(ip) = headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    peer
}

/// Clave de cuota por origen: IPv6 se agrupa por /64 (lo que se entrega a un solo cliente); sin
/// agrupar, un cliente tendría 2^64 cubos y la cuota no limitaría nada.
pub fn ip_group(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v) => IpAddr::V6(std::net::Ipv6Addr::from(u128::from(v) & !0u128 << 64)),
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ip_cliente_solo_con_proxy_confiable() {
        let mut cfg = config::Config::default();
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", "203.0.113.9".parse().unwrap());
        let proxy: IpAddr = "172.18.0.5".parse().unwrap();
        assert_eq!(client_ip(&cfg, Some(proxy), &h), proxy);
        cfg.trusted_proxies = vec!["172.18.0.0/16".parse().unwrap()];
        assert_eq!(
            client_ip(&cfg, Some(proxy), &h),
            "203.0.113.9".parse::<IpAddr>().unwrap()
        );
        let other: IpAddr = "198.51.100.1".parse().unwrap();
        assert_eq!(client_ip(&cfg, Some(other), &h), other);
        h.insert("x-real-ip", "no-ip".parse().unwrap());
        assert_eq!(client_ip(&cfg, Some(proxy), &h), proxy);
        h.insert("x-real-ip", "203.0.113.9".parse().unwrap());
        h.append("x-real-ip", "203.0.113.10".parse().unwrap());
        assert_eq!(client_ip(&cfg, Some(proxy), &h), proxy);
        h.remove("x-real-ip");
        h.insert("x-real-ip", "203.0.113.9".parse().unwrap());
        cfg.trusted_proxies = vec!["127.0.0.1/32".parse().unwrap()];
        assert_eq!(
            client_ip(&cfg, None, &h),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
    }
    #[test]
    fn ipv6_comparte_cubo_por_64() {
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(ip_group(a), ip_group(b));
        assert_ne!(ip_group(a), ip_group(c));
        assert_eq!(
            ip_group("::ffff:192.0.2.1".parse().unwrap()),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
        let m: Mutex<HashMap<IpAddr, (f64, Instant)>> = Default::default();
        assert!(take(&m, ip_group(a), 1).is_ok());
        assert!(take(&m, ip_group(b), 1).is_err(), "misma /64, mismo cubo");
        assert!(take(&m, ip_group(c), 1).is_ok());
    }
    #[test]
    fn cubo_acotado_no_bloquea_claves_nuevas_si_hay_repuestos() {
        let m: Mutex<HashMap<u32, (f64, Instant)>> = Default::default();
        // Entradas ya repuestas (tokens = límite): se expulsan al llenarse.
        m.lock()
            .unwrap()
            .extend((0..MAX_ENTRIES as u32).map(|k| (k, (10., Instant::now()))));
        assert!(take(&m, u32::MAX, 10).is_ok());
        assert!(m.lock().unwrap().len() < MAX_ENTRIES);
        // Entradas agotadas: se rechaza la clave nueva sin crecer.
        m.lock().unwrap().clear();
        m.lock()
            .unwrap()
            .extend((0..MAX_ENTRIES as u32).map(|k| (k, (0., Instant::now()))));
        assert!(take(&m, u32::MAX, 10).is_err());
        assert_eq!(m.lock().unwrap().len(), MAX_ENTRIES);
    }
    #[test]
    fn nonces_acotados_por_red_y_repeticion_rechazada() {
        let c = NonceCache::default();
        let (a, b, d) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::new_v4());
        let code = |r: Result<(), api::ApiError>| r.unwrap_err().1;
        assert!(claim_nonce(&c, a, d, [1; 16], 1000).is_ok());
        assert_eq!(
            code(claim_nonce(&c, a, d, [1; 16], 1000)),
            "invalid_signature"
        );
        // Una red que agota su cuota no deja sin sitio a otra.
        for i in 1..MAX_NETWORK_NONCES as u128 {
            assert!(claim_nonce(&c, a, Uuid::from_u128(i), [0; 16], 1000).is_ok());
        }
        assert_eq!(code(claim_nonce(&c, a, d, [2; 16], 1000)), "rate_limited");
        assert!(claim_nonce(&c, b, d, [2; 16], 1000).is_ok());
        // Con la caché entera llena de vigentes se rechaza a todos; al caducar vuelve a haber sitio.
        for net in 3..=5u128 {
            for i in 0..MAX_NETWORK_NONCES as u128 {
                let _ = claim_nonce(&c, Uuid::from_u128(net), Uuid::from_u128(i), [0; 16], 1000);
            }
        }
        assert_eq!(c.lock().unwrap().total, MAX_ENTRIES);
        assert_eq!(code(claim_nonce(&c, b, d, [3; 16], 1000)), "rate_limited");
        assert!(claim_nonce(&c, a, d, [2; 16], 2000).is_ok());
        assert_eq!(c.lock().unwrap().total, 1);
    }
}

#[cfg(test)]
mod lleno {
    use super::*;
    #[test]
    fn clave_existente_y_nonce_repetido_con_tabla_llena() {
        let m: Mutex<HashMap<u32, (f64, Instant)>> = Default::default();
        m.lock()
            .unwrap()
            .extend((0..MAX_ENTRIES as u32).map(|k| (k, (5., Instant::now()))));
        assert!(take(&m, 0, 10).is_ok());
        assert!(take(&m, u32::MAX, 10).is_err());
        // Red y caché llenas de vigentes: la repetición sigue siendo 401 (no 429) y no pisa el nonce.
        let c = NonceCache::default();
        let (net, d) = (Uuid::from_u128(1), Uuid::new_v4());
        {
            let mut n = c.lock().unwrap();
            n.total = MAX_ENTRIES;
            let own = n.by_network.entry(net).or_default();
            own.extend(
                (1..MAX_NETWORK_NONCES as u128).map(|i| ((Uuid::from_u128(i), [0; 16]), 1000)),
            );
            own.insert((d, [9; 16]), 1000);
        }
        assert_eq!(
            claim_nonce(&c, net, d, [9; 16], 1060).unwrap_err().1,
            "invalid_signature"
        );
        assert_eq!(c.lock().unwrap().by_network[&net][&(d, [9; 16])], 1000);
        assert!(claim_nonce(&c, net, d, [9; 16], 1121).is_ok());
    }
}
