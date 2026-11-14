// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Máquina de estados del relay (`apps/relay/src/state.rs::State::process`, `remove` y `sweep`, las reales).
//! Entrada estructurada: una secuencia de ≤ 48 operaciones sobre 8 orígenes (UDP y TCP) con dos redes y cuatro
//! dispositivos. El arnés FIRMA con claves fijas de fuzz (coordinator, dispositivos) y calcula la cookie UDP
//! válida, de modo que se alcanzan la comprobación de token, la prueba de posesión, la tabla y el reenvío;
//! las banderas de `Alloc` los alteran (relay, token caducado o de vida excesiva, firma ajena, cookie, `ts`,
//! bytes del token). Las claves son públicas: **solo** para fuzzing.
//!
//! Tras cada operación se comprueba que: nada entra en pánico; todo lo emitido es un mensaje relay válido; lo
//! que se devuelve a un origen UDP no supera lo que ese origen envió (sin amplificación, T18); un origen sin
//! asignación que no envía un ALLOC no provoca ninguna salida hacia terceros; un DATA solo se reenvía a la
//! asignación emparejada de la MISMA red y con origen/destino intercambiados (T19); todo PEER_STATE va a la
//! asignación a la que se refiere; y la tabla respeta los topes total, por red y por origen.
#![no_main]
use arbitrary::Unstructured;
use atenea_identity::SigningKey;
use atenea_relay::{
    Output, State,
    alloc::Binding,
    config::Config,
    limits::ip_group,
};
use atenea_relay_core::{self as wire, CookieMaker, Message, RelayToken};
use libfuzzer_sys::fuzz_target;
use std::{collections::HashMap, net::SocketAddr};

const RELAY: [u8; 16] = [0x11; 16];
const SECRET: [u8; 32] = [8; 32];
const NETWORKS: [[u8; 16]; 2] = [[7; 16], [8; 16]];
const START_SEC: u64 = 1_000_000;
const MAX_ALLOCATIONS: usize = 6;
const MAX_PER_NETWORK: usize = 4;
const MAX_PER_IP: usize = 3;

fn key(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}

fn binding(who: u8) -> Binding {
    match who % 8 {
        n @ 0..=3 => Binding::Udp(SocketAddr::from(([127, 0, 0, 1 + n], 1000 + u16::from(n)))),
        // Mismo origen IP que el 0, otro puerto: ejercita el tope por origen.
        4 => Binding::Udp(SocketAddr::from(([127, 0, 0, 1], 2000))),
        n => Binding::Tcp(u64::from(n), [10, 0, 0, n].into()),
    }
}

fn byte(u: &mut Unstructured) -> u8 {
    u.arbitrary().unwrap_or(0)
}

struct Harness {
    state: State,
    cookies: CookieMaker,
    now: u64,
    known: Vec<u64>,
}

impl Harness {
    fn sec(&self) -> u64 {
        self.now / 1000
    }

    fn alloc_message(&self, b: Binding, u: &mut Unstructured) -> Vec<u8> {
        let flags = byte(u);
        let (net, src, dst) = (NETWORKS[usize::from(byte(u) & 1)], 2 + byte(u) % 4, 2 + byte(u) % 4);
        let sec = self.sec();
        let (not_before, mut not_after) = (sec.saturating_sub(10), sec + 100 + u64::from(byte(u)));
        if flags & 0x20 != 0 {
            not_after = sec.saturating_sub(100); // caducado
        }
        if flags & 0x40 != 0 {
            not_after = not_before + 700; // vida superior al máximo
        }
        let token_relay = if flags & 0x02 != 0 { [0x33; 16] } else { RELAY };
        let mut token = RelayToken::issue(&key(1), token_relay, net, [src; 16], [dst; 16], key(src).verifying_key().to_bytes(), not_before, not_after, [byte(u); 8]).encode();
        if flags & 0x80 != 0 {
            let at = usize::from(byte(u)) % token.len();
            token[at] ^= 1 << (byte(u) % 8);
        }
        let cookie = match b {
            Binding::Udp(a) => self.cookies.cookie(a, sec),
            Binding::Tcp(..) => [0; 16],
        };
        let cookie = if flags & 0x04 != 0 { [0xff; 16] } else { cookie };
        let ts = if flags & 0x08 != 0 { sec + 40 } else { sec };
        let signer = if flags & 0x10 != 0 { key(9) } else { key(src) };
        let message_relay = if flags & 0x01 != 0 { [0x33; 16] } else { RELAY };
        wire::encode_alloc(&message_relay, &cookie, ts, &token, &wire::sign_alloc(&signer, &message_relay, &cookie, ts, &token))
    }

    fn pick(&self, u: &mut Unstructured) -> u64 {
        match (byte(u), self.known.is_empty()) {
            (n, false) if n % 4 != 0 => self.known[usize::from(n) % self.known.len()],
            _ => u.arbitrary().unwrap_or(1),
        }
    }

