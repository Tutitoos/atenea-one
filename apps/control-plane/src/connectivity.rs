// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-018: la conectividad que anuncia el código de unión (host STUN y SHA-256 de la CA del relay) y la CA
//! pública que se sirve en `GET /api/v1/relay-ca`. Sin configuración nada de esto existe y el servidor se
//! comporta como la v0.1.8.
use crate::{AppState, api::ApiError, config::Config};
use axum::{extract::State, http::header, response::IntoResponse};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
const END: &str = "-----END CERTIFICATE-----";

/// La CA del relay: el PEM canónico que se sirve y el SHA-256 de su DER, que viaja en el código de unión.
#[derive(Clone, Debug)]
pub struct RelayCa {
    pub pem: String,
    pub sha256: [u8; 32],
}

impl RelayCa {
    /// Exige **un único** certificado PEM. Lo que se sirve se vuelve a emitir en forma canónica a partir del DER,
    /// de modo que el PEM publicado y el hash del código describen exactamente los mismos bytes.
    ///
    /// shortcut: solo se comprueba que el DER sea una secuencia íntegra, no que sea un X.509 válido (el cliente
    /// lo valida al cargarlo como raíz); subir si el control plane llegara a usar la CA para algo más que anunciarla.
    pub fn from_pem(text: &str) -> Result<Self, &'static str> {
        if text.matches(BEGIN).count() != 1 || text.matches(END).count() != 1 {
            return Err("la CA debe contener exactamente un certificado PEM");
        }
        let (_, rest) = text
            .split_once(BEGIN)
            .ok_or("la CA debe contener exactamente un certificado PEM")?;
        let (body, _) = rest
            .split_once(END)
            .ok_or("la CA debe contener exactamente un certificado PEM")?;
        let body: String = body.split_whitespace().collect();
        let der = STANDARD
            .decode(body)
            .map_err(|_| "la CA no es base64 válido")?;
        if !der_sequence(&der) {
            return Err("la CA no es un certificado DER íntegro");
        }
        let mut pem = format!("{BEGIN}\n");
        for line in STANDARD.encode(&der).as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(line).expect("base64 es ASCII"));
            pem.push('\n');
        }
        pem.push_str(END);
        pem.push('\n');
        Ok(Self {
            pem,
            sha256: Sha256::digest(&der).into(),
        })
    }
}

/// `SEQUENCE` DER con la longitud declarada igual a los bytes que hay (forma corta o larga de hasta 4 bytes).
fn der_sequence(der: &[u8]) -> bool {
    let [0x30, first, rest @ ..] = der else {
        return false;
    };
    let (len, header) = if *first < 0x80 {
        (usize::from(*first), 2)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return false;
        }
        (
            rest[..n]
                .iter()
                .fold(0usize, |a, b| (a << 8) | usize::from(*b)),
            2 + n,
        )
    };
    len > 0 && der.len() == header + len
}

/// Qué anuncia este servidor en el código de unión.
#[derive(Clone, Debug)]
pub struct Connectivity {
    /// Nombre o IPv4 sin puerto; el agente usa los puertos 3478 y 3479 de ese servidor.
    pub stun_host: String,
    /// `None` si el relay usa una CA pública: el código no lleva hash y no se sirve ninguna CA.
    pub relay_ca: Option<RelayCa>,
}

impl Connectivity {
    pub fn new(stun_host: &str, relay_ca: Option<RelayCa>) -> Result<Self, &'static str> {
        if !atenea_identity::join_code::valid_stun_host(stun_host) {
            return Err(
                "stun_host: se espera un nombre o una IPv4 sin puerto (1..=100 caracteres)",
            );
        }
        Ok(Self {
            stun_host: stun_host.to_owned(),
            relay_ca,
        })
    }

    /// Lee `stun_host` y `relay_ca_file`. `Ok(None)` = sin conectividad. Una configuración a medias o ilegible es un
    /// error: el servidor no arranca anunciando algo que no puede cumplir.
    pub fn load(config: &Config) -> Result<Option<Self>, String> {
        match (&config.stun_host, &config.relay_ca_file) {
            (None, None) => Ok(None),
            (None, Some(_)) => Err("relay_ca_file exige stun_host".into()),
            (Some(host), file) => {
                let ca = file
                    .as_ref()
                    .map(|path| {
                        std::fs::read_to_string(path)
                            .map_err(|e| format!("relay_ca_file: {e}"))
                            .and_then(|text| RelayCa::from_pem(&text).map_err(str::to_owned))
                    })
                    .transpose()?;
                Self::new(host, ca).map(Some).map_err(str::to_owned)
            }
        }
    }
}

