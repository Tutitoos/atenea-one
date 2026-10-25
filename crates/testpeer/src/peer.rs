//! Par de pruebas sin E/S: autenticación, sesiones y eco IP estricto.
use atenea_crypto::{self as crypto, Session, StaticKeypair};
use atenea_protocol::{self as proto, Packet};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

pub struct Addresses {
    pub own: Ipv4Addr,
    pub agent: Ipv4Addr,
    pub own6: Option<Ipv6Addr>,
    pub agent6: Option<Ipv6Addr>,
}

pub struct Peer {
    keys: StaticKeypair,
    agent_key: [u8; 32],
    network: [u8; 16],
    addresses: Addresses,
    endpoint: Option<SocketAddr>,
    current: Option<Session>,
    previous: Option<Session>,
    next: Option<Session>,
    last_init: Option<u64>,
    cached: Option<(Vec<u8>, Vec<u8>)>,
    last_send: Instant,
    pub echo_replies: u64,
    pub discarded: u64,
}

impl Peer {
    pub fn new(
        keys: StaticKeypair,
        agent_key: [u8; 32],
        network: [u8; 16],
        addresses: Addresses,
        now: Instant,
    ) -> Self {
        Self {
            keys,
            agent_key,
            network,
            addresses,
            endpoint: None,
            current: None,
            previous: None,
            next: None,
            last_init: None,
            cached: None,
            last_send: now,
            echo_replies: 0,
            discarded: 0,
        }
    }

    pub fn receive(
        &mut self,
        from: SocketAddr,
        bytes: &[u8],
        now: Instant,
        unix: u64,
    ) -> Option<Vec<u8>> {
        if self.endpoint.is_some_and(|endpoint| endpoint != from) {
            self.discarded += 1;
            return None;
        }
        let result = self.process(from, bytes, now, unix);
        if result.is_err() {
            self.discarded += 1;
        }
        result.ok().flatten()
    }

    fn process(
        &mut self,
        from: SocketAddr,
        bytes: &[u8],
        now: Instant,
        unix: u64,
    ) -> Result<Option<Vec<u8>>, ()> {
        match proto::parse(bytes).map_err(|_| ())? {
            Packet::Init {
                sender_index,
                noise,
            } => {
                let incoming = crypto::read_init(&self.keys, &self.network, noise, sender_index)
                    .map_err(|_| ())?;
                if incoming.remote_static != self.agent_key {
                    return Err(());
                }
                if crypto::check_timestamp(incoming.timestamp, unix, self.last_init).is_err() {
                    return self
                        .cached
                        .as_ref()
                        .filter(|(request, _)| request == bytes)
                        .map(|(_, response)| Some(response.clone()))
                        .ok_or(());
                }
                let mut index = crypto::random_u32();
                while [&self.current, &self.previous, &self.next]
                    .iter()
                    .any(|slot| slot.as_ref().is_some_and(|s| s.local_index == index))
                {
                    index = crypto::random_u32();
                }
                let timestamp = incoming.timestamp;
                let (session, noise) = incoming.accept(index, now).map_err(|_| ())?;
                let response = proto::encode_resp(index, sender_index, &noise).map_err(|_| ())?;
                self.next = Some(session);
                self.last_init = Some(timestamp);
                self.cached = Some((bytes.to_vec(), response.clone()));
                // Fijar solo tras autenticar la clave y la frescura, nunca por un candidato externo.
                self.endpoint = Some(from);
                Ok(Some(response))
            }
            Packet::Data {
                receiver_index,
                counter,
                ciphertext,
            } => {
                let pending = self
                    .next
                    .as_ref()
                    .is_some_and(|s| s.local_index == receiver_index);
                let session = [&mut self.next, &mut self.current, &mut self.previous]
                    .into_iter()
                    .filter_map(Option::as_mut)
                    .find(|s| s.local_index == receiver_index)
                    .ok_or(())?;
                let plain = session.decrypt(counter, ciphertext, now).map_err(|_| ())?;
                if pending {
                    self.previous = self.current.take();
                    self.current = self.next.take();
                }
                if plain.is_empty() {
                    return Ok(None);
                }
                let reply = echo_reply(&plain, &self.addresses).ok_or(())?;
                let datagram = self
                    .current
                    .as_mut()
                    .ok_or(())?
                    .encrypt(&reply, now)
                    .map_err(|_| ())?;
                self.last_send = now;
                self.echo_replies += 1;
                Ok(Some(datagram))
            }
            Packet::Resp { .. } => Err(()),
        }
    }

    pub fn keepalive(&mut self, now: Instant) -> Option<(SocketAddr, Vec<u8>)> {
        if now.saturating_duration_since(self.last_send) < Duration::from_secs(15) {
            return None;
        }
        let bytes = self.current.as_mut()?.encrypt(&[], now).ok()?;
        self.last_send = now;
        Some((self.endpoint?, bytes))
    }
}

fn checksum(bytes: &[u8], initial: u32) -> u16 {
    let mut sum = initial;
    for pair in bytes.chunks(2) {
        sum += u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn icmp6_checksum(packet: &[u8]) -> u16 {
    let mut sum = 58 + (packet.len() - 40) as u32;
    for pair in packet[8..40].as_chunks::<2>().0 {
        sum += u32::from(u16::from_be_bytes([pair[0], pair[1]]));
    }
    checksum(&packet[40..], sum)
}

fn echo_reply(packet: &[u8], addresses: &Addresses) -> Option<Vec<u8>> {
    let mut reply;
    match packet.first()? >> 4 {
        4 => {
            // Sin opciones ni fragmentos: no se reensambla tráfico en este par de pruebas.
            if packet.len() < 28
                || packet[0] != 0x45
                || usize::from(u16::from_be_bytes([packet[2], packet[3]])) != packet.len()
                || packet[9] != 1
                || u16::from_be_bytes([packet[6], packet[7]]) & 0xbfff != 0
                || packet[12..16] != addresses.agent.octets()
                || packet[16..20] != addresses.own.octets()
                || packet[20..22] != [8, 0]
                || checksum(&packet[..20], 0) != 0
                || checksum(&packet[20..], 0) != 0
            {
                return None;
            }
            reply = packet.to_vec();
            reply[12..16].copy_from_slice(&addresses.own.octets());
            reply[16..20].copy_from_slice(&addresses.agent.octets());
            reply[8] = 64;
            reply[20] = 0;
            reply[22..24].fill(0);
            let icmp = checksum(&reply[20..], 0);
            reply[22..24].copy_from_slice(&icmp.to_be_bytes());
            reply[10..12].fill(0);
            let ip = checksum(&reply[..20], 0);
            reply[10..12].copy_from_slice(&ip.to_be_bytes());
        }
        6 => {
            if packet.len() < 48
                || usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 != packet.len()
                || packet[6] != 58
                || packet[8..24] != addresses.agent6?.octets()
                || packet[24..40] != addresses.own6?.octets()
                || packet[40..42] != [128, 0]
                || icmp6_checksum(packet) != 0
            {
                return None;
            }
            reply = packet.to_vec();
            reply[8..24].copy_from_slice(&addresses.own6?.octets());
            reply[24..40].copy_from_slice(&addresses.agent6?.octets());
            reply[7] = 64;
            reply[40] = 129;
            reply[42..44].fill(0);
            let icmp = icmp6_checksum(&reply);
            reply[42..44].copy_from_slice(&icmp.to_be_bytes());
        }
        _ => return None,
    }
    Some(reply)
}

#[cfg(test)]
mod tests;
