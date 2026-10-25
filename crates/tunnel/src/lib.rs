//! Backends TUN L3: Linux y macOS; contrato IP puro.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::Tun;
#[cfg(target_os = "macos")]
pub use macos::Tun;
