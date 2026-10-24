use super::*;
use proptest::prelude::*;
use std::{fs, path::PathBuf};
const NOW: u64 = 1000;
#[test]
fn certificate_verification_for_control_plane() {
    let (root, _, cert, _, _, _) = setup();
    let anchor = root.verifying_key().to_bytes();
    assert!(cert.verify(&anchor, cert.network_id, NOW).is_ok());
    assert!(cert.verify(&anchor, [9; 16], NOW).is_err());
    assert!(
        cert.verify(&anchor, cert.network_id, cert.not_after)
            .is_err()
    );
    assert!(
        cert.verify(&anchor, cert.network_id, cert.not_before - 1)
            .is_err()
    );
    assert!(
        cert.verify(
            &signing_key([9; 32]).verifying_key().to_bytes(),
            cert.network_id,
            NOW
        )
        .is_err()
    );
    let mut bad = cert.clone();
    bad.signature[0] ^= 1;
    assert!(bad.verify(&anchor, cert.network_id, NOW).is_err());
}
fn setup() -> (
    SigningKey,
    SigningKey,
    IssuerCert,
    DeviceConfig,
    LocalKeys,
    AgentState,
) {
    let root = signing_key([1; 32]);
    let issuer = signing_key([2; 32]);
    let identity = signing_key([3; 32]);
    let cert = IssuerCert::issue(
        &root,
        [4; 16],
        2,
        800,
        2000,
        issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap();
    let cfg = DeviceConfig {
        network_id: [4; 16],
        device_id: [5; 16],
        issuer_serial: 2,
        config_version: 2,
        not_before: 900,
        not_after: 1200,
        noise_pubkey: [6; 32],
        identity_pubkey: identity.verifying_key().to_bytes(),
        mtu: 1400,
        tunnel_addrs: vec!["10.0.0.1".parse().unwrap()],
        network_prefixes: vec![Prefix {
            addr: "10.0.0.0".parse().unwrap(),
            len: 24,
        }],
        peers: vec![Peer {
            device_id: [7; 16],
            noise_pubkey: [8; 32],
            addrs: vec!["10.0.0.2".parse().unwrap()],
            endpoints: vec!["192.0.2.1:443".parse().unwrap()],
        }],
        rules: vec![Rule {
            src: Selector::Device([5; 16]),
            dst: Selector::Device([7; 16]),
            proto: 6,
            port_min: 443,
            port_max: 443,
        }],
    };
    let local = LocalKeys {
        noise_pub: cfg.noise_pubkey,
        identity_pub: cfg.identity_pubkey,
    };
    let mut state = AgentState::new(
        cfg.network_id,
        cfg.device_id,
        root.verifying_key().to_bytes(),
    );
    state.max_config_version = 1;
    state.max_issuer_serial = 1;
    state.max_time_observed = 1000;
    (root, issuer, cert, cfg, local, state)
}
fn raw(issuer: &SigningKey, cert: &IssuerCert, cfg: &DeviceConfig) -> Vec<u8> {
    let body = cfg.encode_wire();
    let mut b = b"AOCF\x01".to_vec();
    b.extend_from_slice(&cert.encode());
    b.extend_from_slice(&(body.len() as u32).to_be_bytes());
    b.extend_from_slice(&body);
    b.extend_from_slice(&sign(issuer, CONFIG_DOMAIN, &body));
    b
}
fn resign(root: &SigningKey, c: &mut IssuerCert) {
    c.signature = sign(root, CERT_DOMAIN, &c.encode()[..114]);
}
fn rejects(b: &[u8], local: &LocalKeys, state: &AgentState, now: u64, e: ConfigError) {
    assert_eq!(verify_config(b, local, state, now).err(), Some(e));
}
fn fixture(name: &str, b: &[u8]) {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/identity")
        .join(name);
    // El modo de regeneración es explícito; la prueba normal nunca reescribe vectores.
    if std::env::var_os("ATENEA_REGENERATE_IDENTITY").is_some() {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b).unwrap();
    }
    assert_eq!(fs::read(&p).unwrap(), b, "vector {name}");
}
#[test]
fn vectores_sinteticos_regenerables() {
    let (root, issuer, cert, cfg, local, state) = setup();
    let valid = issue_config(&issuer, &cert, &cfg).unwrap();
    fixture(
        "valid_bundle.bin",
        &DeviceBundle::create(cfg.device_id, cfg.noise_pubkey, &signing_key([3; 32])).encode(),
    );
    fixture("valid_cert.bin", &cert.encode());
    fixture("valid_config.bin", &valid);
    use ConfigError::*;
    for error in [
        TooLarge,
        UnsupportedVersion,
        Malformed,
        UnknownRoot,
        BadCertSignature,
        CertExpired,
        CertNotYetValid,
        IssuerRollback,
        BadConfigSignature,
        ScopeMismatch,
        WrongDevice,
        ConfigRollback,
        Expired,
        NotYetValid,
        ClockAnomaly,
        InvalidStructure,
    ] {
        let mut c = cert.clone();
        let mut d = cfg.clone();
        let mut now = NOW;
        match error {
            UnknownRoot => c.root_key_id = [0; 8],
            BadCertSignature => c.signature[0] ^= 1,
            IssuerRollback => {
                c.issuer_serial = 0;
                d.issuer_serial = 0;
                resign(&root, &mut c);
            }
            ScopeMismatch => d.network_id = [9; 16],
            WrongDevice => d.device_id = [9; 16],
            ConfigRollback => d.config_version = 1,
            InvalidStructure => d.peers[0].addrs = d.tunnel_addrs.clone(),
            CertExpired => now = 2030,
            CertNotYetValid => now = 769,
            Expired => now = 1230,
            NotYetValid => now = 869,
            ClockAnomaly => now = 969,
            _ => {}
        }
        let mut b = raw(&issuer, &c, &d);
        match error {
            TooLarge => b.resize(252 + MAX_BODY_LEN, 0),
            UnsupportedVersion => b[4] = 2,
            Malformed => b[189] = 1,
            BadConfigSignature => *b.last_mut().unwrap() ^= 1,
            _ => {}
        }
        rejects(&b, &local, &state, now, error);
        fixture(&format!("neg_{}.bin", error.code()), &b);
    }
    fixture("README.md", VECTOR_README.as_bytes());
}
const VECTOR_README: &str = "# Vectores sintéticos de identidad v1\n\nGenerados por `vectores_sinteticos_regenerables` del crate atenea-identity.\nRegeneración explícita: `ATENEA_REGENERATE_IDENTITY=1 cargo test -p atenea-identity vectores_sinteticos_regenerables`; sin la variable solo compara bytes.\nNunca contienen claves de producción. Semillas Ed25519: raíz [1;32], emisor [2;32], identidad [3;32]. Noise público sintético [6;32] (no se usa para DH). Red [4;16], dispositivo [5;16], peer [7;16] con Noise [8;32].\nCertificado: serial 2, ventana [800,2000); configuración: versión 2, ventana [900,1200), MTU 1400, propias 10.0.0.1, red 10.0.0.0/24, peer 10.0.0.2, pista 192.0.2.1:443, regla TCP/443 entre ambos dispositivos.\nEstado inicial: ancla raíz [1;32] derivada, máximos versión=1, serial=1, tiempo=1000, hash cero. now=1000 salvo: cert_expired=2030, cert_not_yet_valid=769, expired=1230, not_yet_valid=869, clock_anomaly=969. Cada neg_<codigo>.bin devuelve ese código con este estado y reloj.\nNegativos: tamaño excedido; versión externa 2; reservado cuerpo 1; raíz desconocida; firma certificado alterada; ventanas temporales; serial 0 firmado; firma cuerpo alterada; ámbito/dispositivo distintos firmados; versión 1 con hash distinto; direcciones de peer iguales a propias firmadas. Se recalculan las firmas cuando corresponde para alcanzar el paso pretendido.\nEl estado local AOST v1 usa magic[4], versión u8, red[16], dispositivo[16], contador anclas u8, anclas[32] en orden de antigüedad, máximos versión/serial/tiempo u64 BE y hash[32]. No tiene checksum: detecta corrupción estructural, no manipulación coherente del disco (fuera del límite de confianza).\nDecisiones sobre ambigüedades: rangos/MTU/validez/versión cero se rechazan al parsear (paso 3); relaciones entre dispositivos y pertenencia a red en paso 14. Los endpoints públicos no tienen que pertenecer al prefijo túnel. Selectores Prefix deben quedar contenidos en un prefijo de red. Un certificado con ventana invertida o duración excesiva devuelve cert_expired. La rotación retira primero el ancla antigua si firma la más reciente; si la antigua anuncia una tercera raíz, reemplaza solo la pendiente, nunca la firmante. El no-op aún ejecuta los pasos 12–14 y solo actualiza el tiempo.\n";
#[test]
fn ida_vuelta_y_noop() {
    let (_, issuer, cert, cfg, local, state) = setup();
    let body = cfg.encode().unwrap();
    assert_eq!(DeviceConfig::parse(&body).unwrap().encode().unwrap(), body);
    assert_eq!(IssuerCert::parse(&cert.encode()).unwrap(), cert);
    let bundle = DeviceBundle::create(cfg.device_id, cfg.noise_pubkey, &signing_key([3; 32]));
    assert_eq!(DeviceBundle::parse(&bundle.encode()).unwrap(), bundle);
    bundle.verify().unwrap();
    assert_eq!(bundle.fingerprint().len(), 39);
    let bytes = issue_config(&issuer, &cert, &cfg).unwrap();
    let first = verify_config(&bytes, &local, &state, NOW).unwrap();
    assert!(!first.noop);
    let persisted = AgentState::from_bytes(&first.state.to_bytes()).unwrap();
    assert_eq!(persisted, first.state);
    let again = verify_config(&bytes, &local, &persisted, 1001).unwrap();
    assert!(again.noop);
    assert_eq!(again.state.max_time_observed, 1001);
    rejects(&bytes, &local, &persisted, 1230, ConfigError::Expired);
    let mut changed = cfg;
    changed.mtu = 1300;
    rejects(
        &issue_config(&issuer, &cert, &changed).unwrap(),
        &local,
        &persisted,
        NOW,
        ConfigError::ConfigRollback,
    );
}
#[test]
fn rechazo_codificacion_y_limites() {
    let (_, _, cert, mut cfg, _, state) = setup();
    let body = cfg.encode().unwrap();
    for n in 0..body.len() {
        assert!(DeviceConfig::parse(&body[..n]).is_err());
    }
    let mut b = body.clone();
    b.push(0);
    assert!(DeviceConfig::parse(&b).is_err());
    let mut b = body;
    b[2] = 1;
    assert!(DeviceConfig::parse(&b).is_err());
    cfg.tunnel_addrs.push("10.0.0.0".parse().unwrap());
    assert!(cfg.encode().is_err());
    cfg.canonicalize();
    assert!(cfg.encode().is_ok());
    cfg.tunnel_addrs.push(cfg.tunnel_addrs[0]);
    cfg.canonicalize();
    assert!(cfg.encode().is_err());
    let mut state_bytes = state.to_bytes();
    state_bytes.push(0);
    assert!(AgentState::from_bytes(&state_bytes).is_err());
    for n in 0..state.to_bytes().len() {
        assert!(AgentState::from_bytes(&state.to_bytes()[..n]).is_err());
    }
    let mut c = cert.encode().to_vec();
    c.push(0);
    assert!(IssuerCert::parse(&c).is_err());
    let p = Prefix {
        addr: "10.0.0.1".parse().unwrap(),
        len: 24,
    };
    assert!(!p.valid());
    assert!(
        Prefix {
            addr: "::".parse().unwrap(),
            len: 0
        }
        .valid()
    );
}
#[test]
fn estructura_completa() {
    let (_, _, _, base, _, _) = setup();
    for mutate in [
        |c: &mut DeviceConfig| c.peers[0].device_id = c.device_id,
        |c: &mut DeviceConfig| c.peers[0].noise_pubkey = c.noise_pubkey,
        |c: &mut DeviceConfig| c.peers[0].noise_pubkey = c.identity_pubkey,
        |c: &mut DeviceConfig| c.peers[0].addrs[0] = "10.1.0.1".parse().unwrap(),
        |c: &mut DeviceConfig| c.rules[0].src = Selector::Device([99; 16]),
        |c: &mut DeviceConfig| c.rules[0].proto = 1,
        |c: &mut DeviceConfig| c.rules[0].port_min = 444,
        |c: &mut DeviceConfig| c.mtu = 1279,
        |c: &mut DeviceConfig| c.mtu = 1501,
        |c: &mut DeviceConfig| c.config_version = 0,
        |c: &mut DeviceConfig| c.not_after = c.not_before,
        |c: &mut DeviceConfig| c.not_after = c.not_before + 3601,
        |c: &mut DeviceConfig| c.peers[0].endpoints[0].set_port(0),
        |c: &mut DeviceConfig| c.network_prefixes[0].addr = "10.0.0.1".parse().unwrap(),
    ] {
        let mut c = base.clone();
        mutate(&mut c);
        assert!(c.encode().is_err());
    }
    let mut c = base.clone();
    let mut peer = c.peers[0].clone();
    peer.device_id = [9; 16];
    peer.addrs = vec!["10.0.0.3".parse().unwrap()];
    c.peers.push(peer);
    c.canonicalize();
    assert!(c.encode().is_err());
    let mut c = base;
    c.not_after = c.not_before + 3600;
    c.mtu = 1280;
    assert!(c.encode().is_ok());
    c.mtu = 1500;
    assert!(c.encode().is_ok());
}
#[test]
fn dominios_estrictos_y_rotacion() {
    let (root, issuer, mut cert, mut cfg, local, state) = setup();
    for domain in [CONFIG_DOMAIN, BUNDLE_DOMAIN] {
        cert.signature = sign(&root, domain, &cert.encode()[..114]);
        rejects(
            &raw(&issuer, &cert, &cfg),
            &local,
            &state,
            NOW,
            ConfigError::BadCertSignature,
        );
    }
    resign(&root, &mut cert);
    let mut b = raw(&issuer, &cert, &cfg);
    let n = b.len();
    for domain in [CERT_DOMAIN, BUNDLE_DOMAIN] {
        b[n - 64..].copy_from_slice(&sign(&issuer, domain, &cfg.encode_wire()));
        rejects(&b, &local, &state, NOW, ConfigError::BadConfigSignature);
    }
    let mut bundle = DeviceBundle::create(cfg.device_id, cfg.noise_pubkey, &issuer);
    for domain in [CERT_DOMAIN, CONFIG_DOMAIN] {
        bundle.proof = sign(&issuer, domain, &bundle.encode()[..82]);
        assert_eq!(bundle.verify(), Err(Error::BadSignature));
    }
    let next = signing_key([9; 32]);
    cert.next_anchor = next.verifying_key().to_bytes();
    resign(&root, &mut cert);
    let s = verify_config(&raw(&issuer, &cert, &cfg), &local, &state, NOW)
        .unwrap()
        .state;
    assert_eq!(s.anchors.len(), 2);
    cfg.config_version += 1;
    let c = IssuerCert::issue(
        &next,
        cfg.network_id,
        2,
        800,
        2000,
        issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap();
    let s = verify_config(&raw(&issuer, &c, &cfg), &local, &s, NOW)
        .unwrap()
        .state;
    assert_eq!(s.anchors, vec![next.verifying_key().to_bytes()]);
    cfg.config_version += 1;
    rejects(
        &raw(&issuer, &cert, &cfg),
        &local,
        &s,
        NOW,
        ConfigError::UnknownRoot,
    );
}
#[test]
fn fronteras_tiempo_y_precedencia() {
    let (root, issuer, mut cert, cfg, local, mut state) = setup();
    state.max_time_observed = 0;
    let b = raw(&issuer, &cert, &cfg);
    assert!(verify_config(&b, &local, &state, 870).is_ok());
    assert!(verify_config(&b, &local, &state, 1229).is_ok());
    rejects(&b, &local, &state, 869, ConfigError::NotYetValid);
    rejects(&b, &local, &state, 1230, ConfigError::Expired);
    cert.not_before = 900;
    cert.not_after = 1200;
    resign(&root, &mut cert);
    let b = raw(&issuer, &cert, &cfg);
    assert!(verify_config(&b, &local, &state, 870).is_ok());
    rejects(&b, &local, &state, 869, ConfigError::CertNotYetValid);
    rejects(&b, &local, &state, 1230, ConfigError::CertExpired);
    state.max_time_observed = 1000;
    let b = raw(&issuer, &cert, &cfg);
    assert!(verify_config(&b, &local, &state, 970).is_ok());
    rejects(&b, &local, &state, 969, ConfigError::ClockAnomaly);
    cert.issuer_serial = state.max_issuer_serial;
    let mut cfg = cfg;
    cfg.issuer_serial = cert.issuer_serial;
    resign(&root, &mut cert);
    assert!(verify_config(&raw(&issuer, &cert, &cfg), &local, &state, NOW).is_ok());
    let mut b = raw(&issuer, &cert, &cfg);
    b[4] = 2;
    b[183..187].copy_from_slice(&((MAX_BODY_LEN + 1) as u32).to_be_bytes());
    rejects(&b, &local, &state, NOW, ConfigError::TooLarge);
}
proptest! {
 #[test] fn parsers_no_panico(b in proptest::collection::vec(any::<u8>(),0..4096)) {
  let (_,_,_,_,local,state)=setup();let _=verify_config(&b,&local,&state,NOW);let _=IssuerCert::parse(&b);let _=DeviceBundle::parse(&b);let _=AgentState::from_bytes(&b);if let Ok(c)=DeviceConfig::parse(&b) {prop_assert_eq!(c.encode().unwrap(),b);}
 }
 #[test] fn vuelta_estado(t in any::<u64>(),version in any::<u64>(),serial in any::<u64>()) {let (_,_,_,_,_,mut s)=setup();s.max_time_observed=t;s.max_config_version=version;s.max_issuer_serial=serial;prop_assert_eq!(AgentState::from_bytes(&s.to_bytes()).unwrap(),s);}
}

