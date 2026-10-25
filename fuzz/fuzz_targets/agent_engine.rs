//! Máquina de estados del agente (ADR-003, fase 2): dos motores reales conectados por una red
//! simulada que el fuzzer controla (pérdida, duplicado, reordenación, corrupción, basura, avance
//! del reloj y caducidad del lease).
//!
//! Se compila `apps/agent/src/engine.rs` tal cual mediante `#[path]`, sin tocar el agente.
//! Invariantes: sin pánico; todo lo emitido es un datagrama válido del protocolo; lo entregado a la
//! TUN es exactamente lo que el otro extremo envió, como mucho una vez (sin falsificación ni
//! replay); sin lease vigente no se entrega nada a la TUN ni se emite tráfico de datos.
#![no_main]
#![allow(dead_code, unused_imports)]

#[path = "../../apps/agent/src/engine.rs"]
mod engine;
#[path = "../../apps/agent/src/packet.rs"]
mod packet;
/// `relay.rs` arrastra TLS/tokio; el motor solo necesita la etiqueta del camino.
mod relay {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
    pub enum RelayKind {
        Udp,
        Tls,
    }
}

use arbitrary::Unstructured;
use atenea_crypto::StaticKeypair;
use atenea_identity::{DeviceConfig, DnsConfig, Peer, Prefix, Rule, Selector};
use engine::{Engine, Out};
use libfuzzer_sys::fuzz_target;
use relay::RelayKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

const NET: [u8; 16] = [7; 16];
const NOT_BEFORE: u64 = 999_000;
const NOT_AFTER: u64 = 1_100_000;
const EP: [&str; 2] = ["192.0.2.1:51820", "192.0.2.2:51820"];
const MAX_OPS: usize = 400;

fn ep(i: usize) -> SocketAddr {
    EP[i].parse().unwrap()
}

fn tunnel_ip(i: usize) -> Ipv4Addr {
    Ipv4Addr::new(10, 77, 0, i as u8 + 1)
}

fn config(own: usize, peer_pub: [u8; 32]) -> DeviceConfig {
    let peer = 1 - own;
    DeviceConfig {
        network_id: NET,
        device_id: [own as u8 + 1; 16],
        issuer_serial: 1,
        config_version: 1,
        not_before: NOT_BEFORE,
        not_after: NOT_AFTER,
        noise_pubkey: [0; 32],
        identity_pubkey: [0; 32],
        mtu: 1400,
        tunnel_addrs: vec![IpAddr::V4(tunnel_ip(own))],
        network_prefixes: vec![Prefix { addr: "10.77.0.0".parse().unwrap(), len: 24 }],
        peers: vec![Peer {
            device_id: [peer as u8 + 1; 16],
            noise_pubkey: peer_pub,
            addrs: vec![IpAddr::V4(tunnel_ip(peer))],
            endpoints: vec![ep(peer)],
            routes: vec![],
        }],
        rules: vec![Rule { src: Selector::Any, dst: Selector::Any, proto: 0, port_min: 0, port_max: 0 }],
        advertised: vec![],
        dns: DnsConfig { servers: vec![], domains: vec![] },
    }
}

fn ipv4(src: Ipv4Addr, dst: Ipv4Addr, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; 20];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&((20 + payload.len()) as u16).to_be_bytes());
    p[8] = 64;
    p[9] = 17;
    p[12..16].copy_from_slice(&src.octets());
    p[16..20].copy_from_slice(&dst.octets());
    p.extend_from_slice(payload);
    p
}

enum Via {
    Direct,
    Relay(RelayKind),
}

struct InFlight {
    to: usize,
    via: Via,
    bytes: Vec<u8>,
}

struct World {
    engines: [Engine; 2],
    now: Instant,
    unix: [u64; 2],
    queue: Vec<InFlight>,
    /// Paquetes enviados por la TUN de cada extremo y aún no entregados al otro.
    pending: [Vec<Vec<u8>>; 2],
    counter: u32,
}

impl World {
    fn new() -> World {
        let keys = [StaticKeypair::from_private(&[0x11; 32]), StaticKeypair::from_private(&[0x22; 32])];
        let cfgs = [config(0, keys[1].public), config(1, keys[0].public)];
        let now = Instant::now();
        let [ka, kb] = keys;
        let mut engines = [Engine::new(NET, ka, 0), Engine::new(NET, kb, 0)];
        for (e, c) in engines.iter_mut().zip(&cfgs) {
            e.apply_config(c);
        }
        World { engines, now, unix: [1_000_000; 2], queue: vec![], pending: [vec![], vec![]], counter: 0 }
    }

    fn lease(&self, i: usize) -> bool {
        self.engines[i].lease_active(self.unix[i])
    }

    /// Recoge las acciones de `from`: red a la cola, TUN comprobada contra lo enviado.
    fn absorb(&mut self, from: usize, outs: Vec<Out>, had_lease: bool) {
        for out in outs {
            match out {
                Out::Udp(addr, dg) => {
                    assert!(atenea_protocol::parse(&dg).is_ok(), "datagrama emitido inválido");
                    if let Some(to) = (0..2).find(|&j| j != from && ep(j) == addr) {
                        self.queue.push(InFlight { to, via: Via::Direct, bytes: dg });
                    }
                }
                Out::Relay { kind, dg, .. } => {
                    assert!(atenea_protocol::parse(&dg).is_ok(), "datagrama relay emitido inválido");
                    self.queue.push(InFlight { to: 1 - from, via: Via::Relay(kind), bytes: dg });
                }
                Out::Tun(pkt) => {
                    assert!(had_lease, "entrega a la TUN sin lease vigente");
                    let sent = &mut self.pending[1 - from];
                    let at = sent.iter().position(|p| *p == pkt);
                    assert!(at.is_some(), "TUN recibió un paquete que el par no envió (o repetido)");
                    sent.swap_remove(at.unwrap());
                }
            }
        }
    }

