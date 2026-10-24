use std::{collections::HashMap, net::IpAddr};

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
}
#[derive(Default)]
pub struct IpRates {
    entries: HashMap<(IpAddr, bool), Bucket>,
}
impl IpRates {
    pub fn take(&mut self, ip: IpAddr, alloc: bool, rate: u32, max: usize, now: u64) -> bool {
        let key = (ip, alloc);
        if !self.entries.contains_key(&key) && self.entries.len() >= max {
            return false;
        }
        self.entries
            .entry(key)
            .or_insert_with(|| Bucket::new(rate, now))
            .take(rate, 1, now)
    }
    pub fn sweep(&mut self, now: u64) {
        self.entries
            .retain(|_, b| now.saturating_sub(b.last) < 120_000);
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
}
