//! Backend TUN L3 de Linux (IP puro, sin cabecera PI).
//!
//! Las demás plataformas todavía no tienen backend y no exportan `Tun`.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::Tun;
