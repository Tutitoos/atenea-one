// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Solo para las pruebas de aceptación (`tests/windows/network-checks.ps1`): emite un código de unión sintético
//! para un control plane falso. Uso: `make_join_code <url> <network_id_hex32> <nombre> <token>`.
use atenea_crypto::hex;
use atenea_identity::join_code::JoinCode;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [url, network, name, token] = args.as_slice() else {
        eprintln!("uso: make_join_code <url> <network_id_hex32> <nombre> <token>");
        std::process::exit(2);
    };
    let code = JoinCode {
        url: url.clone(),
        network_id: hex::decode::<16>(network).expect("network_id: 32 dígitos hex"),
        root_anchor: [0x11; 32],
        name: name.clone(),
        token: Some(token.clone()),
        connectivity: None,
    };
    println!("{}", code.encode().expect("código válido"));
}
