// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
pub mod api;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod devices;
pub mod dns;
pub mod issue;
pub mod networks;
mod openapi;
pub mod orgs;
pub mod panel;
pub mod policies;
pub mod routes;
use axum::http::{HeaderMap, StatusCode};
use std::{
    collections::HashMap,
    hash::Hash,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Instant,
};
use uuid::Uuid;
pub type NonceCache = Arc<Mutex<HashMap<(Uuid, [u8; 16]), u64>>>;
pub type Rates<K> = Arc<Mutex<HashMap<K, (f64, Instant)>>>;
/// Tope duro de claves en cada caché en memoria.
pub const MAX_ENTRIES: usize = 65_536;
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: config::Config,
    pub issuer: atenea_identity::SigningKey,
    pub nonces: NonceCache,
    pub rate: Rates<IpAddr>,
    pub login_rate: Rates<IpAddr>,
    pub device_rate: Rates<Uuid>,
    pub panel: Arc<panel::Live>,
    pub metrics: atenea_observability::Metrics,
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
            panel: Arc::new(panel::Live::default()),
            metrics: Default::default(),
        }
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

/// Registra un nonce firmado. Repetido → 401; caché llena de nonces vigentes → 429.
pub fn claim_nonce(
    cache: &NonceCache,
    dev: Uuid,
    nonce: [u8; 16],
    now: u64,
) -> Result<(), api::ApiError> {
    let mut n = cache.lock().map_err(|_| api::ApiError::internal())?;
    if n.get(&(dev, nonce))
        .is_some_and(|ts| now.saturating_sub(*ts) <= 120)
    {
        return Err(api::ApiError(StatusCode::UNAUTHORIZED, "invalid_signature"));
    }
    if n.len() >= MAX_ENTRIES {
        n.retain(|_, ts| now.saturating_sub(*ts) <= 120);
        if n.len() >= MAX_ENTRIES {
            return Err(api::ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
        }
    }
    n.insert((dev, nonce), now);
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
    fn nonces_acotados_y_repeticion_rechazada() {
        let c = NonceCache::default();
        let d = Uuid::new_v4();
        assert!(claim_nonce(&c, d, [1; 16], 1000).is_ok());
        assert_eq!(
            claim_nonce(&c, d, [1; 16], 1000).unwrap_err().1,
            "invalid_signature"
        );
        c.lock()
            .unwrap()
            .extend((0..MAX_ENTRIES as u32).map(|i| ((Uuid::from_u128(i.into()), [0; 16]), 1000)));
        assert_eq!(
            claim_nonce(&c, d, [2; 16], 1000).unwrap_err().1,
            "rate_limited"
        );
        assert!(claim_nonce(&c, d, [2; 16], 2000).is_ok());
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
        let c = NonceCache::default();
        let d = Uuid::new_v4();
        c.lock()
            .unwrap()
            .extend((0..MAX_ENTRIES as u32).map(|i| ((Uuid::from_u128(i.into()), [0; 16]), 1000)));
        c.lock().unwrap().insert((d, [9; 16]), 1000);
        assert_eq!(
            claim_nonce(&c, d, [9; 16], 1060).unwrap_err().1,
            "invalid_signature"
        );
        assert_eq!(c.lock().unwrap()[&(d, [9; 16])], 1000);
        assert!(claim_nonce(&c, d, [9; 16], 1121).is_ok());
    }
}
