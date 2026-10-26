use super::*;
use crate::{NOISE_PARAMS, StaticKeypair, prologue};
include!("vectors.rs");
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}
fn arr<const N: usize>(s: &str) -> [u8; N] {
    hex(s).try_into().unwrap()
}
fn source() -> Source {
    Source::direct("192.0.2.1:51820".parse().unwrap()).unwrap()
}
fn secrets(now: Instant) -> CookieSecrets {
    CookieSecrets {
        current: Zeroizing::new(std::array::from_fn(|i| i as u8)),
        previous: None,
        since: now,
    }
}
fn packet(s: &CookieSecrets, src: &Source) -> Vec<u8> {
    encode_init(
        0x01020304,
        &(0..108).collect::<Vec<u8>>(),
        &arr(PUBLIC),
        Some(&s.cookie(src)),
    )
    .unwrap()
}

#[test]
fn independent_vectors_1_to_4() {
    let now = Instant::now();
    let s = secrets(now);
    let public = StaticKeypair::from_private(&[0x42; 32]).public;
    assert_eq!(public, arr::<32>(PUBLIC));
    assert_eq!(mac1_key(&public), arr::<32>(K_MAC1));
    let noise: Vec<u8> = (0..108).collect();
    let zero = encode_init(0x01020304, &noise, &public, None).unwrap();
    assert_eq!(&zero[..114], hex(PREFIX));
    assert_eq!(&zero[114..130], hex(MAC1));
    assert_eq!(zero, hex(INIT_ZERO));
    for (src, src_hex, cookie, m2, init) in [
        (source(), IPV4_SOURCE, IPV4_COOKIE, IPV4_MAC2, IPV4_INIT),
        (
            Source::direct("[2001:db8::1]:51820".parse().unwrap()).unwrap(),
            IPV6_SOURCE,
            IPV6_COOKIE,
            IPV6_MAC2,
            IPV6_INIT,
        ),
        (
            Source::relay(&[0x11; 16], &[7; 16], &[0x22; 16], &[0x33; 16]),
            RELAY_SOURCE,
            RELAY_COOKIE,
            RELAY_MAC2,
            RELAY_INIT,
        ),
    ] {
        assert_eq!(src.as_bytes(), hex(src_hex));
        assert_eq!(*s.cookie(&src), arr::<16>(cookie));
        let p = packet(&s, &src);
        assert_eq!(&p[130..], hex(m2));
        assert_eq!(p, hex(init));
        assert!(s.verify_mac2(&p, &src));
    }
    assert_eq!(reply_key(&public, &arr(MAC1)), arr::<32>(K_REPLY));
    let reply = seal_with_nonce(
        0x01020304,
        &public,
        &arr(MAC1),
        &arr(IPV4_COOKIE),
        &arr(NONCE),
    )
    .unwrap();
    assert_eq!(reply.as_slice(), hex(COOKIE_REPLY));
    assert_eq!(&reply[30..], hex(ENCRYPTED_COOKIE));
    let mut aad = reply[..30].to_vec();
    aad.extend_from_slice(&arr::<16>(MAC1));
    assert_eq!(aad, hex(AAD));
    assert_eq!(
        *open_reply(&reply, 0x01020304, &public, &arr(MAC1)).unwrap(),
        arr::<16>(IPV4_COOKIE)
    );
}
#[test]
fn every_cookie_reply_byte_and_context_is_authenticated() {
    let reply = arr::<62>(COOKIE_REPLY);
    let public = arr(PUBLIC);
    let mac = arr(MAC1);
    for i in 0..reply.len() {
        let mut bad = reply;
        bad[i] ^= 1;
        assert!(
            open_reply(&bad, 0x01020304, &public, &mac).is_err(),
            "byte {i}"
        );
    }
    for i in 0..16 {
        let mut bad = mac;
        bad[i] ^= 1;
        assert!(open_reply(&reply, 0x01020304, &public, &bad).is_err());
    }
    for i in 0..32 {
        let mut bad = public;
        bad[i] ^= 1;
        assert!(open_reply(&reply, 0x01020304, &bad, &mac).is_err());
    }
    assert_ne!(
        seal_reply(1, &public, &mac, &arr(IPV4_COOKIE)).unwrap(),
        seal_reply(1, &public, &mac, &arr(IPV4_COOKIE)).unwrap()
    );
}
#[test]
fn canonical_sources_and_context_isolation() {
    assert_eq!(
        source(),
        Source::direct("[::ffff:192.0.2.1]:51820".parse().unwrap()).unwrap()
    );
    for endpoint in ["[fe80::1]:5", "[2001:db8::1%3]:5"] {
        assert!(Source::direct(endpoint.parse().unwrap()).is_err());
    }
    let now = Instant::now();
    let s = secrets(now);
    let p = packet(&s, &source());
    for addr in ["192.0.2.1:51821", "192.0.2.2:51820", "[2001:db8::1]:51820"] {
        assert!(!s.verify_mac2(&p, &Source::direct(addr.parse().unwrap()).unwrap()));
    }
    let relay = Source::relay(&[0x11; 16], &[7; 16], &[0x22; 16], &[0x33; 16]);
    let p = packet(&s, &relay);
    for field in 0..4 {
        let mut ids = [[0x11; 16], [7; 16], [0x22; 16], [0x33; 16]];
        ids[field][0] ^= 1;
        assert!(!s.verify_mac2(&p, &Source::relay(&ids[0], &ids[1], &ids[2], &ids[3])));
    }
    assert!(!s.verify_mac2(&p, &source()));
}
#[test]
fn rotation_boundaries_previous_expiry_and_skipped_periods() {
    let now = Instant::now();
    let mut s = secrets(now);
    let original = packet(&s, &source());
    s.rotate_with(now + Duration::from_secs(119), |_| panic!("sin rotación"))
        .unwrap();
    assert!(s.verify_mac2(&original, &source()));
    s.rotate_with(now + Duration::from_secs(120), |v| {
        v.fill(9);
        Ok(())
    })
    .unwrap();
    assert!(s.verify_mac2(&original, &source()));
    let current = packet(&s, &source());
    s.rotate_with(now + Duration::from_secs(240), |v| {
        v.fill(10);
        Ok(())
    })
    .unwrap();
    assert!(!s.verify_mac2(&original, &source()));
    assert!(s.verify_mac2(&current, &source()));
    s.rotate_with(now + Duration::from_secs(480), |v| {
        v.fill(11);
        Ok(())
    })
    .unwrap();
    assert!(!s.verify_mac2(&current, &source()));
    assert!(s.previous.is_none());
    let restarted = CookieSecrets::new(now).unwrap();
    assert!(!restarted.verify_mac2(&original, &source()));
}
#[test]
fn validation_order_load_threshold_and_window_boundary() {
    let now = Instant::now();
    let s = secrets(now);
    let src = source();
    let public = arr(PUBLIC);
    let valid = hex(INIT_ZERO);
    let mut load = LoadWindow::new(now, NonZeroU32::new(2).unwrap());
    for i in 0..130 {
        let mut bad = valid.clone();
        bad[i] ^= 1;
        assert_eq!(pre_dh(&bad, &public, &src, &s, &mut load, now), PreDh::Drop);
    }
    assert_eq!(load.count, 0); // Ningún descarte llega siquiera al contador, mucho menos a DH.
    assert_eq!(
        pre_dh(&valid, &public, &src, &s, &mut load, now),
        PreDh::Proceed
    );
    assert_eq!(
        pre_dh(&valid, &public, &src, &s, &mut load, now),
        PreDh::Proceed
    );
    assert_eq!(
        pre_dh(
            &valid,
            &public,
            &src,
            &s,
            &mut load,
            now + Duration::from_millis(999)
        ),
        PreDh::Challenge
    );
    assert_eq!(
        pre_dh(&packet(&s, &src), &public, &src, &s, &mut load, now),
        PreDh::Proceed
    );
    assert_eq!(load.count, 4);
    assert_eq!(
        pre_dh(
            &valid,
            &public,
            &src,
            &s,
            &mut load,
            now + Duration::from_secs(1)
        ),
        PreDh::Proceed
    );
    assert_eq!(load.count, 1);
    load.count = u64::MAX;
    assert_eq!(
        pre_dh(
            &valid,
            &public,
            &src,
            &s,
            &mut load,
            now + Duration::from_secs(1)
        ),
        PreDh::Challenge
    );
    assert_eq!(load.count, u64::MAX);
}
#[test]
fn cache_never_shares_key_route_or_extends_ttl() {
    let now = Instant::now();
    let public = arr(PUBLIC);
    let src = source();
    let cache = CachedCookie::new(public, src, Zeroizing::new(arr(IPV4_COOKIE)), now);
    assert!(
        cache
            .get(&public, &src, now + Duration::from_secs(119))
            .is_some()
    );
    assert!(cache.get(&public, &src, now + COOKIE_TTL).is_none());
    assert!(cache.get(&[0; 32], &src, now).is_none());
    assert!(
        cache
            .get(
                &public,
                &Source::direct("192.0.2.1:51821".parse().unwrap()).unwrap(),
                now
            )
            .is_none()
    );
}
#[test]
fn independent_real_noise_vector_and_bidirectional_transport() {
    let si = StaticKeypair::from_private(&[0x41; 32]);
    let sr = StaticKeypair::from_private(&[0x42; 32]);
    let p = prologue(&[7; 16]);
    let mut i = snow::Builder::new(NOISE_PARAMS.parse().unwrap())
        .local_private_key(si.private_bytes())
        .unwrap()
        .remote_public_key(&sr.public)
        .unwrap()
        .prologue(&p)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&[0x43; 32])
        .build_initiator()
        .unwrap();
    let mut r = snow::Builder::new(NOISE_PARAMS.parse().unwrap())
        .local_private_key(sr.private_bytes())
        .unwrap()
        .prologue(&p)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&[0x44; 32])
        .build_responder()
        .unwrap();
    let mut payload = [0; 12];
    payload[..4].copy_from_slice(&0x01020304u32.to_be_bytes());
    payload[4..].copy_from_slice(&1700000000u64.to_be_bytes());
    let mut msg1 = [0; 108];
    assert_eq!(i.write_message(&payload, &mut msg1).unwrap(), 108);
    assert_eq!(msg1.as_slice(), hex(NOISE_MSG1));
    assert_eq!(
        encode_init(0x01020304, &msg1, &sr.public, None).unwrap(),
        hex(REAL_INIT)
    );
    let mut clear = [0; 64];
    assert_eq!(r.read_message(&msg1, &mut clear).unwrap(), 12);
    assert_eq!(&clear[..12], payload);
    let mut msg2 = [0; 52];
    r.write_message(&0x05060708u32.to_be_bytes(), &mut msg2)
        .unwrap();
    assert_eq!(msg2.as_slice(), hex(NOISE_MSG2));
    assert_eq!(
        proto::encode_resp(0x05060708, 0x01020304, &msg2).unwrap(),
        hex(REAL_RESP)
    );
    i.read_message(&msg2, &mut clear).unwrap();
    let i = i.into_stateless_transport_mode().unwrap();
    let r = r.into_stateless_transport_mode().unwrap();
    let mut out = [0; 64];
    let n = i.write_message(0, b"iniciador", &mut out).unwrap();
    assert_eq!(&out[..n], hex(TRANSPORT_I));
    let n = r.read_message(0, &out[..n], &mut clear).unwrap();
    assert_eq!(&clear[..n], b"iniciador");
    let n = r.write_message(0, b"respondedor", &mut out).unwrap();
    assert_eq!(&out[..n], hex(TRANSPORT_R));
    let n = i.read_message(0, &out[..n], &mut clear).unwrap();
    assert_eq!(&clear[..n], b"respondedor");
}

