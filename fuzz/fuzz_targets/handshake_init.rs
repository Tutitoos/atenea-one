//! Envoltura v2, MAC1/MAC2, orden pre-DH y respuestas de cookie arbitrarias.
#![no_main]
use atenea_crypto::{StaticKeypair, read_init, cookie};
use atenea_protocol::{self as proto, Packet};
use std::time::Instant;
use libfuzzer_sys::fuzz_target;

const NET: [u8; 16] = [7; 16];

fuzz_target!(|data: &[u8]| {
    let now = Instant::now();
    let local = StaticKeypair::from_private(&[0x42; 32]);
    let source = cookie::Source::direct("192.0.2.1:51820".parse().unwrap()).unwrap();
    let secrets = cookie::CookieSecrets::new(now).unwrap();
    let mut load = cookie::LoadWindow::new(now, cookie::DEFAULT_THRESHOLD);
    let decision = cookie::pre_dh(data, &local.public, &source, &secrets, &mut load, now);
    if let Ok(Packet::Init { sender_index, noise, mac1, .. }) = proto::parse(data) {
        if decision == cookie::PreDh::Proceed {
            let _ = read_init(&local, &NET, noise, sender_index);
        }
        if cookie::verify_mac1(data, &local.public) {
            let value = secrets.cookie(&source);
            let reply = cookie::seal_reply(sender_index, &local.public, mac1, &value).unwrap();
            assert_eq!(*cookie::open_reply(&reply, sender_index, &local.public, mac1).unwrap(), *value);
        }
    }
    let _ = cookie::open_reply(data, 0x01020304, &local.public, &[0;16]);
});
