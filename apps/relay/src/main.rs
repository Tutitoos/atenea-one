// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
#[path = "../../agent/src/build_info.rs"]
mod build_info;
use atenea_relay::{Relay, config::Config};

#[tokio::main]
async fn main() {
    if build_info::print_version("atenea-relay") {
        return;
    }
    if run().await.is_err() {
        eprintln!("Fallo del relay; compruebe configuración, ficheros y listeners");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if std::env::args().any(|a| a == "--print-config-example") {
        println!("{}", serde_json::to_string_pretty(&Config::default())?);
        return Ok(());
    }
    let relay = Relay::bind(Config::load()?).await?;
    tracing::info!("relay escuchando");
    let (tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("señal SIGTERM");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        let _ = tx.send(true);
    });
    relay.run(rx).await?;
    Ok(())
}