    fn process(&mut self, b: Binding, data: &[u8]) {
        let bound_before = self.state.table.entries.values().any(|a| a.binding == b);
        let source_key = match wire::parse(data) {
            Ok(Message::Data { alloc_id, .. }) => self.state.table.entries.get(&alloc_id).filter(|a| a.binding == b).map(|a| a.key),
            _ => None,
        };
        let is_alloc = matches!(wire::parse(data), Ok(Message::Alloc { .. }));
        let out = self.state.process(b, data, self.now);
        if matches!(b, Binding::Udp(_)) {
            let back: usize = out.iter().filter(|(to, _)| *to == b).map(|(_, body)| body.len()).sum();
            assert!(back <= data.len(), "respuesta de {back} bytes a un origen UDP que envió {}", data.len());
        }
        if !is_alloc && !bound_before {
            assert!(out.iter().all(|(to, _)| *to == b), "un origen sin asignación provocó una salida hacia un tercero");
        }
        self.check_outputs(&out, source_key);
        self.check_table();
    }

    fn check_outputs(&mut self, out: &[Output], source_key: Option<([u8; 16], [u8; 16], [u8; 16])>) {
        for (to, body) in out {
            match wire::parse(body).expect("el relay emitió un mensaje que él mismo no parsea") {
                Message::AllocOk { alloc_id, .. } => {
                    if !self.known.contains(&alloc_id) && self.known.len() < 16 {
                        self.known.push(alloc_id);
                    }
                }
                Message::Data { alloc_id, datagram } => {
                    assert!(wire::valid_datagram(datagram));
                    let target = self.state.table.entries.get(&alloc_id).expect("DATA hacia una asignación inexistente");
                    assert_eq!(target.binding, *to);
                    let key = source_key.expect("DATA reenviado sin una asignación de origen");
                    assert_eq!(target.key, (key.0, key.2, key.1), "DATA reenviado fuera de su pareja o de su red");
                }
                Message::PeerState { alloc_id, .. } => {
                    let target = self.state.table.entries.get(&alloc_id).expect("PEER_STATE sobre una asignación inexistente");
                    assert_eq!(target.binding, *to);
                }
                _ => {}
            }
        }
    }

    fn check_table(&self) {
        let entries = &self.state.table.entries;
        assert!(entries.len() <= MAX_ALLOCATIONS);
        let (mut per_network, mut per_ip) = (HashMap::new(), HashMap::new());
        for a in entries.values() {
            *per_network.entry(a.key.0).or_insert(0usize) += 1;
            *per_ip.entry(ip_group(a.binding.ip())).or_insert(0usize) += 1;
        }
        assert!(per_network.values().all(|&n| n <= MAX_PER_NETWORK));
        assert!(per_ip.values().all(|&n| n <= MAX_PER_IP));
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let config = Config {
        relay_id: "11".repeat(16),
        max_allocations: MAX_ALLOCATIONS,
        max_per_network: MAX_PER_NETWORK,
        max_per_ip: MAX_PER_IP,
        alloc_per_second: 1000,
        cookie_per_second: 1000,
        ..Config::default()
    };
    let mut h = Harness {
        state: State::new(config, vec![key(1).verifying_key().to_bytes()], SECRET),
        cookies: CookieMaker::new(SECRET, RELAY),
        now: START_SEC * 1000,
        known: Vec::new(),
    };
    for _ in 0..48 {
        if u.is_empty() {
            break;
        }
        let who = binding(byte(&mut u));
        match byte(&mut u) % 8 {
            0 => {
                let n = usize::from(u.arbitrary::<u16>().unwrap_or(0)) % 400;
                let bytes: Vec<u8> = (0..n).map(|_| byte(&mut u)).collect();
                h.process(who, &bytes);
            }
            1 => {
                let relay = if byte(&mut u) & 1 == 0 { RELAY } else { [0x33; 16] };
                h.process(who, &wire::encode_cookie_req(&relay));
            }
            2 | 3 => {
                let message = h.alloc_message(who, &mut u);
                h.process(who, &message);
            }
            4 => {
                let id = h.pick(&mut u);
                let extra = usize::from(byte(&mut u)) % 64;
                let mut datagram = vec![wire::WIRE_VERSION, 3];
                datagram.extend(std::iter::repeat_n(0u8, wire::WIRE_DATA_MIN_LEN.saturating_sub(2) + extra));
                h.process(who, &wire::encode_data(id, &datagram));
            }
            5 => {
                let id = h.pick(&mut u);
                h.process(who, &wire::encode_ping(id, u.arbitrary().unwrap_or(0), false));
            }
            6 => h.now += u64::from(u.arbitrary::<u16>().unwrap_or(0)) * 4,
            _ => {
                let out = if byte(&mut u) & 1 == 0 {
                    h.state.sweep(h.now)
                } else {
                    let id = h.pick(&mut u);
                    h.state.remove(vec![id], h.now)
                };
                h.check_outputs(&out, None);
                h.check_table();
            }
        }
    }
});
