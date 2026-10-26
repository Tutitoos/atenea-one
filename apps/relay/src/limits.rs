// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr},
};

/// Clave de cuota por origen: IPv6 se agrupa por /64.
pub fn ip_group(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v) => IpAddr::V6(Ipv6Addr::from(u128::from(v) & !0u128 << 64)),
        v4 => v4,
    }
}
#[derive(Clone, Debug)]
pub struct Bucket {
    tokens: f64,
    last: u64,
}
impl Bucket {
    pub fn new(rate: u32, now: u64) -> Self {
        Self {
            tokens: f64::from(rate),
            last: now,
        }
    }
    pub fn take(&mut self, rate: u32, amount: usize, now: u64) -> bool {
        self.tokens = (self.tokens
            + now.saturating_sub(self.last) as f64 / 1000.0 * f64::from(rate))
        .min(f64::from(rate));
        self.last = now;
        if self.tokens < amount as f64 {
            return false;
        }
        self.tokens -= amount as f64;
        true
    }
    /// ¿Ya recuperó todos sus tokens? Entonces olvidarlo equivale a conservarlo.
    pub fn refilled(&self, rate: u32, now: u64) -> bool {
        self.tokens + now.saturating_sub(self.last) as f64 / 1000.0 * f64::from(rate)
            >= f64::from(rate)
    }
}
/// Mínimo intervalo entre barridos O(n) provocados por una tabla llena.
const EVICT_EVERY_MS: u64 = 100;
#[derive(Default)]
pub struct IpRates {
    entries: HashMap<(IpAddr, bool), (Bucket, u32)>,
    last_evict: u64,
}
impl IpRates {
    pub fn take(&mut self, ip: IpAddr, alloc: bool, rate: u32, max: usize, now: u64) -> bool {
        let key = (ip_group(ip), alloc);
        if !self.entries.contains_key(&key) && self.entries.len() >= max {
            // Los cubos repuestos no limitan a nadie: se expulsan para que IPs nuevas progresen.
            if now.saturating_sub(self.last_evict) >= EVICT_EVERY_MS {
                self.last_evict = now;
                self.sweep(now);
            }
            if self.entries.len() >= max {
                return false;
            }
        }
        self.entries
            .entry(key)
            .or_insert_with(|| (Bucket::new(rate, now), rate))
            .0
            .take(rate, 1, now)
    }
    pub fn sweep(&mut self, now: u64) {
        self.entries.retain(|_, (b, r)| !b.refilled(*r, now));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refill_and_bounds() {
        let mut b = Bucket::new(2, 0);
        assert!(b.take(2, 2, 0));
        assert!(!b.take(2, 1, 0));
        assert!(b.take(2, 1, 500));
        let mut r = IpRates::default();
        let ip = "127.0.0.1".parse().unwrap();
        assert!(r.take(ip, false, 1, 1, 0));
        assert!(!r.take(ip, true, 1, 1, 0));
        r.sweep(120_000);
        assert!(r.take(ip, true, 1, 1, 120_000));
    }
    #[test]
    fn ipv6_is_grouped_by_64() {
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(ip_group(a), ip_group(b));
        assert_ne!(ip_group(a), ip_group(c));
        assert_eq!(
            ip_group("::ffff:192.0.2.1".parse().unwrap()),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
        let mut r = IpRates::default();
        assert!(r.take(a, true, 1, 8, 0));
        assert!(!r.take(b, true, 1, 8, 0), "misma /64 comparte cubo");
    }
    #[test]
    fn full_table_evicts_refilled_buckets_under_rotation() {
        let (max, rate) = (64usize, 10u32);
        let mut r = IpRates::default();
        let mut now = 0;
        let mut served = 0;
        // Rotación de /64 distintos a 1000 por segundo: la tabla se llena de sobra.
        for i in 0..5_000u128 {
            now += 1;
            let ip = IpAddr::V6(Ipv6Addr::from((0x2001_0db8u128 << 96) | (i << 64)));
            // Cada origen nuevo debe poder progresar durante la rotación.
            if r.take(ip, false, rate, max, now) {
                served += 1;
            }
        }
        assert!(r.entries.len() <= max);
        // Sin expulsión solo se atenderían `max` (64); con ella, un orden de magnitud más.
        assert!(served > 1_000, "servidas {served}");
        let legit = "203.0.113.9".parse().unwrap();
        now += 2 * EVICT_EVERY_MS;
        assert!(r.take(legit, false, rate, max, now));
    }
}
