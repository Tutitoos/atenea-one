use sqlx::PgPool;
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
