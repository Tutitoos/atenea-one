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
            sort(&mut p.routes, prefix);
        }
        sort(&mut self.rules, rule);
        self.advertised
            .sort_by_cached_key(|a| encoded(&a.prefix, prefix));
        sort(&mut self.dns.servers, addr);
        self.dns.domains.sort();
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
            && self.advertised.len() <= 8
            && self.advertised.iter().all(|a| a.prefix.valid())
            && self
                .advertised
                .windows(2)
                .all(|a| encoded(&a[0].prefix, prefix) < encoded(&a[1].prefix, prefix))
            && self.dns.servers.len() <= 3
            && ordered(&self.dns.servers, addr)
            && self.dns.domains.len() <= 8
            && self.dns.domains.iter().all(|d| valid_domain(d))
            && self.dns.domains.windows(2).all(|d| d[0] < d[1])
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
                    && p.routes.len() <= 8
                    && ordered(&p.routes, prefix)
                    && p.routes.iter().all(Prefix::valid)
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
        let routes = self
            .peers
            .iter()
            .flat_map(|p| &p.routes)
            .chain(self.advertised.iter().map(|a| &a.prefix));
        if routes.clone().enumerate().any(|(i, p)| {
            !p.valid_route()
                || self.network_prefixes.iter().any(|n| p.overlaps(n))
                || routes.clone().take(i).any(|n| p.overlaps(n))
        }) || !self
            .dns
            .servers
            .iter()
            .all(|a| inside(*a) || routes.clone().any(|p| p.contains(*a)))
        {
            return false;
        }
        self.rules.iter().all(|r| {
            [&r.src, &r.dst].iter().all(|s| match s {
                Selector::Device(id) => {
                    *id == self.device_id || self.peers.iter().any(|p| p.device_id == *id)
                }
                Selector::Prefix(p) => self
                    .network_prefixes
                    .iter()
                    .chain(routes.clone())
                    .any(|n| n.contains_prefix(p)),
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
    fn has_v2_fields(&self) -> bool {
        !self.advertised.is_empty()
            || !self.dns.servers.is_empty()
            || !self.dns.domains.is_empty()
            || self.peers.iter().any(|p| !p.routes.is_empty())
    }
    pub(crate) fn encode_wire(&self) -> Vec<u8> {
        let v2 = self.has_v2_fields();
        let mut b = vec![if v2 { 2 } else { 1 }, 2, 0];
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
            if v2 {
                b.push(p.routes.len() as u8);
                for route in &p.routes {
                    prefix(&mut b, route);
                }
            }
        }
        b.extend_from_slice(&(self.rules.len() as u16).to_be_bytes());
        for r in &self.rules {
            rule(&mut b, r);
        }
        if v2 {
            b.push(self.advertised.len() as u8);
            for a in &self.advertised {
                prefix(&mut b, &a.prefix);
                b.push(u8::from(a.snat));
            }
            b.push(self.dns.servers.len() as u8);
            for a in &self.dns.servers {
                addr(&mut b, a);
            }
            b.push(self.dns.domains.len() as u8);
            for d in &self.dns.domains {
                b.push(d.len() as u8);
                b.extend_from_slice(d.as_bytes());
            }
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
        let version = r.u8()?;
        if !matches!(version, 1 | 2) || r.u8()? != 2 {
            return Err(Error::UnsupportedVersion);
        }
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
            advertised: Vec::new(),
            dns: DnsConfig::default(),
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
            let routes = if version == 2 {
                r.prefixes()?
            } else {
                Vec::new()
            };
            c.peers.push(Peer {
                device_id,
                noise_pubkey,
                addrs,
                endpoints,
                routes,
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
        if version == 2 {
            let n = r.count8(0, 8)?;
            for _ in 0..n {
                let prefix = r.prefix()?;
                let flags = r.u8()?;
                if flags & !1 != 0 {
                    return Err(Error::Malformed);
                }
                c.advertised.push(AdvertisedRoute {
                    prefix,
                    snat: flags == 1,
                });
            }
            let n = r.count8(0, 3)?;
            c.dns.servers = (0..n).map(|_| r.addr()).collect::<Result<_, _>>()?;
            let n = r.count8(0, 8)?;
            c.dns.domains = (0..n).map(|_| r.domain()).collect::<Result<_, _>>()?;
            if !c.has_v2_fields() {
                return Err(Error::Malformed);
            }
        }
        r.end()?;
        if !c.wire_valid() {
            return Err(Error::Malformed);
        }
        Ok(c)
    }
}
