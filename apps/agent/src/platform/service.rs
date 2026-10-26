//! Adaptador seguro entre SCM y el bucle compartido del agente.
pub(crate) use atenea_windows::service_stop_receiver as stop_receiver;
use std::path::PathBuf;

/// Conserva el diagnóstico desde el arranque hasta la desconexión de SCM.
pub(crate) fn init_logging(level: tracing::Level) -> Result<(), String> {
    let root = std::env::var_os("ProgramData").ok_or("ProgramData ausente")?;
    let dir = PathBuf::from(root).join("Atenea");
    if !dir.is_absolute() {
        return Err("ProgramData debe ser una ruta absoluta".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("directorio de diagnóstico: {e}"))?;
    let file = atenea_windows::append_private(&dir.join("agent.log"))
        .map_err(|e| format!("agent.log privado: {e}"))?;
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .init();
    std::panic::set_hook(Box::new(|info| {
        // No se registra el payload: puede contener datos de configuración.
        tracing::error!(location = ?info.location(), "pánico en el servicio");
    }));
    tracing::info!(pid = std::process::id(), "inicio del proceso de servicio");
    Ok(())
}

pub fn run_service(config: PathBuf) -> Result<(), String> {
    atenea_windows::dispatch_service(config, |config| {
        tracing::info!("iniciando runtime del servicio");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        let result = runtime.block_on(crate::run::run(config));
        tracing::info!(
            success = result.is_ok(),
            "bucle del agente terminado; cerrando runtime"
        );
        crate::run::shutdown_runtime(runtime);
        tracing::info!("runtime del servicio cerrado");
        result
    })
}
