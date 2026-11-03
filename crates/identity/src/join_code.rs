// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Código de red v1 (`docs/protocol/join-code-v1.md`): todo lo necesario para unirse a una red,
//! en un texto copiable con suma de comprobación. Detecta erratas; NO autentica.
use crate::{NetworkId, PublicKey, hash};
use std::fmt;

const PREFIX: &str = "ATENEA1";
const DOMAIN: &[u8] = b"atenea-one/join-code/v1";
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
/// Longitud máxima del texto completo, comprobada antes de decodificar.
pub const MAX_TEXT_LEN: usize = 512;
const FLAG_TOKEN: u8 = 1;
/// Lleva el bloque de conectividad (host STUN); ADR-018.
const FLAG_CONNECTIVITY: u8 = 2;
/// Además del STUN, lleva el SHA-256 de la CA del relay (solo válido junto a `FLAG_CONNECTIVITY`).
const FLAG_RELAY_CA: u8 = 4;
const KNOWN_FLAGS: u8 = FLAG_TOKEN | FLAG_CONNECTIVITY | FLAG_RELAY_CA;

/// Cómo alcanzar el STUN y el relay de la red sin configurarlos a mano (ADR-018). Nada de esto autentica:
/// la CA no viaja en el código, solo su hash, con el que el cliente comprueba la que descargue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connectivity {
    /// Nombre o IPv4 **sin puerto**; el agente usa los puertos 3478 y 3479 de ese servidor.
    pub stun_host: String,
    /// SHA-256 del certificado (DER) de la CA del relay; `None` si el relay usa una CA pública.
    pub relay_ca_sha256: Option<[u8; 32]>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct JoinCode {
    pub url: String,
    pub network_id: NetworkId,
    pub root_anchor: PublicKey,
    pub name: String,
    pub token: Option<String>,
    pub connectivity: Option<Connectivity>,
}

/// El token no sale nunca por `Debug`.
impl fmt::Debug for JoinCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinCode")
            .field("url", &self.url)
            .field("network_id", &self.network_id)
            .field("root_anchor", &self.root_anchor)
            .field("name", &self.name)
            .field("token", &self.token.as_ref().map(|_| "<oculto>"))
            .field("connectivity", &self.connectivity)
            .finish()
    }
}

/// Errores del código de red; ninguno contiene datos del código (token incluido).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinCodeError {
    TooLong,
    BadPrefix,
    BadEncoding,
    BadChecksum,
    UnsupportedVersion,
    ReservedFlags,
    Truncated,
    TrailingBytes,
    InvalidUrl,
    InvalidName,
    InvalidToken,
    InvalidConnectivity,
}
impl fmt::Display for JoinCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLong => "el código supera los 512 caracteres",
            Self::BadPrefix => "el código no empieza por ATENEA1",
            Self::BadEncoding => "el código contiene caracteres o longitud no válidos",
            Self::BadChecksum => "la suma de comprobación no coincide: el código tiene erratas",
            Self::UnsupportedVersion => "versión de código no admitida",
            Self::ReservedFlags => "el código usa indicadores reservados",
            Self::Truncated => "el código está incompleto",
            Self::TrailingBytes => "el código tiene datos sobrantes",
            Self::InvalidUrl => "la dirección del servidor no es válida",
            Self::InvalidName => "el nombre de la red no es válido",
            Self::InvalidToken => "el token del código no es válido",
            Self::InvalidConnectivity => "el bloque de conectividad del código no es válido",
        })
    }
}
impl std::error::Error for JoinCodeError {}

