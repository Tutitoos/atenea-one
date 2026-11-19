#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Agente VPN: dueño de la TUN y de las claves del dispositivo (fase 2, configuración de laboratorio).

mod admin;
mod build_info;
mod config;
mod coord;
mod engine;
mod ipc;
mod names;
mod net;
mod netconf;
mod netwatch;
mod packet;
mod platform;
mod relay;
mod run;
mod server;
mod services;
mod state;
mod sync;
#[cfg(all(test, unix))]
mod testutil;
mod versions;

fn main() -> std::process::ExitCode {
    if build_info::print_version("atenea-agent") {
        return std::process::ExitCode::SUCCESS;
    }
    run::main()
}
