// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Relay: `parse`, `RelayToken::parse` y `FrameDecoder` nunca entran en pánico con entradas
//! arbitrarias; un token aceptado se re-codifica idéntico y un decodificador no acumula más de lo debido.
#![no_main]
use atenea_relay_core::{self as wire, FrameDecoder, MAX_FRAME, RelayToken, Message, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(message) = parse(data) {
        assert_eq!(&data[..2], &[wire::MAGIC, wire::VERSION]);
        let encoded = match message {
            Message::CookieReq { relay_id } => wire::encode_cookie_req(&relay_id),
            Message::Cookie { relay_id, cookie } => wire::encode_cookie(&relay_id,&cookie),
            Message::Alloc { relay_id,cookie,ts,token,sig } => wire::encode_alloc(&relay_id,&cookie,ts,token,&sig),
            Message::AllocOk { alloc_id,ttl_secs,peer_ready } => wire::encode_alloc_ok(alloc_id,ttl_secs,peer_ready),
            Message::AllocErr { code } => wire::encode_alloc_err(code),
            Message::Data { alloc_id, datagram } => {
                assert!(wire::valid_datagram(datagram));
                assert!(data.len() <= wire::DATA_OVERHEAD + wire::MAX_RELAYED_DATAGRAM);
                wire::encode_data(alloc_id,datagram)
            },
            Message::Ping { alloc_id,nonce } => wire::encode_ping(alloc_id,nonce,false),
            Message::Pong { alloc_id,nonce } => wire::encode_ping(alloc_id,nonce,true),
            Message::PeerState { alloc_id,ready } => wire::encode_peer_state(alloc_id,ready),
        };
        assert_eq!(encoded,data);
    }
    if let Ok(t) = RelayToken::parse(data) {
        assert_eq!(t.encode().as_slice(), data);
    }
    // El primer byte decide el tamaño de los trozos con que se alimenta el flujo TCP.
    let Some((&chunk, rest)) = data.split_first() else {
        return;
    };
    let mut d = FrameDecoder::new();
    for part in rest.chunks(usize::from(chunk) + 1) {
        d.push(part);
        loop {
            match d.next_frame() {
                Ok(Some(f)) => assert!(!f.is_empty() && f.len() <= MAX_FRAME),
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
});