#[test]
fn listas_reservados_y_contadores() {
    let (root, issuer, cert, base, local, state) = setup();
    let mut c = base.clone();
    c.tunnel_addrs = vec!["10.0.0.1".parse().unwrap(), "10.0.0.3".parse().unwrap()];
    c.canonicalize();
    let mut bad = c.clone();
    bad.tunnel_addrs.reverse();
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = c.clone();
    bad.tunnel_addrs[1] = bad.tunnel_addrs[0];
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = c.clone();
    bad.network_prefixes.push(bad.network_prefixes[0].clone());
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = c.clone();
    bad.peers.push(bad.peers[0].clone());
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = c.clone();
    bad.rules.push(bad.rules[0].clone());
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = c.clone();
    let endpoint = bad.peers[0].endpoints[0];
    bad.peers[0].endpoints.push(endpoint);
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    for n in [0, 5] {
        let mut bad = base.clone();
        bad.tunnel_addrs.resize(n, "10.0.0.3".parse().unwrap());
        assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
        let mut bad = base.clone();
        bad.network_prefixes
            .resize(n, bad.network_prefixes[0].clone());
        assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
        let mut bad = base.clone();
        bad.peers[0].addrs.resize(n, "10.0.0.3".parse().unwrap());
        assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    }
    let mut bad = base.clone();
    bad.peers.resize(257, bad.peers[0].clone());
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = base.clone();
    bad.rules.resize(257, bad.rules[0].clone());
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut bad = base.clone();
    bad.peers[0]
        .endpoints
        .resize(5, "192.0.2.1:443".parse().unwrap());
    assert!(DeviceConfig::parse(&bad.encode_wire()).is_err());
    let mut b = base.encode_wire();
    // Hasta el peer: cabecera fija 133, addr propia 6, prefijo 7, contador 2, id+clave 48.
    let peer_flags = 133 + 6 + 7 + 2 + 48;
    b[peer_flags] = 1;
    assert!(DeviceConfig::parse(&b).is_err());
    let mut bundle = DeviceBundle::create(base.device_id, base.noise_pubkey, &issuer)
        .encode()
        .to_vec();
    bundle.push(0);
    assert!(DeviceBundle::parse(&bundle).is_err());
    let mut s = state.clone();
    s.anchors.clear();
    assert!(AgentState::from_bytes(&s.to_bytes()).is_err());
    s.anchors = vec![root.verifying_key().to_bytes(); 2];
    assert!(AgentState::from_bytes(&s.to_bytes()).is_err());
    s.anchors = vec![[0; 32]];
    assert!(AgentState::from_bytes(&s.to_bytes()).is_err());
    let mut c = cert;
    c.not_after = c.not_before + MAX_CERT_VALIDITY_SECS + 1;
    resign(&root, &mut c);
    rejects(
        &raw(&issuer, &c, &base),
        &local,
        &state,
        NOW,
        ConfigError::CertExpired,
    );
}

#[test]
fn extremos_u64_y_prefijos_ipv6() {
    let (root, issuer, mut cert, mut cfg, local, mut state) = setup();
    state.max_time_observed = 0;
    cert.not_before = u64::MAX - 2000;
    cert.not_after = u64::MAX;
    resign(&root, &mut cert);
    cfg.not_before = u64::MAX - 100;
    cfg.not_after = u64::MAX;
    assert!(verify_config(&raw(&issuer, &cert, &cfg), &local, &state, u64::MAX).is_ok());
    for (a, len, valid) in [
        ("2001:db8::", 32, true),
        ("2001:db8::1", 32, false),
        ("2001:db8::1", 128, true),
        ("::", 129, false),
        ("0.0.0.0", 33, false),
    ] {
        assert_eq!(
            Prefix {
                addr: a.parse().unwrap(),
                len
            }
            .valid(),
            valid
        );
    }
}
