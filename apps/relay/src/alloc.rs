use crate::{
    config::Config,
    limits::{Bucket, ip_group},
};
use atenea_relay_core::{ErrCode, RelayToken};
use rand::{RngCore, rngs::OsRng};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};

pub type Key = ([u8; 16], [u8; 16], [u8; 16]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Udp(SocketAddr),
    Tcp(u64, IpAddr),
}
impl Binding {
    /// Clave de cuota por origen (IPv6 agrupado por /64), no la dirección exacta.
    pub fn ip(self) -> IpAddr {
        ip_group(match self {
            Self::Udp(a) => a.ip(),
            Self::Tcp(_, ip) => ip,
        })
    }
}
pub struct Allocation {
    pub id: u64,
    pub key: Key,
    pub binding: Binding,
    pub expires: u64,
    pub packets: Bucket,
    pub bytes: Bucket,
}
#[derive(Default)]
pub struct Table {
    pub entries: HashMap<u64, Allocation>,
    keys: HashMap<Key, u64>,
    networks: HashMap<[u8; 16], usize>,
    ips: HashMap<IpAddr, usize>,
}
fn decrement<K: Eq + std::hash::Hash + Copy>(map: &mut HashMap<K, usize>, key: K) {
    if let Some(n) = map.get_mut(&key) {
        *n -= 1;
        if *n == 0 {
            map.remove(&key);
        }
    }
}
impl Table {
    pub fn peer(&self, key: Key, now: u64) -> Option<&Allocation> {
        self.keys
            .get(&(key.0, key.2, key.1))
            .and_then(|id| self.entries.get(id))
            .filter(|a| a.expires > now)
    }
    pub fn upsert(
        &mut self,
        t: &RelayToken,
        binding: Binding,
        now: u64,
        c: &Config,
    ) -> Result<u64, ErrCode> {
        let expires = now
            .saturating_add(120_000)
            .min(t.not_after.saturating_add(30).saturating_mul(1000));
        if expires <= now {
            return Err(ErrCode::Expired);
        }
        let key = (t.network_id, t.src_device, t.dst_device);
        let old = self.keys.get(&key).copied();
        let old_ip = old
            .and_then(|id| self.entries.get(&id))
            .map(|a| a.binding.ip());
        if (old.is_none()
            && (self.entries.len() >= c.max_allocations
                || self.networks.get(&key.0).copied().unwrap_or(0) >= c.max_per_network))
            || (old_ip != Some(binding.ip())
                && self.ips.get(&binding.ip()).copied().unwrap_or(0) >= c.max_per_ip)
        {
            return Err(ErrCode::Full);
        }
        let id = old.unwrap_or_else(|| {
            loop {
                let id = OsRng.next_u64();
                if id != 0 && !self.entries.contains_key(&id) {
                    break id;
                }
            }
        });
        if let Some(ip) = old_ip {
            decrement(&mut self.ips, ip);
        } else {
            *self.networks.entry(key.0).or_default() += 1;
        }
        *self.ips.entry(binding.ip()).or_default() += 1;
        if let Some(a) = self.entries.get_mut(&id) {
            a.binding = binding;
            a.expires = expires;
        } else {
            self.entries.insert(
                id,
                Allocation {
                    id,
                    key,
                    binding,
                    expires,
                    packets: Bucket::new(c.packets_per_second, now),
                    bytes: Bucket::new(c.bits_per_second / 8, now),
                },
            );
            self.keys.insert(key, id);
        }
        Ok(id)
    }
    pub fn remove(&mut self, id: u64) -> Option<Allocation> {
        let a = self.entries.remove(&id)?;
        self.keys.remove(&a.key);
        decrement(&mut self.networks, a.key.0);
        decrement(&mut self.ips, a.binding.ip());
        Some(a)
    }
    pub fn expired(&self, now: u64) -> Vec<u64> {
        self.entries
            .values()
            .filter(|a| a.expires <= now)
            .map(|a| a.id)
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_rebind_pair_and_expire() {
        let signer = atenea_identity::SigningKey::from_bytes(&[1; 32]);
        let mut t = RelayToken::issue(
            &signer, [0x11; 16], [1; 16], [2; 16], [3; 16], [4; 32], 100, 400, [5; 8],
        );
        let mut table = Table::default();
        let c = Config::default();
        let a = Binding::Udp("127.0.0.1:1".parse().unwrap());
        let b = Binding::Udp("127.0.0.1:2".parse().unwrap());
        let id = table.upsert(&t, a, 100_000, &c).unwrap();
        assert_eq!(table.upsert(&t, b, 110_000, &c).unwrap(), id);
        assert_eq!(table.entries[&id].binding, b);
        assert!(table.peer(table.entries[&id].key, 110_000).is_none());
        std::mem::swap(&mut t.src_device, &mut t.dst_device);
        table.upsert(&t, a, 110_000, &c).unwrap();
        assert!(table.peer(table.entries[&id].key, 110_000).is_some());
        for id in table.expired(231_000) {
            table.remove(id);
        }
        assert!(table.entries.is_empty());
        assert!(table.ips.is_empty());
    }
}
