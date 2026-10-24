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
}
