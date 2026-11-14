// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Cabecera PROXY v2 del relay (`apps/relay/src/tcp.rs::proxy`, la función real, leyendo de un `&[u8]`).
//! Entrada: el primer byte decide si el origen de la conexión TCP es loopback; el resto es el flujo. Nunca
//! pánico; solo se acepta desde loopback, con firma PROXY, comando 0x21 (PROXY) y TCP4 con 12 bytes o TCP6 con
//! 36 (sin TLV, LOCAL ni UNSPEC); la IP devuelta es la del cuerpo y se consumen exactamente 16 + longitud bytes.
//! Un rechazo por origen no loopback no lee nada.
#![no_main]
use atenea_relay::proxy_v2;
use libfuzzer_sys::fuzz_target;
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::pin,
    task::{Context, Poll, Waker},
};

const SIGNATURE: &[u8; 12] = b"\r\n\r\n\0\r\nQUIT\n";

/// Un `&[u8]` nunca devuelve `Pending`: basta un sondeo con un waker vacío.
fn run<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => unreachable!("un lector en memoria no espera"),
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&flag, stream)) = data.split_first() else {
        return;
    };
    let loopback = flag & 1 == 0;
    let source: SocketAddr = if loopback { "127.0.0.1:50000" } else { "192.0.2.7:50000" }.parse().unwrap();
    let mut reader = stream;
    let result = run(proxy_v2(&mut reader, source));
    let consumed = stream.len() - reader.len();
    match result {
        Ok(ip) => {
            assert!(loopback, "PROXY aceptado desde un origen no loopback");
            assert!(stream.len() >= 16 && &stream[..12] == SIGNATURE && stream[12] == 0x21);
            let length = usize::from(u16::from_be_bytes([stream[14], stream[15]]));
            match (stream[13], ip) {
                (0x11, IpAddr::V4(v4)) => {
                    assert_eq!(length, 12);
                    assert_eq!(v4, Ipv4Addr::new(stream[16], stream[17], stream[18], stream[19]));
                }
                (0x21, IpAddr::V6(v6)) => {
                    assert_eq!(length, 36);
                    assert_eq!(v6, Ipv6Addr::from(<[u8; 16]>::try_from(&stream[16..32]).unwrap()));
                }
                other => panic!("familia y dirección incoherentes: {other:?}"),
            }
            assert_eq!(consumed, 16 + length);
        }
        Err(_) if !loopback => assert_eq!(consumed, 0, "un origen no loopback no debe leer el flujo"),
        Err(_) => {}
    }
});
