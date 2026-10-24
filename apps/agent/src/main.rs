#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
//! Agente VPN: dueño de la TUN y de las claves del dispositivo (fase 2, configuración de laboratorio).

mod config;
mod engine;
mod packet;
#[cfg(target_os = "linux")]
mod run;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    run::main()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "atenea-agent solo soporta Linux en esta fase (ver docs/architecture, fase 8 para otros SO)"
    );
    std::process::ExitCode::FAILURE
}