fn ascii_visible(s: &str) -> bool {
    s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Misma regla de nombres que dispositivos (controles y marcas bidireccionales fuera, sin recortes).
pub fn valid_name(name: &str) -> bool {
    (1..=48).contains(&name.len())
        && name == name.trim()
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

fn valid_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    (1..=100).contains(&url.len())
        && ascii_visible(url)
        && !url.contains(['@', '?', '#', '\\'])
        && !url.ends_with('/')
        && !host.is_empty()
        && !host.starts_with(':')
}

/// Nombre o IPv4 sin puerto para el STUN: la misma regla que `--stun-host` del CLI (el agente exige los dos
/// puertos, 3478 y 3479, del mismo servidor). Los nombres con otros caracteres (p. ej. IPv6) no se admiten.
pub fn valid_stun_host(host: &str) -> bool {
    (1..=100).contains(&host.len())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

impl JoinCode {
    fn validate(&self) -> Result<(), JoinCodeError> {
        if !valid_url(&self.url) {
            return Err(JoinCodeError::InvalidUrl);
        }
        if !valid_name(&self.name) {
            return Err(JoinCodeError::InvalidName);
        }
        if let Some(t) = &self.token
            && !((1..=64).contains(&t.len()) && ascii_visible(t))
        {
            return Err(JoinCodeError::InvalidToken);
        }
        if let Some(c) = &self.connectivity
            && !valid_stun_host(&c.stun_host)
        {
            return Err(JoinCodeError::InvalidConnectivity);
        }
        Ok(())
    }

    /// Texto canónico (con guiones) sin aplicar el límite de longitud; lo comparten `encode` y `decode`.
    fn render(&self) -> Result<String, JoinCodeError> {
        self.validate()?;
        let mut flags = u8::from(self.token.is_some()) * FLAG_TOKEN;
        if let Some(c) = &self.connectivity {
            flags |= FLAG_CONNECTIVITY;
            if c.relay_ca_sha256.is_some() {
                flags |= FLAG_RELAY_CA;
            }
        }
        let mut p = vec![1, flags];
        // Las longitudes caben en un byte: validate() las acotó a ≤ 100, ≤ 100, ≤ 48 y ≤ 64.
        p.push(self.url.len() as u8);
        p.extend_from_slice(self.url.as_bytes());
        p.extend_from_slice(&self.network_id);
        p.extend_from_slice(&self.root_anchor);
        p.push(self.name.len() as u8);
        p.extend_from_slice(self.name.as_bytes());
        if let Some(c) = &self.connectivity {
            p.push(c.stun_host.len() as u8);
            p.extend_from_slice(c.stun_host.as_bytes());
            if let Some(hash) = &c.relay_ca_sha256 {
                p.extend_from_slice(hash);
            }
        }
        if let Some(t) = &self.token {
            p.push(t.len() as u8);
            p.extend_from_slice(t.as_bytes());
        }
        p.extend_from_slice(&checksum(&p));
        let text = base32(&p);
        let groups: Vec<&str> = (0..text.len())
            .step_by(6)
            .map(|i| &text[i..(i + 6).min(text.len())])
            .collect();
        Ok(format!("{PREFIX}-{}", groups.join("-")))
    }

    pub fn encode(&self) -> Result<String, JoinCodeError> {
        let text = self.render()?;
        // Con la conectividad el peor caso ya no cabe: nunca se emite un texto que el decodificador rechazaría.
        if text.len() > MAX_TEXT_LEN {
            return Err(JoinCodeError::TooLong);
        }
        Ok(text)
    }

    pub fn decode(text: &str) -> Result<Self, JoinCodeError> {
        if text.len() > MAX_TEXT_LEN {
            return Err(JoinCodeError::TooLong);
        }
        let norm: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let body = norm.strip_prefix(PREFIX).ok_or(JoinCodeError::BadPrefix)?;
        let p = unbase32(body)?;
        if p.len() < 5 {
            return Err(JoinCodeError::Truncated);
        }
        let (p, sum) = p.split_at(p.len() - 4);
        if checksum(p) != sum {
            return Err(JoinCodeError::BadChecksum);
        }
        let mut r = Cursor(p);
        if r.take(1)?[0] != 1 {
            return Err(JoinCodeError::UnsupportedVersion);
        }
        let flags = r.take(1)?[0];
        if flags & !KNOWN_FLAGS != 0 {
            return Err(JoinCodeError::ReservedFlags);
        }
        if flags & FLAG_RELAY_CA != 0 && flags & FLAG_CONNECTIVITY == 0 {
            return Err(JoinCodeError::InvalidConnectivity);
        }
        let url = r.string(JoinCodeError::InvalidUrl)?;
        let network_id = r.take(16)?.try_into().expect("longitud fija");
        let root_anchor = r.take(32)?.try_into().expect("longitud fija");
        let name = r.string(JoinCodeError::InvalidName)?;
        let connectivity = (flags & FLAG_CONNECTIVITY != 0)
            .then(|| -> Result<Connectivity, JoinCodeError> {
                let stun_host = r.string(JoinCodeError::InvalidConnectivity)?;
                let relay_ca_sha256 = (flags & FLAG_RELAY_CA != 0)
                    .then(|| r.take(32).map(|b| b.try_into().expect("longitud fija")))
                    .transpose()?;
                Ok(Connectivity {
                    stun_host,
                    relay_ca_sha256,
                })
            })
            .transpose()?;
        let token = (flags & FLAG_TOKEN != 0)
            .then(|| r.string(JoinCodeError::InvalidToken))
            .transpose()?;
        if !r.0.is_empty() {
            return Err(JoinCodeError::TrailingBytes);
        }
        let code = Self {
            url,
            network_id,
            root_anchor,
            name,
            token,
            connectivity,
        };
        code.validate()?;
        // Lo que el codificador no emitiría (forma canónica > 512) tampoco se acepta: el fuzz exige ida y vuelta.
        if code.render()?.len() > MAX_TEXT_LEN {
            return Err(JoinCodeError::TooLong);
        }
        Ok(code)
    }
}

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], JoinCodeError> {
        if self.0.len() < n {
            return Err(JoinCodeError::Truncated);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn string(&mut self, invalid: JoinCodeError) -> Result<String, JoinCodeError> {
        let n = usize::from(self.take(1)?[0]);
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| invalid)
    }
}

fn checksum(payload: &[u8]) -> [u8; 4] {
    let mut m = DOMAIN.to_vec();
    m.extend_from_slice(payload);
    hash(&m)[..4].try_into().expect("longitud fija")
}

/// Base32 RFC 4648 sin relleno.
fn base32(b: &[u8]) -> String {
    let (mut out, mut acc, mut bits) = (String::new(), 0u32, 0);
    for &x in b {
        acc = (acc << 8) | u32::from(x);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[(acc >> bits & 31) as usize] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[(acc << (5 - bits) & 31) as usize] as char);
    }
    out
}

