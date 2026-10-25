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
    relay_id: wire::RelayId,
    cookie: CookieMaker,
    rates: IpRates,
    issuers: Vec<[u8; 32]>,
}
impl State {
    pub fn new(config: Config, issuers: Vec<[u8; 32]>, secret: [u8; 32]) -> Self {
        let relay_id = config.id().expect("relay_id validado");
        Self {
            config,
            relay_id,
            table: Table::default(),
            counters: Counters::default(),
            cookie: CookieMaker::new(secret, relay_id),
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
                return vec![];
            }
        };
        match msg {
            Message::CookieReq { relay_id } => {
                if relay_id != self.relay_id {
                    return vec![];
                }
                if let Binding::Udp(a) = b {
                    if self.rates.take(
                        b.ip(),
                        false,
                        self.config.cookie_per_second,
                        self.config.max_rate_entries,
                        now,
                    ) {
                        return vec![(
                            b,
                            wire::encode_cookie(&relay_id, &self.cookie.cookie(a, sec)),
                        )];
                    }
                    self.drop_packet("cookie_rate");
                }
            }
            Message::Alloc {
                relay_id,
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
                    Binding::Tcp(..) if !wire::valid_tcp_cookie(&cookie) => {
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
                if relay_id != self.relay_id || t.relay_id != relay_id {
                    return self.reject(b, ErrCode::WrongRelay);
                }
                if let Err(e) = t.verify(&self.issuers, sec) {
                    return self.reject(
                        b,
                        match e {
                            TokenError::Expired | TokenError::NotYetValid | TokenError::TooLong => {
                                ErrCode::Expired
                            }
                            _ => ErrCode::BadToken,
                        },
                    );
                }
                if ts.abs_diff(sec) > 30 {
                    return self.reject(b, ErrCode::Expired);
                }
                if !wire::verify_alloc(&t.src_identity_pub, &relay_id, &cookie, ts, token, &sig) {
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
            [0x11; 16],
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
                &[0x11; 16],
                &cookie,
                ts,
                &token,
                &wire::sign_alloc(&key(src), &[0x11; 16], &cookie, ts, &token),
            ),
            now,
        )
    }
    #[test]
    fn expired_cookie_sweep_and_peer_state() {
        let mut s = State::new(
            Config {
                relay_id: "11".repeat(16),
                ..Config::default()
            },
            vec![key(1).verifying_key().to_bytes()],
            [8; 32],
        );
        let a = Binding::Udp("127.0.0.1:1".parse().unwrap());
        let b = Binding::Udp("127.0.0.1:2".parse().unwrap());
        let old = s.cookie.cookie("127.0.0.1:1".parse().unwrap(), 100);
        let t = RelayToken::issue(
            &key(1),
            [0x11; 16],
            [7; 16],
            [2; 16],
            [3; 16],
            key(2).verifying_key().to_bytes(),
            100,
            400,
            [1; 8],
        )
        .encode();
        let req = wire::encode_alloc(
            &[0x11; 16],
            &old,
            240,
            &t,
            &wire::sign_alloc(&key(2), &[0x11; 16], &old, 240, &t),
        );
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
                relay_id: "11".repeat(16),
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
            relay_id: "11".repeat(16),
            packets_per_second: 1,
            bits_per_second: 240,
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
            s.process(
                a,
                &wire::encode_data(id, &[2, 3].into_iter().chain([0; 28]).collect::<Vec<_>>()),
                100_000
            )
            .len(),
            1
        );
        assert!(
            s.process(
                a,
                &wire::encode_data(id, &[2, 3].into_iter().chain([0; 28]).collect::<Vec<_>>()),
                100_000
            )
            .is_empty()
        );
        assert!(
            s.process(
                a,
                &wire::encode_data(id, &[2, 3].into_iter().chain([0; 29]).collect::<Vec<_>>()),
                101_000
            )
            .is_empty()
        );
    }
    #[test]
    fn per_ip_quota_covers_the_whole_ipv6_64() {
        let c = Config {
            relay_id: "11".repeat(16),
            max_per_ip: 1,
            alloc_per_second: 100,
            ..Config::default()
        };
        let mut s = State::new(c, vec![key(1).verifying_key().to_bytes()], [0; 32]);
        let a = Binding::Tcp(1, "2001:db8:1:2::1".parse().unwrap());
        let same = Binding::Tcp(2, "2001:db8:1:2:aaaa::2".parse().unwrap());
        let other = Binding::Tcp(3, "2001:db8:1:3::1".parse().unwrap());
        assert!(matches!(
            wire::parse(&alloc(&mut s, a, 2, 3, 100_000)[0].1),
            Ok(Message::AllocOk { .. })
        ));
        assert!(matches!(
            wire::parse(&alloc(&mut s, same, 4, 5, 100_000)[0].1),
            Ok(Message::AllocErr {
                code: ErrCode::Full
            })
        ));
        assert!(matches!(
            wire::parse(&alloc(&mut s, other, 4, 5, 100_000)[0].1),
            Ok(Message::AllocOk { .. })
        ));
    }

