// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Adaptador seguro entre SCM y el bucle compartido del agente.
pub(crate) use atenea_windows::service_stop_receiver as stop_receiver;
use std::path::{Path, PathBuf};

/// Conserva el diagnóstico desde el arranque hasta la desconexión de SCM. El registro vive junto a la
/// configuración (`agent.log` en su directorio): cada servicio escribe en el suyo, porque un fichero
/// compartido lo abriría el primer servicio con acceso exclusivo de escritura y el segundo no arrancaría.
pub(crate) fn init_logging(level: tracing::Level, config: &Path) -> Result<(), String> {
    let dir = config
        .parent()
        .filter(|dir| config.is_absolute() && dir.is_absolute())
        .ok_or("el servicio exige una ruta de configuración absoluta")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("directorio de diagnóstico: {e}"))?;
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