/// Inverso estricto: alfabeto exacto y bits de relleno a cero (una sola codificación por valor).
fn unbase32(s: &str) -> Result<Vec<u8>, JoinCodeError> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes() {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .ok_or(JoinCodeError::BadEncoding)? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if bits >= 5 || acc != 0 {
        return Err(JoinCodeError::BadEncoding);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTOR: &str = "ATENEA1-AEABY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-5PWBEV-4";
    const VECTOR_TOKEN: &str = "ATENEA1-AEARY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YWMRDT-S4S2K4-2HIWSH-KV2FUV-3QNRRF-QQTTMJ-3TPSJM-GQ";

    fn sample(token: Option<&str>) -> JoinCode {
        JoinCode {
            url: "https://control.example.test".into(),
            network_id: core::array::from_fn(|i| i as u8 + 1),
            root_anchor: core::array::from_fn(|i| i as u8 + 0x20),
            name: "Red de ejemplo".into(),
            token: token.map(Into::into),
            connectivity: None,
        }
    }

    /// Construye un texto con payload arbitrario y checksum válido, para probar cada regla.
    fn forge(edit: impl FnOnce(&mut Vec<u8>)) -> String {
        let url = b"https://control.example.test";
        let mut p = vec![1, 0, url.len() as u8];
        p.extend_from_slice(url);
        p.extend_from_slice(&[7; 48]);
        p.extend_from_slice(&[4]);
        p.extend_from_slice(b"Casa");
        edit(&mut p);
        let sum = checksum(&p);
        p.extend_from_slice(&sum);
        format!("{PREFIX}-{}", base32(&p))
    }

    #[test]
    fn vector_fijo() {
        assert_eq!(sample(None).encode().unwrap(), VECTOR);
        assert_eq!(JoinCode::decode(VECTOR).unwrap(), sample(None));
        let t = sample(Some("dG9rZW4tZGUtZWplbXBsbw"));
        assert_eq!(t.encode().unwrap(), VECTOR_TOKEN);
        assert_eq!(JoinCode::decode(VECTOR_TOKEN).unwrap(), t);
    }

    #[test]
    fn ida_y_vuelta_y_tolerancia() {
        for c in [sample(None), sample(Some("abc_DEF-123"))] {
            assert_eq!(JoinCode::decode(&c.encode().unwrap()).unwrap(), c);
        }
        let sucio = format!(" {}\n", VECTOR.to_lowercase().replace('-', " \n"));
        assert_eq!(JoinCode::decode(&sucio).unwrap(), sample(None));
        assert_eq!(
            JoinCode::decode(&VECTOR.replace('-', "")).unwrap(),
            sample(None)
        );
    }

    #[test]
    fn url_con_puerto_y_prefijo() {
        let mut c = sample(None);
        c.url = "https://control.example.test:8443/prefijo".into();
        assert_eq!(JoinCode::decode(&c.encode().unwrap()).unwrap(), c);
    }

    #[test]
    fn reglas_de_url_al_codificar() {
        for u in [
            "http://control.example.test",
            "https://usuario@control.example.test",
            "https://control.example.test?x=1",
            "https://control.example.test#f",
            "https://control.example.test/",
            "https://",
            "https:///ruta",
            "https://control example.test",
            "https://control.example.test\n",
            "https://control.example.test/ñ",
            &format!("https://{}", "a".repeat(93)),
            "",
        ] {
            let mut c = sample(None);
            c.url = u.into();
            assert_eq!(c.encode(), Err(JoinCodeError::InvalidUrl), "{u}");
        }
        let mut c = sample(None);
        c.url = format!("https://{}", "a".repeat(92));
        assert!(c.encode().is_ok());
    }

    #[test]
    fn reglas_de_nombre_al_codificar() {
        for n in [
            "",
            " Casa",
            "Casa ",
            &"a".repeat(49),
            "Ca\nsa",
            "Ca\u{202e}sa",
            "Ca\u{2066}sa",
        ] {
            let mut c = sample(None);
            c.name = n.into();
            assert_eq!(c.encode(), Err(JoinCodeError::InvalidName), "{n:?}");
        }
        let mut c = sample(None);
        c.name = "ñ".repeat(24);
        assert!(c.encode().is_ok());
    }

    #[test]
    fn reglas_de_token_al_codificar() {
        for t in ["", "con espacio", &"a".repeat(65), "ñ"] {
            assert_eq!(
                sample(Some(t)).encode(),
                Err(JoinCodeError::InvalidToken),
                "{t:?}"
            );
        }
        assert!(sample(Some(&"a".repeat(64))).encode().is_ok());
    }

    #[test]
    fn reglas_al_decodificar() {
        let bad = |edit: fn(&mut Vec<u8>)| JoinCode::decode(&forge(edit)).unwrap_err();
        assert!(JoinCode::decode(&forge(|_| {})).is_ok());
        assert_eq!(bad(|p| p[0] = 2), JoinCodeError::UnsupportedVersion);
        assert_eq!(bad(|p| p[1] = 8), JoinCodeError::ReservedFlags);
        assert_eq!(bad(|p| p[1] = 0x80), JoinCodeError::ReservedFlags);
        // conectividad anunciada sin su bloque
        assert_eq!(bad(|p| p[1] = 2), JoinCodeError::Truncated);
        // flag de token sin token
        assert_eq!(bad(|p| p[1] = 1), JoinCodeError::Truncated);
        // token sin flag → bytes sobrantes
        assert_eq!(
            bad(|p| p.extend_from_slice(&[2, b'a', b'b'])),
            JoinCodeError::TrailingBytes
        );
        assert_eq!(bad(|p| p.push(0)), JoinCodeError::TrailingBytes);
        assert_eq!(bad(|p| p.truncate(10)), JoinCodeError::Truncated);
        // url http
        assert_eq!(
            bad(|p| p[7] = b'p'), // "https" → "httpp"
            JoinCodeError::InvalidUrl
        );
        // nombre vacío
        assert_eq!(
            bad(|p| {
                let n = p.len() - 5;
                p.truncate(n);
                p.push(0);
            }),
            JoinCodeError::InvalidName
        );
        // nombre con U+202E
        assert_eq!(
            bad(|p| {
                let n = p.len() - 5;
                p.truncate(n);
                let s = "a\u{202e}b".as_bytes();
                p.push(s.len() as u8);
                p.extend_from_slice(s);
            }),
            JoinCodeError::InvalidName
        );
        // token con flag pero vacío
        assert_eq!(
            bad(|p| {
                p[1] = 1;
                p.push(0);
            }),
            JoinCodeError::InvalidToken
        );
    }

    #[test]
    fn texto_demasiado_largo_prefijo_y_relleno() {
        assert_eq!(
            JoinCode::decode(&"A".repeat(513)),
            Err(JoinCodeError::TooLong)
        );
        assert!(JoinCode::decode(&VECTOR.replace("ATENEA1", "ATENEA2")).is_err());
        assert_eq!(
            JoinCode::decode(&VECTOR.replace("ATENEA1-", "")),
            Err(JoinCodeError::BadPrefix)
        );
        assert_eq!(JoinCode::decode(""), Err(JoinCodeError::BadPrefix));
        assert_eq!(JoinCode::decode("ATENEA1"), Err(JoinCodeError::Truncated));
        // caracteres fuera del alfabeto
        assert_eq!(
            JoinCode::decode(&VECTOR.replace("AEABY2", "AEAB!2")),
            Err(JoinCodeError::BadEncoding)
        );
    }

    #[test]
    fn maximo_cabe_en_512() {
        let c = JoinCode {
            url: format!("https://{}", "a".repeat(92)),
            name: "a".repeat(48),
            token: Some("t".repeat(64)),
            ..sample(None)
        };
        assert!(c.encode().unwrap().len() <= MAX_TEXT_LEN);
    }

    #[test]
    fn toda_alteracion_de_un_caracter_se_rechaza() {
        for original in [
            VECTOR,
            VECTOR_TOKEN,
            VECTOR_CONN,
            VECTOR_CONN_CA,
            VECTOR_CONN_TOKEN_CA,
        ] {
            let good = JoinCode::decode(original).unwrap();
            let chars: Vec<char> = original.chars().collect();
            for i in 0..chars.len() {
                for r in ALPHABET.iter().map(|&b| b as char).chain(['-', '1']) {
                    if r.eq_ignore_ascii_case(&chars[i]) {
                        continue;
                    }
                    let mut t = chars.clone();
                    t[i] = r;
                    let t: String = t.into_iter().collect();
                    assert!(
                        JoinCode::decode(&t) != Ok(good.clone()),
                        "aceptado en {i} con {r}"
                    );
                    assert!(JoinCode::decode(&t).is_err(), "aceptado en {i} con {r}");
                }
            }
        }
    }

    #[test]
    fn errores_y_debug_no_incluyen_el_token() {
        let t = "secreto-token_123";
        let c = sample(Some(t));
        assert!(!format!("{c:?}").contains(t));
        let mut m = c.clone();
        m.url = "http://x".into();
        assert!(!format!("{:?}{}", m.encode(), m.encode().unwrap_err()).contains(t));
    }

    fn sample_conn(token: Option<&str>, ca: bool) -> JoinCode {
        JoinCode {
            connectivity: Some(Connectivity {
                stun_host: "relay.example.test".into(),
                relay_ca_sha256: ca.then(|| core::array::from_fn(|i| i as u8 + 0x60)),
            }),
            ..sample(token)
        }
    }

    #[test]
    fn conectividad_ida_y_vuelta() {
        for c in [
            sample_conn(None, false),
            sample_conn(None, true),
            sample_conn(Some("abc_DEF-123"), false),
            sample_conn(Some("abc_DEF-123"), true),
        ] {
            let text = c.encode().unwrap();
            assert!(text.len() <= MAX_TEXT_LEN);
            assert_eq!(JoinCode::decode(&text).unwrap(), c);
        }
        // Un código sin el bloque se decodifica sin conectividad (los vectores de la v0.1.8 siguen valiendo).
        assert_eq!(JoinCode::decode(VECTOR).unwrap().connectivity, None);
        assert_eq!(JoinCode::decode(VECTOR_TOKEN).unwrap().connectivity, None);
    }

    #[test]
    fn reglas_de_stun_al_codificar() {
        for h in [
            "",
            "host:3478",
            "con espacio",
            "bad_host",
            "ñ",
            "relay.example.test\n",
            "relay/ruta",
            &"a".repeat(101),
        ] {
            let mut c = sample_conn(None, false);
            c.connectivity.as_mut().unwrap().stun_host = h.into();
            assert_eq!(c.encode(), Err(JoinCodeError::InvalidConnectivity), "{h:?}");
        }
        for h in [
            "relay.example.test",
            "203.0.113.7",
            "a-b.example",
            &"a".repeat(100),
        ] {
            let mut c = sample_conn(None, true);
            c.connectivity.as_mut().unwrap().stun_host = h.into();
            assert!(c.encode().is_ok(), "{h:?}");
        }
    }

    const VECTOR_CONN: &str = "ATENEA1-AEBBY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YSOJSW-YYLZFZ-SXQYLN-OBWGKL-TUMVZX-I2T7VG-KA";
    const VECTOR_CONN_CA: &str = "ATENEA1-AEDBY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YSOJSW-YYLZFZ-SXQYLN-OBWGKL-TUMVZX-IYDBMJ-RWIZLG-M5UGS2-TLNRWW-433QOF-ZHG5DV-OZ3XQ6-L2PN6H-27T7CE-H4DZI";
    const VECTOR_CONN_TOKEN_CA: &str = "ATENEA1-AEDRY2-DUORYH-GORPF5-RW63TU-OJXWYL-TFPBQW-24DMMU-XHIZLT-OQAQEA-YEAUDA-OCAJBI-FQYDIO-B4ICAI-JCEMSC-KJRHFA-USUKZM-FUXC6M-BRGIZT-INJWG4-4DSOR3-HQ6T4P-YOKJSW-IIDEMU-QGK2TF-NVYGY3-YSOJSW-YYLZFZ-SXQYLN-OBWGKL-TUMVZX-IYDBMJ-RWIZLG-M5UGS2-TLNRWW-433QOF-ZHG5DV-OZ3XQ6-L2PN6H-27T7CZ-SEOOLS-LJLTI5-C2I5KX-IWSXOB-WGEWCC-ONRHPE-HPRAEA";

    #[test]
    fn vectores_de_conectividad() {
        assert_eq!(sample_conn(None, false).encode().unwrap(), VECTOR_CONN);
        assert_eq!(
            JoinCode::decode(VECTOR_CONN).unwrap(),
            sample_conn(None, false)
        );
        assert_eq!(sample_conn(None, true).encode().unwrap(), VECTOR_CONN_CA);
        assert_eq!(
            JoinCode::decode(VECTOR_CONN_CA).unwrap(),
            sample_conn(None, true)
        );
        let t = sample_conn(Some("dG9rZW4tZGUtZWplbXBsbw"), true);
        assert_eq!(t.encode().unwrap(), VECTOR_CONN_TOKEN_CA);
        assert_eq!(JoinCode::decode(VECTOR_CONN_TOKEN_CA).unwrap(), t);
    }

    #[test]
    fn reglas_de_conectividad_al_decodificar() {
        let bad = |edit: fn(&mut Vec<u8>)| JoinCode::decode(&forge(edit)).unwrap_err();
        let ok = |edit: fn(&mut Vec<u8>)| JoinCode::decode(&forge(edit)).unwrap();
        // válido: solo STUN
        let c = ok(|p| {
            p[1] = 2;
            p.extend_from_slice(&[5]);
            p.extend_from_slice(b"stun1");
        });
        let conn = c.connectivity.unwrap();
        assert_eq!(conn.stun_host, "stun1");
        assert_eq!(conn.relay_ca_sha256, None);
        // válido: STUN + hash de la CA
        let c = ok(|p| {
            p[1] = 6;
            p.extend_from_slice(&[5]);
            p.extend_from_slice(b"stun1");
            p.extend_from_slice(&[0xab; 32]);
        });
        assert_eq!(c.connectivity.unwrap().relay_ca_sha256, Some([0xab; 32]));
        // válido: conectividad antes del token
        let c = ok(|p| {
            p[1] = 3;
            p.extend_from_slice(&[5]);
            p.extend_from_slice(b"stun1");
            p.extend_from_slice(&[1, b'a']);
        });
        assert_eq!(c.token.as_deref(), Some("a"));
        // hash de CA sin conectividad
        assert_eq!(
            bad(|p| {
                p[1] = 4;
                p.extend_from_slice(&[0xab; 32]);
            }),
            JoinCodeError::InvalidConnectivity
        );
        // STUN vacío, con puerto o no UTF-8
        assert_eq!(
            bad(|p| {
                p[1] = 2;
                p.push(0);
            }),
            JoinCodeError::InvalidConnectivity
        );
        assert_eq!(
            bad(|p| {
                p[1] = 2;
                p.extend_from_slice(&[3]);
                p.extend_from_slice(b"a:1");
            }),
            JoinCodeError::InvalidConnectivity
        );
        assert_eq!(
            bad(|p| {
                p[1] = 2;
                p.extend_from_slice(&[1, 0xff]);
            }),
            JoinCodeError::InvalidConnectivity
        );
        // hash de CA incompleto
        assert_eq!(
            bad(|p| {
                p[1] = 6;
                p.extend_from_slice(&[5]);
                p.extend_from_slice(b"stun1");
                p.extend_from_slice(&[0xab; 10]);
            }),
            JoinCodeError::Truncated
        );
        // bytes sobrantes tras el bloque; y un bit reservado junto a la conectividad
        assert_eq!(
            bad(|p| {
                p[1] = 2;
                p.extend_from_slice(&[5]);
                p.extend_from_slice(b"stun1");
                p.push(0);
            }),
            JoinCodeError::TrailingBytes
        );
        assert_eq!(
            bad(|p| {
                p[1] = 0x0a;
                p.extend_from_slice(&[5]);
                p.extend_from_slice(b"stun1");
            }),
            JoinCodeError::ReservedFlags
        );
    }

    #[test]
    fn la_conectividad_respeta_el_limite_de_512_al_codificar() {
        let peor = JoinCode {
            url: format!("https://{}", "a".repeat(92)),
            name: "a".repeat(48),
            token: Some("t".repeat(64)),
            connectivity: Some(Connectivity {
                stun_host: "s".repeat(100),
                relay_ca_sha256: Some([9; 32]),
            }),
            ..sample(None)
        };
        assert_eq!(peor.encode(), Err(JoinCodeError::TooLong));
        // Un caso realista cabe con holgura y nunca se emite un texto que el decodificador rechazaría por longitud.
        let realista = sample_conn(Some(&"t".repeat(43)), true);
        assert!(realista.encode().unwrap().len() < MAX_TEXT_LEN);
    }

    /// El fuzz exige que todo lo que `decode` acepta se re-emita con `encode`: un texto sin guiones de ≤ 512
    /// caracteres cuya forma canónica (con guiones) supera 512 NO debe aceptarse.
    #[test]
    fn decode_rechaza_lo_que_encode_no_emitiria() {
        let grande = JoinCode {
            url: format!("https://{}", "a".repeat(92)),
            name: "a".repeat(48),
            connectivity: Some(Connectivity {
                stun_host: "s".repeat(60),
                relay_ca_sha256: Some([9; 32]),
            }),
            ..sample(None)
        };
        assert_eq!(grande.encode(), Err(JoinCodeError::TooLong));
        let canonico = grande.render().unwrap();
        assert!(canonico.len() > MAX_TEXT_LEN);
        let sin_guiones: String = canonico.chars().filter(|c| *c != '-').collect();
        assert!(sin_guiones.len() <= MAX_TEXT_LEN, "{}", sin_guiones.len());
        assert_eq!(JoinCode::decode(&sin_guiones), Err(JoinCodeError::TooLong));
        assert_eq!(JoinCode::decode(&canonico), Err(JoinCodeError::TooLong));
    }

    /// Mismo invariante que el fuzz `join_code_parse`: lo que `decode` acepta, `encode` lo re-emite y vuelve igual.
    #[test]
    fn todo_lo_aceptado_se_reemite_y_vuelve_igual() {
        let comprueba = |t: &str| {
            if let Ok(c) = JoinCode::decode(t) {
                let e = c.encode().expect("lo decodificado es válido");
                assert_eq!(JoinCode::decode(&e), Ok(c));
            }
        };
        for v in [
            VECTOR,
            VECTOR_TOKEN,
            VECTOR_CONN,
            VECTOR_CONN_CA,
            VECTOR_CONN_TOKEN_CA,
        ] {
            for n in 0..=v.len() {
                comprueba(&v[..n]);
                comprueba(&v[n..]);
            }
            let plano: String = v.chars().filter(|c| *c != '-').collect();
            comprueba(&plano);
            comprueba(&plano.to_lowercase());
        }
    }
}
