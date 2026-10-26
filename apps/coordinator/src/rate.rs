// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use std::{collections::HashMap, hash::Hash};

struct Bucket {
    tokens: f64,
    updated: u64,
}
pub(crate) struct Rates<K> {
    buckets: HashMap<K, Bucket>,
    limit: usize,
}
impl<K: Eq + Hash> Rates<K> {
    pub fn new(limit: usize) -> Self {
        Self {
            buckets: HashMap::new(),
            limit,
        }
    }
    pub fn allow(&mut self, key: K, now: u64, per_second: f64, burst: u32) -> bool {
        // Solo se eliminan cubos completamente repuestos; no se evade el límite rotando claves.
        if !self.buckets.contains_key(&key) && self.buckets.len() >= self.limit {
            self.buckets.retain(|_, b| {
                b.tokens + now.saturating_sub(b.updated) as f64 * per_second < f64::from(burst)
            });
            if self.buckets.len() >= self.limit {
                return false;
            }
        }
        let b = self.buckets.entry(key).or_insert(Bucket {
            tokens: f64::from(burst),
            updated: now,
        });
        b.tokens =
            (b.tokens + now.saturating_sub(b.updated) as f64 * per_second).min(f64::from(burst));
        b.updated = now;
        if b.tokens < 1.0 {
            return false;
        }
        b.tokens -= 1.0;
        true
    }
}

/// Ventana móvil estricta y acotada, independiente de los cubos de rendezvous.
pub(crate) struct Window<K> {
    entries: HashMap<K, Vec<u64>>,
    limit: usize,
    seconds: u64,
    count: usize,
}
impl<K: Eq + Hash> Window<K> {
    pub fn new(limit: usize, seconds: u64, count: usize) -> Self {
        Self {
            entries: HashMap::new(),
            limit,
            seconds,
            count,
        }
    }
    pub fn allow(&mut self, key: K, now: u64) -> bool {
        let seconds = self.seconds;
        if !self.entries.contains_key(&key) && self.entries.len() >= self.limit {
            self.entries.retain(|_, times| {
                times.retain(|t| now.saturating_sub(*t) < seconds);
                !times.is_empty()
            });
            if self.entries.len() >= self.limit {
                return false;
            }
        }
        let times = self.entries.entry(key).or_default();
        times.retain(|t| now.saturating_sub(*t) < seconds);
        if times.len() >= self.count {
            return false;
        }
        times.push(now);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_and_refilled() {
        let mut r = Rates::new(1);
        assert!(r.allow(1, 0, 0.5, 1));
        assert!(!r.allow(1, 1, 0.5, 1));
        assert!(!r.allow(2, 1, 0.5, 1));
        assert!(r.allow(2, 2, 0.5, 1));
    }
    #[test]
    fn relay_window_is_strict_bounded_and_expires() {
        let mut window = Window::new(1, 60, 2);
        assert!(window.allow(1, 100));
        assert!(window.allow(1, 105));
        assert!(!window.allow(1, 159));
        assert!(!window.allow(2, 159));
        assert!(window.allow(1, 160));
        assert!(!window.allow(1, 164));
        assert!(window.allow(1, 165));
        assert!(window.allow(2, 225));
    }
}
