//! STUN mínimo (RFC 8489: solo Binding sobre UDP, sin autenticación) y utilidades de candidatos.
//!
//! STUN descubre el endpoint que ve un servidor; no es una solución de NAT completa. Los endpoints
//! aprendidos aquí o del coordinator son **pistas no autenticadas**: solo un handshake Noise
//! autenticado convierte un candidato en una ruta (ADR-001, ADR-003).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const MAGIC_COOKIE: u32 = 0x2112_A442;
pub type TxId = [u8; 12];

const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const HEADER_LEN: usize = 20;
/// Tamaño máximo que se considera un mensaje STUN (los de Binding son diminutos).
pub const MAX_STUN_LEN: usize = 128;
/// Máximo de candidatos que se aceptan de un tercero (coordinator) para un par.
pub const MAX_CANDIDATES: usize = 8;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StunError {
    TooShort,
    TooLong,
    BadCookie,
    BadLength,
    UnsupportedType,
    BadAttribute,
    MissingMappedAddress,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Message {
    BindingRequest { tx: TxId },
    BindingSuccess { tx: TxId, mapped: SocketAddr },
}

/// Discriminación barata en el socket de datos compartido: un datagrama de Atenea tiene por primer
/// byte la versión `1` y por segundo el tipo `1..=3`; un mensaje STUN lleva el `magic cookie` en
/// los bytes 4..8, su longitud de atributos es múltiplo de 4 y el tipo es Binding (`0x0001` o `0x0101`).
/// La coincidencia accidental de un Init de Atenea con esta forma tiene probabilidad ≈ 2⁻³²
/// (el cliente reintenta el handshake).
pub fn looks_like_stun(buf: &[u8]) -> bool {
    if buf.len() < HEADER_LEN || buf.len() > MAX_STUN_LEN || buf[0] & 0xC0 != 0 {
        return false;
    }
    if u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) != MAGIC_COOKIE {
        return false;
    }
    let len = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
    let ty = u16::from_be_bytes([buf[0], buf[1]]);
    len % 4 == 0 && HEADER_LEN + len == buf.len() && matches!(ty, BINDING_REQUEST | BINDING_SUCCESS)
}