#[test]
fn rng_failure_never_emits_or_advances_rotation() {
    let now = Instant::now();
    let mut s = secrets(now);
    let before = packet(&s, &source());
    assert_eq!(
        seal_with_rng(1, &arr(PUBLIC), &arr(MAC1), &arr(IPV4_COOKIE), |_| Err(
            getrandom::Error::UNEXPECTED
        )),
        Err(CookieError::Random)
    );
    assert_eq!(
        s.rotate_with(now + COOKIE_TTL, |_| Err(getrandom::Error::UNEXPECTED)),
        Err(CookieError::Random)
    );
    assert_eq!(s.since, now);
    assert!(s.verify_mac2(&before, &source()));
}

#[test]
fn mac2_mutations_require_cookie_without_dh_and_rebind_recovers() {
    let now = Instant::now();
    let s = secrets(now);
    let public = arr(PUBLIC);
    let src = source();
    let p = packet(&s, &src);
    let mut load = LoadWindow::new(now, NonZeroU32::new(1).unwrap());
    assert_eq!(
        pre_dh(&p, &public, &src, &s, &mut load, now),
        PreDh::Proceed
    );
    for offset in 130..146 {
        let mut bad = p.clone();
        bad[offset] ^= 1;
        assert_eq!(
            pre_dh(&bad, &public, &src, &s, &mut load, now),
            PreDh::Challenge
        );
    }
    let rebound = Source::direct("192.0.2.1:51821".parse().unwrap()).unwrap();
    assert_eq!(
        pre_dh(&p, &public, &rebound, &s, &mut load, now),
        PreDh::Challenge
    );
    let new_cookie = packet(&s, &rebound);
    assert_eq!(
        pre_dh(&new_cookie, &public, &rebound, &s, &mut load, now),
        PreDh::Proceed
    );
}
