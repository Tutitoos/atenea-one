use atenea_coordinator::{Coordinator, config::Config, router, stun};
use std::{error::Error, sync::Arc};
use tokio::{net::TcpListener, sync::watch};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::load()?;
    if std::env::args().any(|arg| arg == "--print-relay-key") {
        let key = config
            .relay_signing_key()?
            .ok_or("RELAY_SIGNING_KEY_FILE no configurada")?;
        println!("{}", hex::encode(key.verifying_key().to_bytes()));
        return Ok(());
    }
    let state = Coordinator::new(config.clone())?;
    let udp = stun::bind(&config).await?;
    let http = TcpListener::bind(&config.listen).await?;
    let (stop, rx) = watch::channel(false);
    let sweeper = tokio::spawn(Arc::clone(&state).run_sweeper(rx.clone()));
    let udp_task = tokio::spawn(stun::serve(udp, config, rx));
    tracing::info!("coordinator preparado");
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let result = axum::serve(
        http,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await;
    let _ = stop.send(true);
    sweeper.await?;
    udp_task.await??;
    result?;
    Ok(())
}
