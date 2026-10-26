// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Hasta implementar el backend real, ninguna operación del host aparenta éxito.
use super::HostNetwork;
use atenea_identity::{AdvertisedRoute, DnsConfig, Prefix};
use std::net::{IpAddr, SocketAddr};

#[derive(Default)]
pub struct Binaries;
pub struct Network;
const UNSUPPORTED: &str = "plataforma sin backend todavía";
impl Network {
    pub fn new(_interface: String, _bins: Binaries) -> Self {
        Self
    }
    pub fn ensure_available() -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}
impl HostNetwork for Network {
    fn supports_subnet_routing(&self) -> bool {
        false
    }
    fn configure_link(&self, _mtu: usize, _addrs: &[(IpAddr, u8)]) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn existing_routes(&self) -> Result<Vec<Prefix>, String> {
        Err(UNSUPPORTED.into())
    }
    fn add_route(&self, _prefix: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn remove_route(&self, _prefix: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn set_dns(&self, _dns: &DnsConfig) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn clear_dns(&self) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn set_snat(&self, _routes: &[AdvertisedRoute], _replace: bool) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn cleanup(&self) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn forwarding(&self, _ipv6: bool) -> bool {
        false
    }
    async fn local_addrs(&self, _port: u16) -> Vec<SocketAddr> {
        Vec::new()
    }
}
