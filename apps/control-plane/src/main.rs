// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
#[path = "../../agent/src/build_info.rs"]
mod build_info;
use atenea_control_plane::{AppState, config::Config};
#[tokio::main]
async fn main() {
    if build_info::print_version("atenea-control-plane") {
        return;
    }
    if run().await.is_err() {
        eprintln!("Fallo del control plane; compruebe configuración, base de datos y listeners");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if std::env::args().any(|a| a == "--openapi") {
        println!(
            "{}",
            serde_json::to_string_pretty(&atenea_control_plane::openapi())?
        );
        return Ok(());
    }
    let config = Config::load()?;
    if config.config_validity_secs == 0
        || config.config_validity_secs > 3600
        || config.enroll_per_minute == 0
        || config.login_per_minute == 0
        || config.device_per_minute == 0
        || config.join_request_ttl_secs == 0
        || config.join_request_ttl_secs > 31_536_000
    {
        return Err("Validez o límite de tasa inválidos".into());
    }
    if config.dev_insecure_cookies {
        tracing::warn!("Cookies sin Secure: modo de desarrollo explícito");
    }
    let metadata = std::fs::symlink_metadata(&config.issuer_key_file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !issuer_key_mode(metadata.permissions().mode()) || !metadata.is_file() {
            return Err("El fichero de emisor debe ser regular y 0400 o 0600".into());
        }
    }
    let mut seed = std::fs::read_to_string(&config.issuer_key_file)?;
    let bytes: [u8; 32] = hex::decode(seed.trim())?
        .try_into()
        .map_err(|_| "Semilla de emisor inválida")?;
    use zeroize::Zeroize;
    seed.zeroize();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;
    if std::env::args().any(|a| a == "--migrate") {
        atenea_control_plane::db::migrate(&pool, config.application_role.as_deref()).await?;
        return Ok(());
    }
    let writable:bool=sqlx::query_scalar("SELECT has_table_privilege(current_user,'audit_events','UPDATE') OR has_table_privilege(current_user,'audit_events','DELETE') OR has_table_privilege(current_user,'audit_events','TRUNCATE')").fetch_one(&pool).await?;
    if writable {
        return Err(
            "El rol de aplicación no puede modificar auditoría; use un rol restringido".into(),
        );
    }
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    let state = AppState::new(pool, config, atenea_identity::signing_key(bytes));
    let metrics_state = state.clone();
    let metrics_task = atenea_observability::serve(
        &state.config.metrics_listen,
        std::sync::Arc::new(move || {
            format!(
                "{}# TYPE atenea_websocket_connections gauge\natenea_websocket_connections {}\n",
                metrics_state.metrics.encode(),
                metrics_state.panel.connection_count()
            )
        }),
    )
    .await?;
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let sampler = tokio::spawn(atenea_control_plane::panel::sampler(state.clone(), stop_rx));
    axum::serve(
        listener,
        atenea_control_plane::router(state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    let _ = stop_tx.send(true);
    sampler.await?;
    metrics_task.abort();
    Ok(())
}

#[cfg(unix)]
fn issuer_key_mode(mode: u32) -> bool {
    matches!(mode & 0o777, 0o400 | 0o600)
}

#[cfg(all(test, unix))]
#[test]
fn permisos_clave_emisor() {
    for mode in [0o400, 0o600] {
        assert!(issuer_key_mode(mode));
    }
    for mode in [0o440, 0o640, 0o644, 0o604, 0o700] {
        assert!(!issuer_key_mode(mode));
    }
}