    fn deliver(&mut self, item: InFlight) {
        let (to, now, unix) = (item.to, self.now, self.unix[item.to]);
        let lease = self.lease(to);
        let outs = match item.via {
            Via::Direct => self.engines[to].on_udp(ep(1 - to), &item.bytes, now, unix),
            Via::Relay(kind) => self.engines[to].on_relay_rx(&[(1 - to) as u8 + 1; 16], kind, &item.bytes, now, unix),
        };
        self.absorb(to, outs, lease);
    }

    fn tick(&mut self, i: usize) {
        let lease = self.lease(i);
        let outs = self.engines[i].on_tick(self.now, self.unix[i]);
        if !lease {
            assert!(outs.iter().all(|o| !matches!(o, Out::Tun(_))), "tick sin lease entregó a la TUN");
        }
        self.absorb(i, outs, lease);
        let _ = self.engines[i].status(self.now, self.unix[i]);
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let mut w = World::new();
    for _ in 0..MAX_OPS {
        if u.is_empty() {
            break;
        }
        match u.int_in_range(0..=8u8).unwrap_or(0) {
            // Avance del reloj monotónico y de pared (el lease puede caducar y recuperarse).
            0 | 1 => {
                let secs = [0, 1, 1, 5, 20, 100, 200, 130_000][u.int_in_range(0..=7usize).unwrap_or(0)];
                let millis = u.int_in_range(0..=999u64).unwrap_or(0);
                w.now += Duration::from_millis(secs * 1000 + millis);
                let side = u.int_in_range(0..=2usize).unwrap_or(0);
                for i in 0..2 {
                    if side == 2 || side == i {
                        w.unix[i] += secs;
                    }
                }
                for i in 0..2 {
                    w.tick(i);
                }
            }
            // Paquete de la TUN.
            2 | 3 => {
                let from = u.int_in_range(0..=1usize).unwrap_or(0);
                let n = u.int_in_range(0..=48usize).unwrap_or(0);
                let mut payload = w.counter.to_be_bytes().to_vec();
                w.counter += 1;
                payload.extend((0..n).map(|_| u.arbitrary::<u8>().unwrap_or(0)));
                let pkt = ipv4(tunnel_ip(from), tunnel_ip(1 - from), &payload);
                let lease = w.lease(from);
                let outs = w.engines[from].on_tun(&pkt, w.now, w.unix[from]);
                if !lease {
                    assert!(outs.iter().all(|o| !matches!(o, Out::Udp(..) | Out::Relay { .. })), "TUN sin lease emitió tráfico");
                } else {
                    w.pending[from].push(pkt);
                }
                w.absorb(from, outs, lease);
            }
            // Entrega de un elemento cualquiera de la cola (reordenación).
            4 | 5 if !w.queue.is_empty() => {
                let i = u.int_in_range(0..=w.queue.len() - 1).unwrap_or(0);
                let item = w.queue.swap_remove(i);
                w.deliver(item);
            }
            // Duplicado, con o sin cambio de camino.
            6 if !w.queue.is_empty() => {
                let i = u.int_in_range(0..=w.queue.len() - 1).unwrap_or(0);
                let src = &w.queue[i];
                let via = match (&src.via, u.arbitrary::<bool>().unwrap_or(false)) {
                    (Via::Direct, true) => Via::Relay(RelayKind::Udp),
                    (Via::Relay(_), true) => Via::Direct,
                    (Via::Direct, false) => Via::Direct,
                    (Via::Relay(k), false) => Via::Relay(*k),
                };
                let copy = InFlight { to: src.to, via, bytes: src.bytes.clone() };
                w.deliver(copy);
            }
            // Corrupción de un byte y entrega.
            7 if !w.queue.is_empty() => {
                let i = u.int_in_range(0..=w.queue.len() - 1).unwrap_or(0);
                let mut item = w.queue.swap_remove(i);
                if !item.bytes.is_empty() {
                    let at = u.int_in_range(0..=item.bytes.len() - 1).unwrap_or(0);
                    item.bytes[at] ^= u.int_in_range(1..=255u8).unwrap_or(1);
                }
                w.deliver(item);
            }
            // Basura de un origen cualquiera (incluida la dirección real del par).
            _ => {
                let to = u.int_in_range(0..=1usize).unwrap_or(0);
                let from = u.int_in_range(0..=2usize).unwrap_or(0);
                let from: SocketAddr = if from < 2 { ep(from) } else { "203.0.113.9:4000".parse().unwrap() };
                let n = u.int_in_range(0..=200usize).unwrap_or(0);
                let bytes: Vec<u8> = (0..n).map(|_| u.arbitrary::<u8>().unwrap_or(0)).collect();
                let lease = w.lease(to);
                let outs = w.engines[to].on_udp(from, &bytes, w.now, w.unix[to]);
                w.absorb(to, outs, lease);
            }
        }
    }
});
