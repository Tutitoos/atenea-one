// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub database_url: String,
    pub listen: String,
    pub metrics_listen: String,
    pub public_url: String,
    pub oidc_issuer: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub oidc_redirect_url: String,
    pub issuer_key_file: String,
    pub config_validity_secs: u64,
    pub dev_insecure_cookies: bool,
    pub enroll_per_minute: u32,
    /// Solicitudes de unión (`pending` de origen `request`) admitidas a la vez por red.
    pub max_pending_join_requests: u32,
    /// Segundos tras los que una solicitud de unión pendiente se elimina de forma perezosa.
    pub join_request_ttl_secs: u64,
    /// Intentos de `GET /auth/login` por minuto y IP cliente.
    pub login_per_minute: u32,
    /// Peticiones firmadas por minuto y dispositivo (config, estado, rutas).
    pub device_per_minute: u32,
    /// Redes de los proxies inversos cuyo `X-Real-IP` se acepta; vacío = no se confía en cabeceras.
    pub trusted_proxies: Vec<ipnet::IpNet>,
    pub application_role: Option<String>,
    pub web_dir: Option<std::path::PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            listen: "127.0.0.1:8080".into(),
            metrics_listen: "127.0.0.1:9101".into(),
            public_url: "http://127.0.0.1:8080".into(),
            oidc_issuer: String::new(),
            oidc_client_id: String::new(),
            oidc_client_secret: String::new(),
            oidc_redirect_url: String::new(),
            issuer_key_file: String::new(),
            config_validity_secs: 300,
            dev_insecure_cookies: false,
            enroll_per_minute: 10,
            max_pending_join_requests: 20,
            join_request_ttl_secs: 7 * 86400,
            login_per_minute: 20,
            device_per_minute: 30,
            trusted_proxies: Vec::new(),
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
        for field in ["DATABASE_URL", "OIDC_CLIENT_SECRET"] {
            if let Ok(path) = std::env::var(format!("ATENEA_CP_{field}_FILE")) {
                let value = read_secret(std::path::Path::new(&path)).map_err(|_| {
                    Box::new(figment::Error::from("no se pudo leer secreto por fichero"))
                })?;
                f = f.merge(Serialized::default(&field.to_lowercase(), value));
            }
        }
        f.merge(
            Env::prefixed("ATENEA_CP_").ignore(&["DATABASE_URL_FILE", "OIDC_CLIENT_SECRET_FILE"]),
        )
        .extract()
        .map_err(Box::new)
    }
}

fn read_secret(path: &std::path::Path) -> std::io::Result<String> {
    let value = std::fs::read_to_string(path)?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "secreto vacío",
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::read_secret;

    #[test]
    fn secreto_por_fichero_conserva_espacios_y_rechaza_vacio() {
        let path = std::env::temp_dir().join(format!("atenea-secret-{}", std::process::id()));
        std::fs::write(&path, " valor \r\n").unwrap();
        assert_eq!(read_secret(&path).unwrap(), " valor ");
        std::fs::write(&path, "\n").unwrap();
        assert!(read_secret(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(read_secret(&path).is_err());
    }
}

#[cfg(test)]
mod proxies {
    use super::Config;
    use figment::{
        Figment,
        providers::{Format, Serialized, Toml},
    };
    #[test]
    fn trusted_proxies_se_leen_como_cidr_y_por_defecto_vacios() {
        let base = || Figment::from(Serialized::defaults(Config::default()));
        assert!(
            base()
                .extract::<Config>()
                .unwrap()
                .trusted_proxies
                .is_empty()
        );
        let c: Config = base()
            .merge(Toml::string("trusted_proxies=['172.18.0.0/16','fd00::/8']"))
            .extract()
            .unwrap();
        assert_eq!(c.trusted_proxies.len(), 2);
        assert!(
            base()
                .merge(Toml::string("trusted_proxies=['no-cidr']"))
                .extract::<Config>()
                .is_err()
        );
    }
}
