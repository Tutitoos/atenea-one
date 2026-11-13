// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::time::Duration;

/// Espera máxima por una conexión (sqlx: 30 s): con las 20 ocupadas se responde `500`.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);
/// Espera máxima por un bloqueo: un inquilino no puede aparcar las 20 conexiones en su red.
const LOCK_TIMEOUT: &str = "5s";
/// Duración máxima de una sentencia; las migraciones quedan fuera (`timeouts = false`).
const STATEMENT_TIMEOUT: &str = "30s";

/// Pool de la aplicación. Con `timeouts`, cada conexión nace con `lock_timeout` y
/// `statement_timeout`; sus fallos (y `PoolTimedOut`) llegan como `sqlx::Error` y `ApiError::from`
/// los responde como cualquier otro fallo de base de datos.
pub async fn pool(options: PgConnectOptions, timeouts: bool) -> Result<PgPool, sqlx::Error> {
    let options = if timeouts {
        options.options([
            ("lock_timeout", LOCK_TIMEOUT),
            ("statement_timeout", STATEMENT_TIMEOUT),
        ])
    } else {
        options
    };
    PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_with(options)
        .await
}

pub async fn migrate(
    pool: &PgPool,
    role: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::migrate!("./migrations").run(pool).await?;
    if let Some(role) = role {
        if role.is_empty() || !role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("Nombre de rol inválido".into());
        }
        let mut tx = pool.begin().await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)")
                .bind(role)
                .fetch_one(&mut *tx)
                .await?;
        if !exists {
            sqlx::query(&format!("CREATE ROLE \"{role}\" LOGIN"))
                .execute(&mut *tx)
                .await?;
        }
        for statement in [
            format!("GRANT USAGE ON SCHEMA public TO \"{role}\""),
            format!(
                "GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA public TO \"{role}\""
            ),
            format!("GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA public TO \"{role}\""),
            format!("REVOKE UPDATE,DELETE,TRUNCATE ON audit_events FROM \"{role}\""),
        ] {
            sqlx::query(&statement).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
