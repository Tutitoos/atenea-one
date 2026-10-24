use crate::{codec::*, *};
use std::collections::HashSet;
impl DeviceConfig {
    pub fn canonicalize(&mut self) {
        sort(&mut self.tunnel_addrs, addr);
        sort(&mut self.network_prefixes, prefix);
        self.peers.sort_by_key(|p| p.device_id);
        for p in &mut self.peers {
            sort(&mut p.addrs, addr);
            sort(&mut p.endpoints, endpoint);
        }
        sort(&mut self.rules, rule);
    }
    fn wire_valid(&self) -> bool {
        (1280..=1500).contains(&self.mtu)
            && self.config_version > 0
            && self.not_after > self.not_before
            && self.not_after - self.not_before <= MAX_CONFIG_VALIDITY_SECS
            && (1..=4).contains(&self.tunnel_addrs.len())
            && (1..=4).contains(&self.network_prefixes.len())
            && self.peers.len() <= 256
            && self.rules.len() <= 256
            && ordered(&self.tunnel_addrs, addr)
            && ordered(&self.network_prefixes, prefix)
            && self.network_prefixes.iter().all(Prefix::valid)
            && self
                .peers
                .windows(2)
                .all(|p| p[0].device_id < p[1].device_id)
            && self.peers.iter().all(|p| {
                (1..=4).contains(&p.addrs.len())
                    && p.endpoints.len() <= 4
                    && ordered(&p.addrs, addr)
                    && ordered(&p.endpoints, endpoint)
                    && p.endpoints.iter().all(endpoint_valid)
            })
            && ordered(&self.rules, rule)
            && self.rules.iter().all(|r| {
                matches!(r.proto, 0 | 1 | 6 | 17 | 58)
                    && r.port_min <= r.port_max
                    && (matches!(r.proto, 6 | 17) || (r.port_min == 0 && r.port_max == 0))
                    && [&r.src, &r.dst].iter().all(|s| match s {
                        Selector::Prefix(p) => p.valid(),
                        _ => true,
                    })
            })
    }
    pub(crate) fn semantic_valid(&self) -> bool {
        let inside = |a| self.network_prefixes.iter().any(|p| p.contains(a));
        let mut addresses = HashSet::new();
        let mut keys = HashSet::new();
        if !self
            .tunnel_addrs
            .iter()
            .all(|a| inside(*a) && addresses.insert(*a))
        {
            return false;
        }
        for p in &self.peers {
            if p.device_id == self.device_id
                || p.noise_pubkey == self.noise_pubkey
                || p.noise_pubkey == self.identity_pubkey
                || !keys.insert(p.noise_pubkey)
                || !p.addrs.iter().all(|a| inside(*a) && addresses.insert(*a))
            {
                return false;
            }
        }
        self.rules.iter().all(|r| {
            [&r.src, &r.dst].iter().all(|s| match s {
                Selector::Device(id) => {
                    *id == self.device_id || self.peers.iter().any(|p| p.device_id == *id)
                }
                Selector::Prefix(p) => self.network_prefixes.iter().any(|n| {
                    n.addr.is_ipv4() == p.addr.is_ipv4() && n.len <= p.len && n.contains(p.addr)
                }),
                Selector::Any => true,
            })
        })
    }
    pub fn validate_structure(&self) -> Result<(), Error> {
        if self.wire_valid() && self.semantic_valid() {
            Ok(())
        } else {
            Err(Error::InvalidStructure)
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate_structure()?;
        let b = self.encode_wire();
        if b.len() > MAX_BODY_LEN {
            Err(Error::TooLarge)
        } else {
            Ok(b)
        }
    }
    pub(crate) fn encode_wire(&self) -> Vec<u8> {
        let mut b = vec![1, 2, 0];
        b.extend_from_slice(&self.network_id);
        b.extend_from_slice(&self.device_id);
        for n in [
            self.issuer_serial,
            self.config_version,
            self.not_before,
            self.not_after,
        ] {
            b.extend_from_slice(&n.to_be_bytes());
        }
        b.extend_from_slice(&self.noise_pubkey);
        b.extend_from_slice(&self.identity_pubkey);
        b.extend_from_slice(&self.mtu.to_be_bytes());
        b.push(self.tunnel_addrs.len() as u8);
        for a in &self.tunnel_addrs {
            addr(&mut b, a);
        }
        b.push(self.network_prefixes.len() as u8);
        for p in &self.network_prefixes {
            prefix(&mut b, p);
        }
        b.extend_from_slice(&(self.peers.len() as u16).to_be_bytes());
        for p in &self.peers {
            b.extend_from_slice(&p.device_id);
            b.extend_from_slice(&p.noise_pubkey);
            b.push(0);
            b.push(p.addrs.len() as u8);
            for a in &p.addrs {
                addr(&mut b, a);
            }
            b.push(p.endpoints.len() as u8);
            for e in &p.endpoints {
                endpoint(&mut b, e);
            }
        }
        b.extend_from_slice(&(self.rules.len() as u16).to_be_bytes());
        for r in &self.rules {
            rule(&mut b, r);
        }
        b
    }
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let c = Self::parse_wire(body)?;
        if !c.semantic_valid() {
            return Err(Error::InvalidStructure);
        }
        Ok(c)
    }
    pub(crate) fn parse_wire(body: &[u8]) -> Result<Self, Error> {
        if body.len() > MAX_BODY_LEN {
            return Err(Error::TooLarge);
        }
        let mut r = Reader::new(body);
        r.header(2)?;
        r.zero()?;
        let mut c = Self {
            network_id: r.array()?,
            device_id: r.array()?,
            issuer_serial: r.u64()?,
            config_version: r.u64()?,
            not_before: r.u64()?,
            not_after: r.u64()?,
            noise_pubkey: r.array()?,
            identity_pubkey: r.array()?,
            mtu: r.u16()?,
            tunnel_addrs: r.addrs()?,
            network_prefixes: Vec::new(),
            peers: Vec::new(),
            rules: Vec::new(),
        };
        let n = r.count8(1, 4)?;
        for _ in 0..n {
            c.network_prefixes.push(r.prefix()?);
        }
        let n = r.count16(256)?;
        for _ in 0..n {
            let device_id = r.array()?;
            let noise_pubkey = r.array()?;
            r.zero()?;
            let addrs = r.addrs()?;
            let n = r.count8(0, 4)?;
            let endpoints = (0..n).map(|_| r.endpoint()).collect::<Result<_, _>>()?;
            c.peers.push(Peer {
                device_id,
                noise_pubkey,
                addrs,
                endpoints,
            });
        }
        let n = r.count16(256)?;
        for _ in 0..n {
            c.rules.push(Rule {
                src: r.selector()?,
                dst: r.selector()?,
                proto: r.u8()?,
                port_min: r.u16()?,
                port_max: r.u16()?,
            });
        }
        r.end()?;
        if !c.wire_valid() {
            return Err(Error::Malformed);
        }
        Ok(c)
    }
}