    #[test]
    fn wrong_audience_never_creates_refreshes_or_moves_allocations() {
        let config = |id: &str| Config {
            relay_id: id.repeat(16),
            alloc_per_second: 100,
            ..Config::default()
        };
        let mut a = State::new(
            config("11"),
            vec![key(1).verifying_key().to_bytes()],
            [8; 32],
        );
        let mut b = State::new(
            config("12"),
            vec![key(1).verifying_key().to_bytes()],
            [8; 32],
        );
        for binding in [
            Binding::Tcp(1, "127.0.0.1".parse().unwrap()),
            Binding::Udp("127.0.0.1:7".parse().unwrap()),
        ] {
            let token = RelayToken::issue(
                &key(1),
                [0x11; 16],
                [7; 16],
                [2; 16],
                [3; 16],
                key(2).verifying_key().to_bytes(),
                100,
                400,
                [1; 8],
            )
            .encode();
            let cookie = match binding {
                Binding::Udp(src) => b.cookie.cookie(src, 100),
                _ => [0; 16],
            };
            let proof = wire::sign_alloc(&key(2), &[0x11; 16], &cookie, 100, &token);
            for id in [[0x11; 16], [0x12; 16]] {
                let req = wire::encode_alloc(&id, &cookie, 100, &token, &proof);
                let out = b.process(binding, &req, 100_000);
                assert_eq!(
                    wire::parse(&out[0].1),
                    Ok(Message::AllocErr {
                        code: ErrCode::WrongRelay
                    })
                );
                assert!(b.table.entries.is_empty());
            }
            let good_cookie = match binding {
                Binding::Udp(src) => a.cookie.cookie(src, 100),
                _ => [0; 16],
            };
            let good = wire::encode_alloc(
                &[0x11; 16],
                &good_cookie,
                100,
                &token,
                &wire::sign_alloc(&key(2), &[0x11; 16], &good_cookie, 100, &token),
            );
            let first = a.process(binding, &good, 100_000);
            assert!(matches!(
                wire::parse(&first[0].1),
                Ok(Message::AllocOk { .. })
            ));
            let before = a
                .table
                .entries
                .values()
                .map(|v| (v.id, v.binding, v.expires))
                .collect::<Vec<_>>();
            let mut changed = token;
            changed[5] = 0x12;
            let wrong = wire::encode_alloc(&[0x11; 16], &good_cookie, 100, &changed, &proof);
            assert_eq!(
                wire::parse(&a.process(binding, &wrong, 101_000)[0].1),
                Ok(Message::AllocErr {
                    code: ErrCode::WrongRelay
                })
            );
            assert_eq!(
                a.table
                    .entries
                    .values()
                    .map(|v| (v.id, v.binding, v.expires))
                    .collect::<Vec<_>>(),
                before
            );
            // Repetición dentro de la misma audiencia está expresamente permitida.
            assert!(matches!(
                wire::parse(&a.process(binding, &good, 101_000)[0].1),
                Ok(Message::AllocOk { .. })
            ));
            a.table = Table::default();
        }
    }

    #[test]
    fn alloc_time_edges_cookie_and_authorization_errors_do_not_change_state() {
        let id = [0x11; 16];
        let config = Config {
            relay_id: "11".repeat(16),
            alloc_per_second: 100,
            ..Config::default()
        };
        let mut state = State::new(config, vec![key(1).verifying_key().to_bytes()], [8; 32]);
        let binding = Binding::Tcp(1, "127.0.0.1".parse().unwrap());
        let token = RelayToken::issue(
            &key(1),
            id,
            [7; 16],
            [2; 16],
            [3; 16],
            key(2).verifying_key().to_bytes(),
            100,
            400,
            [1; 8],
        )
        .encode();
        for ts in [70, 130] {
            let req = wire::encode_alloc(
                &id,
                &[0; 16],
                ts,
                &token,
                &wire::sign_alloc(&key(2), &id, &[0; 16], ts, &token),
            );
            assert!(matches!(
                wire::parse(&state.process(binding, &req, 100_000)[0].1),
                Ok(Message::AllocOk { .. })
            ));
        }
        let before = state
            .table
            .entries
            .values()
            .map(|v| (v.id, v.binding, v.expires))
            .collect::<Vec<_>>();
        for (cookie, ts, code) in [
            ([0; 16], 69, ErrCode::Expired),
            ([0; 16], 131, ErrCode::Expired),
            ([1; 16], 100, ErrCode::BadCookie),
        ] {
            let req = wire::encode_alloc(
                &id,
                &cookie,
                ts,
                &token,
                &wire::sign_alloc(&key(2), &id, &cookie, ts, &token),
            );
            assert_eq!(
                wire::parse(&state.process(binding, &req, 100_000)[0].1),
                Ok(Message::AllocErr { code })
            );
        }
        let req = wire::encode_alloc(
            &id,
            &[0; 16],
            430,
            &token,
            &wire::sign_alloc(&key(2), &id, &[0; 16], 430, &token),
        );
        assert_eq!(
            wire::parse(&state.process(binding, &req, 430_000)[0].1),
            Ok(Message::AllocErr {
                code: ErrCode::Expired
            })
        );
        let same = RelayToken::issue(
            &key(1),
            id,
            [7; 16],
            [2; 16],
            [2; 16],
            key(2).verifying_key().to_bytes(),
            100,
            400,
            [1; 8],
        )
        .encode();
        let req = wire::encode_alloc(
            &id,
            &[0; 16],
            100,
            &same,
            &wire::sign_alloc(&key(2), &id, &[0; 16], 100, &same),
        );
        assert_eq!(
            wire::parse(&state.process(binding, &req, 100_000)[0].1),
            Ok(Message::AllocErr {
                code: ErrCode::Unauthorized
            })
        );
        assert_eq!(
            state
                .table
                .entries
                .values()
                .map(|v| (v.id, v.binding, v.expires))
                .collect::<Vec<_>>(),
            before
        );
    }
}
