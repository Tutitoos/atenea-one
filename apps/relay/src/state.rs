use crate::{
    alloc::{Binding, Table},
    config::Config,
    limits::IpRates,
};
use atenea_relay_core::{self as wire, CookieMaker, ErrCode, Message, RelayToken, TokenError};
use serde::Serialize;
use std::collections::BTreeMap;

pub type Output = (Binding, Vec<u8>);
#[derive(Default, Serialize)]
pub struct Counters {
    pub forwarded_packets: u64,
    pub forwarded_bytes: u64,
    pub udp_packets: u64,
    pub udp_bytes: u64,
    pub tcp_packets: u64,
    pub tcp_bytes: u64,
    pub drops: BTreeMap<&'static str, u64>,
    pub rejects: BTreeMap<u8, u64>,
    pub udp_received_packets: u64,
    pub udp_received_bytes: u64,
    pub tcp_received_packets: u64,
    pub tcp_received_bytes: u64,
    pub allocations: u64,
}
pub struct State {
    pub config: Config,
    pub table: Table,
    pub counters: Counters,
    cookie: CookieMaker,
    rates: IpRates,
    issuers: Vec<[u8; 32]>,
}
impl State {
    pub fn new(config: Config, issuers: Vec<[u8; 32]>, secret: [u8; 32]) -> Self {
        Self {
            config,
            table: Table::default(),
            counters: Counters::default(),
            cookie: CookieMaker::new(secret),
            rates: IpRates::default(),
            issuers,
        }
    }
    pub fn drop_packet(&mut self, reason: &'static str) {
        *self.counters.drops.entry(reason).or_default() += 1;
    }
    fn reject(&mut self, b: Binding, code: ErrCode) -> Vec<Output> {
        *self.counters.rejects.entry(code as u8).or_default() += 1;
        vec![(b, wire::encode_alloc_err(code))]
    }
    pub fn process(&mut self, b: Binding, data: &[u8], now: u64) -> Vec<Output> {
        let sec = now / 1000;
        let msg = match wire::parse(data) {
            Ok(m) => m,
            Err(_) => {
                self.drop_packet("malformed");
                if data.starts_with(&[wire::MAGIC, wire::ty::ALLOC]) {
                    let validated = match b {
                        Binding::Tcp(..) => true,
                        Binding::Udp(a) => data
                            .get(2..18)
                            .and_then(|c| c.try_into().ok())
                            .is_some_and(|c| self.cookie.verify(a, &c, sec)),
                    };
                    if validated {
                        let code = if self.rates.take(
                            b.ip(),
                            true,
                            self.config.alloc_per_second,
                            self.config.max_rate_entries,
                            now,
                        ) {
                            ErrCode::BadToken
                        } else {
                            ErrCode::RateLimited
                        };
                        return self.reject(b, code);
                    }
                }
                return vec![];
            }
        };
        match msg {
            Message::CookieReq => {
                if let Binding::Udp(a) = b {
                    if self.rates.take(
                        b.ip(),
                        false,
                        self.config.cookie_per_second,
                        self.config.max_rate_entries,
                        now,
                    ) {
                        return vec![(b, wire::encode_cookie(&self.cookie.cookie(a, sec)))];
                    }
                    self.drop_packet("cookie_rate");
                }
            }
            Message::Alloc {
                cookie,
                ts,
                token,
                sig,
            } => {
                match b {
                    Binding::Udp(a) if !self.cookie.verify(a, &cookie, sec) => {
                        self.drop_packet("cookie");
                        return vec![];
                    }
                    Binding::Tcp(..) if cookie != [0; 16] => {
                        return self.reject(b, ErrCode::BadCookie);
                    }
                    _ => {}
                }
                if !self.rates.take(
                    b.ip(),
                    true,
                    self.config.alloc_per_second,
                    self.config.max_rate_entries,
                    now,
                ) {
                    return self.reject(b, ErrCode::RateLimited);
                }
                let t = match RelayToken::parse(token) {
                    Ok(t) => t,
                    Err(_) => return self.reject(b, ErrCode::BadToken),
                };
                if let Err(e) = t.verify(&self.issuers, sec) {
                    return self.reject(
                        b,
                        match e {
                            TokenError::Expired | TokenError::NotYetValid => ErrCode::Expired,
                            _ => ErrCode::BadToken,
                        },
                    );
                }
                if ts.abs_diff(sec) > 30 {
                    return self.reject(b, ErrCode::Expired);
                }
                if !wire::verify_alloc(&t.src_identity_pub, &cookie, ts, token, &sig) {
                    return self.reject(b, ErrCode::BadSignature);
                }
                if t.src_device == t.dst_device {
                    return self.reject(b, ErrCode::Unauthorized);
                }
                let id = match self.table.upsert(&t, b, now, &self.config) {
                    Ok(id) => id,
                    Err(e) => return self.reject(b, e),
                };
                self.counters.allocations += 1;
                let a = &self.table.entries[&id];
                let peer = self.table.peer(a.key, now);
                let mut out = vec![(
                    b,
                    wire::encode_alloc_ok(id, ((a.expires - now) / 1000) as u16, peer.is_some()),
                )];
                if let Some(p) = peer {
                    out.push((p.binding, wire::encode_peer_state(p.id, true)));
                }
                out.push((b, wire::encode_peer_state(id, peer.is_some())));
                return out;
            }
            Message::Data { alloc_id, datagram } => {
                match b {
                    Binding::Udp(_) => {
                        self.counters.udp_received_packets += 1;
                        self.counters.udp_received_bytes += datagram.len() as u64;
                    }
                    Binding::Tcp(..) => {
                        self.counters.tcp_received_packets += 1;
                        self.counters.tcp_received_bytes += datagram.len() as u64;
                    }
                }
                let Some(a) = self
                    .table
                    .entries
                    .get_mut(&alloc_id)
                    .filter(|a| a.binding == b && a.expires > now)
                else {
                    self.drop_packet("unbound");
                    return vec![];
                };
                if datagram.len() > self.config.max_datagram {
                    self.drop_packet("size");
                    return vec![];
                }
                if !a.packets.take(self.config.packets_per_second, 1, now)
                    || !a
                        .bytes
                        .take(self.config.bits_per_second / 8, datagram.len(), now)
                {
                    self.drop_packet("allocation_rate");
                    return vec![];
                }
                let key = a.key;
                let Some(p) = self.table.peer(key, now) else {
                    self.drop_packet("no_peer");
                    return vec![];
                };
                return vec![(p.binding, wire::encode_data(p.id, datagram))];
            }
            Message::Ping { alloc_id, nonce } => {
                if self
                    .table
                    .entries
                    .get(&alloc_id)
                    .is_some_and(|a| a.binding == b && a.expires > now)
                {
                    return vec![(b, wire::encode_ping(alloc_id, nonce, true))];
                }
                self.drop_packet("unbound");
            }
            _ => self.drop_packet("unexpected"),
        }
        vec![]
    }
    pub fn remove(&mut self, ids: Vec<u64>, now: u64) -> Vec<Output> {
        let mut out = vec![];
        for id in ids {
            if let Some(a) = self.table.remove(id)
                && let Some(p) = self.table.peer(a.key, now)
            {
                out.push((p.binding, wire::encode_peer_state(p.id, false)));
            }
        }
        out
    }
    pub fn sweep(&mut self, now: u64) -> Vec<Output> {
        self.rates.sweep(now);
        self.remove(self.table.expired(now), now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atenea_identity::SigningKey;
    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }
    fn alloc(s: &mut State, b: Binding, src: u8, dst: u8, now: u64) -> Vec<Output> {
        let token = RelayToken::issue(
            &key(1),
            [7; 16],
            [src; 16],
            [dst; 16],
            key(src).verifying_key().to_bytes(),
            100,
            400,
            [1; 8],
        )
        .encode();
        let cookie = match b {
            Binding::Udp(a) => s.cookie.cookie(a, now / 1000),
            _ => [0; 16],
        };
        let ts = now / 1000;
        s.process(
            b,
            &wire::encode_alloc(
                &cookie,
                ts,
                &token,
                &wire::sign_alloc(&key(src), &cookie, ts, &token),
            ),
            now,
        )
    }
    #[test]
    fn expired_cookie_sweep_and_peer_state() {
        let mut s = State::new(
            Config::default(),
            vec![key(1).verifying_key().to_bytes()],
            [8; 32],
        );
        let a = Binding::Udp("127.0.0.1:1".parse().unwrap());
        let b = Binding::Udp("127.0.0.1:2".parse().unwrap());
        let old = s.cookie.cookie("127.0.0.1:1".parse().unwrap(), 100);
        let t = RelayToken::issue(
            &key(1),
            [7; 16],
            [2; 16],
            [3; 16],
            key(2).verifying_key().to_bytes(),
            100,
            400,
            [1; 8],
        )
        .encode();
        let req = wire::encode_alloc(&old, 240, &t, &wire::sign_alloc(&key(2), &old, 240, &t));
        assert!(s.process(a, &req, 240_000).is_empty());
        alloc(&mut s, a, 2, 3, 100_000);
        let out = alloc(&mut s, b, 3, 2, 110_000);
        assert!(out.iter().any(|(binding, body)| *binding == a
            && matches!(
                wire::parse(body),
                Ok(Message::PeerState { ready: true, .. })
            )));
        let out = s.sweep(221_000);
        assert_eq!(s.table.entries.len(), 1);
        assert!(out.iter().any(|(_, body)| matches!(
            wire::parse(body),
            Ok(Message::PeerState { ready: false, .. })
        )));
        s.sweep(231_000);
        assert!(s.table.entries.is_empty());
    }
    #[test]
    fn allocation_ip_network_packet_and_byte_limits() {
        for (ip_limit, net_limit, total) in [(1, 10, 10), (10, 1, 10), (10, 10, 1)] {
            let c = Config {
                max_per_ip: ip_limit,
                max_per_network: net_limit,
                max_allocations: total,
                alloc_per_second: 100,
                ..Config::default()
            };
            let mut s = State::new(c, vec![key(1).verifying_key().to_bytes()], [0; 32]);
            let a = Binding::Udp("127.0.0.1:1".parse().unwrap());
            let b = Binding::Udp("127.0.0.1:2".parse().unwrap());
            alloc(&mut s, a, 2, 3, 100_000);
            assert!(matches!(
                wire::parse(&alloc(&mut s, b, 3, 2, 100_000)[0].1),
                Ok(Message::AllocErr {
                    code: ErrCode::Full
                })
            ));
        }
        let c = Config {
            packets_per_second: 1,
            bits_per_second: 16,
            alloc_per_second: 1,
            ..Config::default()
        };
        let mut s = State::new(c, vec![key(1).verifying_key().to_bytes()], [0; 32]);
        let a = Binding::Tcp(1, "127.0.0.1".parse().unwrap());
        let b = Binding::Tcp(2, "127.0.0.2".parse().unwrap());
        let out = alloc(&mut s, a, 2, 3, 100_000);
        let id = match wire::parse(&out[0].1).unwrap() {
            Message::AllocOk { alloc_id, .. } => alloc_id,
            _ => panic!(),
        };
        assert!(matches!(
            wire::parse(&alloc(&mut s, a, 2, 3, 100_000)[0].1),
            Ok(Message::AllocErr {
                code: ErrCode::RateLimited
            })
        ));
        alloc(&mut s, b, 3, 2, 100_000);
        assert_eq!(
            s.process(a, &wire::encode_data(id, b"xx"), 100_000).len(),
            1
        );
        assert!(
            s.process(a, &wire::encode_data(id, b"xx"), 100_000)
                .is_empty()
        );
        assert!(
            s.process(a, &wire::encode_data(id, b"xxx"), 101_000)
                .is_empty()
        );
    }
}