pub fn encode_binding_request(tx: &TxId) -> [u8; HEADER_LEN] {
    let mut b = [0u8; HEADER_LEN];
    b[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    b[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    b[8..20].copy_from_slice(tx);
    b
}

pub fn encode_binding_success(tx: &TxId, mapped: SocketAddr) -> Vec<u8> {
    let mut attr = vec![0u8, if mapped.is_ipv4() { 1 } else { 2 }];
    attr.extend_from_slice(&(mapped.port() ^ (MAGIC_COOKIE >> 16) as u16).to_be_bytes());
    let mut mask = MAGIC_COOKIE.to_be_bytes().to_vec();
    mask.extend_from_slice(tx);
    let octets: Vec<u8> = match mapped.ip() {
        IpAddr::V4(a) => a.octets().to_vec(),
        IpAddr::V6(a) => a.octets().to_vec(),
    };
    attr.extend(octets.iter().zip(&mask).map(|(a, m)| a ^ m));
    let mut b = Vec::with_capacity(HEADER_LEN + 4 + attr.len());
    b.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
    b.extend_from_slice(&((4 + attr.len()) as u16).to_be_bytes());
    b.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    b.extend_from_slice(tx);
    b.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
    b.extend_from_slice(&(attr.len() as u16).to_be_bytes());
    b.extend_from_slice(&attr);
    b
}

fn decode_xor_mapped(value: &[u8], tx: &TxId) -> Result<SocketAddr, StunError> {
    if value.len() < 4 || value[0] != 0 {
        return Err(StunError::BadAttribute);
    }
    let port = u16::from_be_bytes([value[2], value[3]]) ^ (MAGIC_COOKIE >> 16) as u16;
    let mut mask = MAGIC_COOKIE.to_be_bytes().to_vec();
    mask.extend_from_slice(tx);
    match (value[1], value.len()) {
        (1, 8) => {
            let mut o = [0u8; 4];
            for i in 0..4 {
                o[i] = value[4 + i] ^ mask[i];
            }
            Ok(SocketAddr::new(Ipv4Addr::from(o).into(), port))
        }
        (2, 20) => {
            let mut o = [0u8; 16];
            for i in 0..16 {
                o[i] = value[4 + i] ^ mask[i];
            }
            Ok(SocketAddr::new(Ipv6Addr::from(o).into(), port))
        }
        _ => Err(StunError::BadAttribute),
    }
}

/// Parsea una petición o una respuesta de éxito de Binding. Ignora atributos desconocidos.
pub fn parse(buf: &[u8]) -> Result<Message, StunError> {
    if buf.len() < HEADER_LEN {
        return Err(StunError::TooShort);
    }
    if buf.len() > MAX_STUN_LEN {
        return Err(StunError::TooLong);
    }
    if u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) != MAGIC_COOKIE {
        return Err(StunError::BadCookie);
    }
    let len = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
    if len % 4 != 0 || HEADER_LEN + len != buf.len() {
        return Err(StunError::BadLength);
    }
    let mut tx = [0u8; 12];
    tx.copy_from_slice(&buf[8..20]);
    // Estructura TLV de todos los atributos (también en peticiones): longitudes y relleno coherentes.
    let mut attrs = &buf[HEADER_LEN..];
    while !attrs.is_empty() {
        if attrs.len() < 4 {
            return Err(StunError::BadAttribute);
        }
        let l = usize::from(u16::from_be_bytes([attrs[2], attrs[3]]));
        let padded = l.div_ceil(4) * 4;
        if attrs.len() < 4 + padded {
            return Err(StunError::BadAttribute);
        }
        attrs = &attrs[4 + padded..];
    }
    match u16::from_be_bytes([buf[0], buf[1]]) {
        BINDING_REQUEST => Ok(Message::BindingRequest { tx }),
        BINDING_SUCCESS => {
            let mut rest = &buf[HEADER_LEN..];
            while !rest.is_empty() {
                if rest.len() < 4 {
                    return Err(StunError::BadAttribute);
                }
                let ty = u16::from_be_bytes([rest[0], rest[1]]);
                let l = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
                let padded = l.div_ceil(4) * 4;
                if rest.len() < 4 + padded {
                    return Err(StunError::BadAttribute);
                }
                if ty == ATTR_XOR_MAPPED_ADDRESS {
                    return Ok(Message::BindingSuccess {
                        tx,
                        mapped: decode_xor_mapped(&rest[4..4 + l], &tx)?,
                    });
                }
                rest = &rest[4 + padded..];
            }
            Err(StunError::MissingMappedAddress)
        }
        _ => Err(StunError::UnsupportedType),
    }
}

/// Comportamiento de mapeo del NAT observado con dos consultas desde el MISMO socket local a dos
/// destinos distintos (aquí, dos puertos del mismo servidor STUN).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NatMapping {
    /// Mismo endpoint externo hacia ambos destinos: el hole punching es viable.
    Independent,
    /// El endpoint externo cambia con el destino (NAT simétrico): el hole punching directo fallará.
    Dependent,
}

pub fn classify_mapping(first: SocketAddr, second: SocketAddr) -> NatMapping {
    if first == second {
        NatMapping::Independent
    } else {
        NatMapping::Dependent
    }
}

