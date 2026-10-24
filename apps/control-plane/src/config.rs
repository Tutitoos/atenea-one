use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub database_url: String,
    pub listen: String,
    pub public_url: String,
    pub oidc_issuer: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub oidc_redirect_url: String,
    pub issuer_key_file: String,
    pub config_validity_secs: u64,
    pub dev_insecure_cookies: bool,
    pub enroll_per_minute: u32,
    pub application_role: Option<String>,
    pub web_dir: Option<std::path::PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            listen: "127.0.0.1:8080".into(),
            public_url: "http://127.0.0.1:8080".into(),
            oidc_issuer: String::new(),
            oidc_client_id: String::new(),
            oidc_client_secret: String::new(),
            oidc_redirect_url: String::new(),
            issuer_key_file: String::new(),
            config_validity_secs: 300,
            dev_insecure_cookies: false,
            enroll_per_minute: 10,
            application_role: None,
            web_dir: None,
        }
    }
}
impl Config {
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let mut f = Figment::from(Serialized::defaults(Self::default()));
        if let Ok(path) = std::env::var("ATENEA_CP_CONFIG") {
            f = f.merge(Toml::file(path));
        }
        f.merge(Env::prefixed("ATENEA_CP_"))
            .extract()
            .map_err(Box::new)
    }
}
