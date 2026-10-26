//! `parse` nunca entra en pánico y todo lo aceptado se recodifica byte a byte igual.
#![no_main]
use atenea_protocol::{Packet, data_header, encode_init, encode_resp, encode_cookie_reply, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|buf: &[u8]| {
    let re = match parse(buf) {
        Ok(Packet::Init { sender_index, noise, mac1, mac2 }) => encode_init(sender_index, noise, mac1, mac2).expect("init aceptado"),
        Ok(Packet::Resp { sender_index, receiver_index, noise }) => {
            encode_resp(sender_index, receiver_index, noise).expect("resp aceptado")
        }
        Ok(Packet::Data { receiver_index, counter, ciphertext }) => {
            let mut v = data_header(receiver_index, counter).to_vec();
            v.extend_from_slice(ciphertext);
            v
        }
        Ok(Packet::CookieReply {receiver_index, nonce, encrypted_cookie}) => encode_cookie_reply(receiver_index, nonce, encrypted_cookie).to_vec(),
        Err(_) => return,
    };
    assert_eq!(re, buf, "recodificación distinta de la entrada aceptada");
});
