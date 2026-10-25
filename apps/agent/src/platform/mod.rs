//! Integración con el host; la política de red permanece en `netconf`.
use atenea_identity::{AdvertisedRoute, DnsConfig, Prefix};
use std::net::{IpAddr, SocketAddr};

pub mod lifecycle;
#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(unix)]
pub mod local_ipc;
#[cfg(any(target_os = "macos", test))]
pub mod macos;
pub mod private_files;
pub mod tun;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub mod unsupported;
pub use lifecycle::Lifecycle;
#[cfg(target_os = "linux")]
pub use linux::Network as Platform;
#[cfg(unix)]
pub use local_ipc::LocalIpc;
#[cfg(target_os = "macos")]
pub use macos::Network as Platform;
pub use tun::TunDevice;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use unsupported::Network as Platform;

/// El identificador del enlace pertenece al backend (nombre Unix o índice/LUID Windows).
pub(crate) trait HostNetwork {
    fn supports_subnet_routing(&self) -> bool;
    fn configure_link(&self, mtu: usize, addrs: &[(IpAddr, u8)]) -> Result<(), String>;
    /// Rutas no predeterminadas de otros enlaces, para comprobar conflictos.
    fn existing_routes(&self) -> Result<Vec<Prefix>, String>;
    fn add_route(&self, prefix: &str) -> Result<(), String>;
    fn remove_route(&self, prefix: &str) -> Result<(), String>;
    fn set_dns(&self, dns: &DnsConfig) -> Result<(), String>;
    fn clear_dns(&self) -> Result<(), String>;
    fn set_snat(&self, routes: &[AdvertisedRoute], replace: bool) -> Result<(), String>;
    fn cleanup(&self) -> Result<(), String>;
    fn forwarding(&self, ipv6: bool) -> bool;
    async fn local_addrs(&self, port: u16) -> Vec<SocketAddr>;
}
