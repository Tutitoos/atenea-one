#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
//! Agente VPN: dueño de la TUN y de las claves del dispositivo (fase 2, configuración de laboratorio).

mod build_info;
mod config;
mod coord;
mod engine;
mod ipc;
mod net;
mod netconf;
mod packet;
mod platform;
mod relay;
mod run;
mod state;
mod sync;
#[cfg(all(test, unix))]
mod testutil;

fn main() -> std::process::ExitCode {
    if build_info::print_version("atenea-agent") {
        return std::process::ExitCode::SUCCESS;
    }
    run::main()
}
