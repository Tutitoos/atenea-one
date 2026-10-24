pub mod api;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod devices;
pub mod issue;
pub mod networks;
mod openapi;
pub mod orgs;
pub mod panel;
pub mod policies;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Instant,
};
use uuid::Uuid;
pub type NonceCache = Arc<Mutex<HashMap<(Uuid, [u8; 16]), u64>>>;
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: config::Config,
    pub issuer: atenea_identity::SigningKey,
    pub nonces: NonceCache,
    pub rate: Arc<Mutex<HashMap<IpAddr, (f64, Instant)>>>,
    pub panel: Arc<panel::Live>,
}
impl AppState {
    pub fn new(
        pool: sqlx::PgPool,
        config: config::Config,
        issuer: atenea_identity::SigningKey,
    ) -> Self {
        Self {
            pool,
            config,
            issuer,
            nonces: Default::default(),
            rate: Default::default(),
            panel: Arc::new(panel::Live::default()),
        }
    }
}
pub use api::{openapi, router};
