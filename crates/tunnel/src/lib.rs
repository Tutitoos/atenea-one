// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Backends TUN L3: Linux y macOS; contrato IP puro.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::Tun;
#[cfg(target_os = "macos")]
pub use macos::Tun;
#[cfg(target_os = "windows")]
pub use windows::Tun;
