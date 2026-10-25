use super::*;

const NETWORK: [u8; 16] = [7; 16];
const UNIX: u64 = 1_700_000_000;

fn addresses() -> Addresses {
    Addresses {
        own: "10.77.0.1".parse().unwrap(),
        agent: "10.77.0.2".parse().unwrap(),
        own6: Some("fd77::1".parse().unwrap()),
        agent6: Some("fd77::2".parse().unwrap()),
    }
}

fn endpoint() -> SocketAddr {
    "127.0.0.1:51820".parse().unwrap()
}

fn fixture() -> (Peer, StaticKeypair, Instant) {
    let keys = StaticKeypair::from_private([1; 32]);
    let agent = StaticKeypair::from_private([2; 32]);
    let now = Instant::now();
    (
        Peer::new(keys, agent.public, NETWORK, addresses(), now),
        agent,
        now,
    )
}

fn connect(peer: &mut Peer, agent: &StaticKeypair, now: Instant, unix: u64, index: u32) -> Session {
    let (pending, noise) =
        crypto::initiate(agent, &peer.keys.public, &NETWORK, index, unix).unwrap();
    let request = proto::encode_init(index, &noise).unwrap();
    let response = peer.receive(endpoint(), &request, now, unix).unwrap();
    assert_eq!(
        peer.receive(endpoint(), &request, now, unix),
        Some(response.clone())
    );
    let Packet::Resp {
        sender_index,
        receiver_index,
        noise,
    } = proto::parse(&response).unwrap()
    else {
        panic!("respuesta esperada");
    };
    assert_eq!(receiver_index, index);
    let mut session = pending.finish(noise, sender_index, now).unwrap();
    assert!(!peer.next.as_ref().unwrap().is_confirmed());
    let confirm = session.encrypt(&[], now).unwrap();
    assert!(peer.receive(endpoint(), &confirm, now, unix).is_none());
    assert!(peer.current.as_ref().unwrap().is_confirmed());
    session
}

fn request(v6: bool) -> Vec<u8> {
    let addresses = addresses();
    let mut packet = if v6 {
        let mut p = vec![0; 40];
        p[0] = 0x60;
        p[4..6].copy_from_slice(&13u16.to_be_bytes());
        p[6] = 58;
        p[7] = 42;
        p[8..24].copy_from_slice(&addresses.agent6.unwrap().octets());
        p[24..40].copy_from_slice(&addresses.own6.unwrap().octets());
        p
    } else {
        let mut p = vec![0; 20];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&33u16.to_be_bytes());
        p[6] = 0x40;
        p[8] = 42;
        p[9] = 1;
        p[12..16].copy_from_slice(&addresses.agent.octets());
        p[16..20].copy_from_slice(&addresses.own.octets());
        let c = checksum(&p, 0);
        p[10..12].copy_from_slice(&c.to_be_bytes());
        p
    };
    let offset = packet.len();
    packet.extend_from_slice(&[
        if v6 { 128 } else { 8 },
        0,
        0,
        0,
        0x12,
        0x34,
        0,
        1,
        1,
        2,
        3,
        4,
        5,
    ]);
    let c = if v6 {
        icmp6_checksum(&packet)
    } else {
        checksum(&packet[offset..], 0)
    };
    packet[offset + 2..offset + 4].copy_from_slice(&c.to_be_bytes());
    packet
}

fn decrypt(session: &mut Session, bytes: &[u8], now: Instant) -> Vec<u8> {
    let Packet::Data {
        receiver_index,
        counter,
        ciphertext,
    } = proto::parse(bytes).unwrap()
    else {
        panic!("datos esperados");
    };
    assert_eq!(receiver_index, session.local_index);
    session.decrypt(counter, ciphertext, now).unwrap()
}

// Verificador independiente: suma en complemento a uno debe terminar en 0xffff.
fn assert_checksum(bytes: &[u8]) {
    let mut total: u64 = bytes
        .chunks(2)
        .map(|pair| u64::from(pair[0]) * 256 + u64::from(*pair.get(1).unwrap_or(&0)))
        .sum();
    while total > 0xffff {
        total = (total & 0xffff) + (total >> 16);
    }
    assert_eq!(total, 0xffff);
}