/// Un candidato aceptable: dirección unicast enrutable (privadas permitidas, redes locales incluidas)
/// con puerto distinto de cero. Se descartan no especificadas, de difusión, multicast, loopback,
/// enlace local (necesitan ámbito) y IPv4-mapeadas en IPv6.
pub fn candidate_ok(addr: &SocketAddr) -> bool {
    if addr.port() == 0 {
        return false;
    }
    match addr.ip() {
        IpAddr::V4(a) => {
            !(a.is_unspecified()
                || a.is_broadcast()
                || a.is_multicast()
                || a.is_loopback()
                || a.is_link_local())
        }
        IpAddr::V6(a) => {
            !(a.is_unspecified()
                || a.is_multicast()
                || a.is_loopback()
                || (a.segments()[0] & 0xffc0) == 0xfe80
                || a.to_ipv4_mapped().is_some())
        }
    }
}

/// Filtra, deduplica conservando el orden y acota la lista de candidatos recibida de un tercero.
pub fn sanitize_candidates(
    input: impl IntoIterator<Item = SocketAddr>,
    max: usize,
) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = Vec::new();
    for a in input {
        if candidate_ok(&a) && !out.contains(&a) {
            out.push(a);
            if out.len() == max {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const TX: TxId = [7; 12];

    fn sa(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    /// Vector del RFC 5769 §2.2 (respuesta IPv4 con XOR-MAPPED-ADDRESS 192.0.2.1:32853), sin SOFTWARE ni integridad.
    #[test]
    fn rfc5769_style_ipv4_roundtrip() {
        let tx = [
            0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
        ];
        let m = encode_binding_success(&tx, sa("192.0.2.1:32853"));
        assert_eq!(&m[20..24], &[0x00, 0x20, 0x00, 0x08]);
        // port XOR 0x2112 = 0xA147; dirección XOR cookie = e1 12 a6 43
        assert_eq!(
            &m[24..32],
            &[0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43]
        );
        assert_eq!(
            parse(&m),
            Ok(Message::BindingSuccess {
                tx,
                mapped: sa("192.0.2.1:32853")
            })
        );
        assert!(looks_like_stun(&m));
    }

    #[test]
    fn ipv6_roundtrip() {
        let m = encode_binding_success(&TX, sa("[2001:db8::7]:51820"));
        assert_eq!(
            parse(&m),
            Ok(Message::BindingSuccess {
                tx: TX,
                mapped: sa("[2001:db8::7]:51820")
            })
        );
    }

    #[test]
    fn request_roundtrip_and_discrimination() {
        let r = encode_binding_request(&TX);
        assert_eq!(parse(&r), Ok(Message::BindingRequest { tx: TX }));
        assert!(looks_like_stun(&r));
        // un Init de Atenea (versión 1, tipo 1, 114 bytes) no es STUN
        let mut init = vec![0u8; 114];
        init[0] = 1;
        init[1] = 1;
        assert!(!looks_like_stun(&init));
        // ni un datagrama de datos corto
        let data = [
            1u8, 3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0,
        ];
        assert!(!looks_like_stun(&data));
    }

    #[test]
    fn rejects_malformed() {
        let r = encode_binding_request(&TX);
        assert_eq!(parse(&r[..10]), Err(StunError::TooShort));
        let mut bad = r;
        bad[4] ^= 1;
        assert_eq!(parse(&bad), Err(StunError::BadCookie));
        let mut bad = r;
        bad[3] = 4; // longitud declarada sin atributos
        assert_eq!(parse(&bad), Err(StunError::BadLength));
        let mut bad = r;
        bad[0] = 0;
        bad[1] = 0x11; // error de Binding
        assert_eq!(parse(&bad), Err(StunError::UnsupportedType));
        // éxito sin XOR-MAPPED-ADDRESS
        let mut none = r.to_vec();
        none[0..2].copy_from_slice(&BINDING_SUCCESS.to_be_bytes());
        assert_eq!(parse(&none), Err(StunError::MissingMappedAddress));
        // atributo truncado
        let mut m = encode_binding_success(&TX, sa("192.0.2.1:1"));
        m.truncate(m.len() - 4);
        let l = (m.len() - 20) as u16;
        m[2..4].copy_from_slice(&l.to_be_bytes());
        assert_eq!(parse(&m), Err(StunError::BadAttribute));
        assert_eq!(parse(&[0u8; MAX_STUN_LEN + 1]), Err(StunError::TooLong));
    }

    #[test]
    fn unknown_attributes_are_skipped() {
        let mut m = encode_binding_success(&TX, sa("192.0.2.9:4000"));
        // inserta un atributo desconocido (SOFTWARE 0x8022, "ab" + relleno) antes del mapeo
        let extra = [0x80, 0x22, 0x00, 0x02, b'a', b'b', 0, 0];
        m.splice(20..20, extra);
        let l = (m.len() - 20) as u16;
        m[2..4].copy_from_slice(&l.to_be_bytes());
        assert_eq!(
            parse(&m),
            Ok(Message::BindingSuccess {
                tx: TX,
                mapped: sa("192.0.2.9:4000")
            })
        );
    }

    #[test]
    fn request_with_malformed_attributes_is_rejected() {
        // Atributo de 8 bytes declarado pero solo 4 presentes (la longitud global cuadra).
        let mut r = encode_binding_request(&TX).to_vec();
        r.extend_from_slice(&[0x80, 0x22, 0x00, 0x08, 1, 2, 3, 4]);
        r[2..4].copy_from_slice(&8u16.to_be_bytes());
        assert_eq!(parse(&r), Err(StunError::BadAttribute));
        // Atributo bien formado: se acepta (y se ignora).
        let mut ok = encode_binding_request(&TX).to_vec();
        ok.extend_from_slice(&[0x80, 0x22, 0x00, 0x04, b'a', b't', b'e', b'n']);
        ok[2..4].copy_from_slice(&8u16.to_be_bytes());
        assert_eq!(parse(&ok), Ok(Message::BindingRequest { tx: TX }));
    }

    #[test]
    fn mapping_classification() {
        assert_eq!(
            classify_mapping(sa("203.0.113.5:40000"), sa("203.0.113.5:40000")),
            NatMapping::Independent
        );
        assert_eq!(
            classify_mapping(sa("203.0.113.5:40000"), sa("203.0.113.5:40001")),
            NatMapping::Dependent
        );
    }

    #[test]
    fn candidate_filtering() {
        for good in [
            "192.0.2.1:1",
            "10.0.0.5:51820",
            "[2001:db8::1]:9",
            "[fd00::1]:9",
        ] {
            assert!(candidate_ok(&sa(good)), "{good}");
        }
        for bad in [
            "0.0.0.0:9",
            "255.255.255.255:9",
            "224.0.0.1:9",
            "127.0.0.1:9",
            "169.254.1.1:9",
            "10.0.0.1:0",
            "[::]:9",
            "[::1]:9",
            "[ff02::1]:9",
            "[fe80::1]:9",
            "[::ffff:1.2.3.4]:9",
        ] {
            assert!(!candidate_ok(&sa(bad)), "{bad}");
        }
        let v = sanitize_candidates(
            [
                "1.1.1.1:1",
                "127.0.0.1:1",
                "1.1.1.1:1",
                "2.2.2.2:2",
                "3.3.3.3:3",
            ]
            .map(sa),
            2,
        );
        assert_eq!(v, vec![sa("1.1.1.1:1"), sa("2.2.2.2:2")]);
    }

    proptest! {
        #[test]
        fn parse_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..160)) {
            let _ = parse(&buf);
            let _ = looks_like_stun(&buf);
        }

        #[test]
        fn success_roundtrips(a: [u8; 4], port in 1u16.., tx: [u8; 12], v6: bool, b: [u8; 16]) {
            let addr = if v6 { SocketAddr::new(Ipv6Addr::from(b).into(), port) } else { SocketAddr::new(Ipv4Addr::from(a).into(), port) };
            let m = encode_binding_success(&tx, addr);
            prop_assert_eq!(parse(&m), Ok(Message::BindingSuccess { tx, mapped: addr }));
        }
    }
}