/// `GET /api/v1/relay-ca`: la CA del relay en PEM. Pública y sin sesión: la descarga un cliente que aún no está
/// unido a la red. No es una credencial; el cliente solo la acepta si su SHA-256 coincide con el del código de unión.
pub async fn relay_ca(State(s): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let ca = s
        .connectivity
        .as_ref()
        .and_then(|c| c.relay_ca.as_ref())
        .ok_or_else(ApiError::not_found)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-pem-file"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        ca.pem.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn der() -> Vec<u8> {
        let mut d = vec![0x30, 10, 0x02, 0x01, 0x01, 0x04, 0x05];
        d.extend_from_slice(b"ca-ok");
        d
    }
    fn pem(der: &[u8]) -> String {
        format!("{BEGIN}\n{}\n{END}\n", STANDARD.encode(der))
    }

    #[test]
    fn el_pem_se_reemite_canonico_y_el_hash_es_el_del_der() {
        let a = RelayCa::from_pem(&pem(&der())).unwrap();
        assert_eq!(a.sha256, <[u8; 32]>::from(Sha256::digest(der())));
        // Texto antes y después, CRLF y líneas partidas: mismo hash y mismo PEM canónico.
        let b64 = STANDARD.encode(der());
        let sucio = format!(
            "# comentario\r\n{BEGIN}\r\n{}\r\n{}\r\n{END}\r\n\r\nresto\r\n",
            &b64[..4],
            &b64[4..]
        );
        let b = RelayCa::from_pem(&sucio).unwrap();
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(a.pem, b.pem);
        assert!(a.pem.starts_with(BEGIN) && a.pem.ends_with(&format!("{END}\n")));
        // Un DER largo se parte en líneas de 64 caracteres como mucho.
        let mut grande = vec![0x30, 0x82, 0x01, 0x00];
        grande.extend(std::iter::repeat_n(7u8, 256));
        let c = RelayCa::from_pem(&pem(&grande)).unwrap();
        assert!(c.pem.lines().all(|l| l.len() <= 64));
    }

    #[test]
    fn rechaza_lo_que_no_es_un_unico_certificado_integro() {
        let ok = pem(&der());
        for malo in [
            String::new(),
            "sin pem".to_owned(),
            format!("{ok}{ok}"),
            format!("{BEGIN}\nno es base64 !!\n{END}\n"),
            format!("{BEGIN}\n{END}\n"),
            format!("{END}\n{BEGIN}\nAAAA\n"),
            pem(&[0x31, 3, 1, 2, 3]),
            pem(&[0x30, 5, 1, 2, 3]),
            pem(&[0x30, 2, 1, 2, 3]),
            pem(&[0x30, 0]),
            pem(&[0x30, 0x80, 1, 2]),
            pem(&[0x30, 0x85, 0, 0, 0, 0, 1]),
            pem(&[0x30]),
        ] {
            assert!(RelayCa::from_pem(&malo).is_err(), "{malo:?}");
        }
    }

    #[test]
    fn el_stun_sigue_la_regla_del_cli() {
        for h in ["", "host:3478", "con espacio", "bad_host", &"a".repeat(101)] {
            assert!(Connectivity::new(h, None).is_err(), "{h:?}");
        }
        assert!(Connectivity::new("relay.example.test", None).is_ok());
        assert!(Connectivity::new("203.0.113.7", None).is_ok());
    }

    #[test]
    fn la_carga_exige_una_configuracion_coherente() {
        let dir = std::env::temp_dir().join(format!("atenea-conn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ca = dir.join("ca.pem");
        std::fs::write(&ca, pem(&der())).unwrap();
        let cfg = |stun: Option<&str>, file: Option<&std::path::Path>| Config {
            stun_host: stun.map(str::to_owned),
            relay_ca_file: file.map(std::path::Path::to_path_buf),
            ..Config::default()
        };
        // Nada configurado: sin conectividad (comportamiento de la v0.1.8).
        assert!(Connectivity::load(&cfg(None, None)).unwrap().is_none());
        // La CA sin STUN no se admite: el hash solo viaja con el bloque.
        assert!(Connectivity::load(&cfg(None, Some(&ca))).is_err());
        // Solo STUN, y STUN con CA.
        let solo = Connectivity::load(&cfg(Some("relay.example.test"), None))
            .unwrap()
            .unwrap();
        assert!(solo.relay_ca.is_none());
        let con = Connectivity::load(&cfg(Some("relay.example.test"), Some(&ca)))
            .unwrap()
            .unwrap();
        assert_eq!(
            con.relay_ca.unwrap().sha256,
            <[u8; 32]>::from(Sha256::digest(der()))
        );
        // STUN inválido, fichero inexistente o con contenido que no es un certificado: error.
        assert!(Connectivity::load(&cfg(Some("host:1"), None)).is_err());
        assert!(
            Connectivity::load(&cfg(
                Some("relay.example.test"),
                Some(&dir.join("no-existe"))
            ))
            .is_err()
        );
        std::fs::write(&ca, "no es un certificado").unwrap();
        assert!(Connectivity::load(&cfg(Some("relay.example.test"), Some(&ca))).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