#[test]
fn encrypted_ipv4_ipv6_echo_checksums_and_replay() {
    let (mut peer, agent, now) = fixture();
    let mut session = connect(&mut peer, &agent, now, UNIX, 9);
    for v6 in [false, true] {
        let request = request(v6);
        let bytes = session.encrypt(&request, now).unwrap();
        let response = peer.receive(endpoint(), &bytes, now, UNIX).unwrap();
        assert!(peer.receive(endpoint(), &bytes, now, UNIX).is_none());
        let reply = decrypt(&mut session, &response, now);
        let offset = if v6 { 40 } else { 20 };
        assert_eq!(reply[offset], if v6 { 129 } else { 0 });
        assert_eq!(&reply[offset + 4..], &request[offset + 4..]);
        if v6 {
            assert_eq!(&reply[8..24], &request[24..40]);
            assert_eq!(&reply[24..40], &request[8..24]);
            let mut pseudo = reply[8..40].to_vec();
            pseudo.extend_from_slice(&((reply.len() - 40) as u32).to_be_bytes());
            pseudo.extend_from_slice(&[0, 0, 0, 58]);
            pseudo.extend_from_slice(&reply[40..]);
            assert_checksum(&pseudo);
        } else {
            assert_eq!(&reply[12..16], &request[16..20]);
            assert_eq!(&reply[16..20], &request[12..16]);
            assert_checksum(&reply[..20]);
            assert_checksum(&reply[20..]);
        }
    }
    assert_eq!(peer.echo_replies, 2);
    assert_eq!(peer.discarded, 2);
}

#[test]
fn malformed_packets_and_unauthorized_sources_are_discarded() {
    let (mut peer, agent, now) = fixture();
    let mut session = connect(&mut peer, &agent, now, UNIX, 9);
    for v6 in [false, true] {
        let packet = request(v6);
        for len in 0..packet.len() {
            let bytes = session.encrypt(&packet[..len], now).unwrap();
            assert!(peer.receive(endpoint(), &bytes, now, UNIX).is_none());
        }
        for index in [
            if v6 { 8 } else { 12 },
            if v6 { 24 } else { 16 },
            if v6 { 42 } else { 22 },
        ] {
            let mut bad = packet.clone();
            bad[index] ^= 1;
            if index != if v6 { 42 } else { 22 } {
                if v6 {
                    bad[42..44].fill(0);
                    let c = icmp6_checksum(&bad);
                    bad[42..44].copy_from_slice(&c.to_be_bytes());
                } else {
                    bad[10..12].fill(0);
                    let c = checksum(&bad[..20], 0);
                    bad[10..12].copy_from_slice(&c.to_be_bytes());
                }
            }
            let bytes = session.encrypt(&bad, now).unwrap();
            assert!(peer.receive(endpoint(), &bytes, now, UNIX).is_none());
        }
    }
    for bytes in [&[][..], &[1, 3][..], &[1, 99][..], &[2, 1][..]] {
        assert!(peer.receive(endpoint(), bytes, now, UNIX).is_none());
    }
    let bytes = session.encrypt(&request(false), now).unwrap();
    assert!(
        peer.receive("127.0.0.1:9999".parse().unwrap(), &bytes, now, UNIX)
            .is_none()
    );
    let mut tampered = bytes.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(peer.receive(endpoint(), &tampered, now, UNIX).is_none());
    assert!(peer.receive(endpoint(), &bytes, now, UNIX).is_some());
    assert_eq!(peer.echo_replies, 1);
}

#[test]
fn wrong_key_network_timestamp_do_not_pin_endpoint() {
    let (mut peer, agent, now) = fixture();
    let stranger = StaticKeypair::from_private([3; 32]);
    for (key, network, unix) in [
        (&stranger, NETWORK, UNIX),
        (&agent, [8; 16], UNIX),
        (&agent, NETWORK, UNIX - 31),
    ] {
        let (_, noise) = crypto::initiate(key, &peer.keys.public, &network, 5, unix).unwrap();
        assert!(
            peer.receive(
                endpoint(),
                &proto::encode_init(5, &noise).unwrap(),
                now,
                UNIX
            )
            .is_none()
        );
        assert!(peer.endpoint.is_none());
    }
    connect(&mut peer, &agent, now, UNIX, 9);
}

#[test]
fn rekey_promotes_only_after_confirmation_and_preserves_old_session() {
    let (mut peer, agent, now) = fixture();
    let mut old = connect(&mut peer, &agent, now, UNIX, 9);
    let old_index = peer.current.as_ref().unwrap().local_index;
    let later = now + Duration::from_secs(121);
    let mut new = connect(&mut peer, &agent, later, UNIX + 121, 10);
    assert_eq!(peer.previous.as_ref().unwrap().local_index, old_index);
    let bytes = old.encrypt(&request(false), later).unwrap();
    let response = peer.receive(endpoint(), &bytes, later, UNIX + 121).unwrap();
    assert_eq!(decrypt(&mut new, &response, later)[20], 0);
    let (to, keepalive) = peer.keepalive(later + Duration::from_secs(15)).unwrap();
    assert_eq!(to, endpoint());
    assert!(decrypt(&mut new, &keepalive, later + Duration::from_secs(15)).is_empty());
    let expired = later + Duration::from_secs(180);
    assert!(peer.keepalive(expired).is_none());
}
