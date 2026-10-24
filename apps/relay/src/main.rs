use atenea_relay::{Relay, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if std::env::args().any(|a| a == "--print-config-example") {
        println!("{}", serde_json::to_string_pretty(&Config::default())?);
        return Ok(());
    }
    let relay = Relay::bind(Config::load()?).await?;
    tracing::info!(udp=%relay.udp_addr()?,tcp=%relay.tcp_addr()?,"relay escuchando");
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
