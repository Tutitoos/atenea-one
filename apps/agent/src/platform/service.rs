//! Adaptador seguro entre SCM y el bucle compartido del agente.
pub(crate) use atenea_windows::service_stop_receiver as stop_receiver;
use std::path::PathBuf;

pub fn run_service(config: PathBuf) -> Result<(), String> {
    atenea_windows::dispatch_service(config, |config| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        runtime.block_on(crate::run::run(config))
    })
}
