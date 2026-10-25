//! `verify_config` con firmas válidas: la entrada son parámetros del certificado, del estado y del
//! CUERPO de la configuración; el arnés firma con claves fijas de fuzz, de modo que cada mutación
//! llega a los pasos posteriores a la firma (parseo del cuerpo, ámbito, antirrollback, lease,
//! rotación de anclas, estructura). Con la firma rota nada pasaría del paso 4.
//!
//! Diseño de la entrada: 57 bytes de control (serie, validez del certificado, versiones y reloj del
//! estado, `now`, banderas) + cuerpo crudo. Las claves son fijas y públicas: solo para fuzzing.
#![no_main]
use atenea_identity::{AgentState, IssuerCert, LocalKeys, SigningKey, signing_key, verify_config};
use ed25519_dalek::Signer;
use libfuzzer_sys::fuzz_target;

const CONTROL: usize = 57;
const NETWORK: [u8; 16] = [4; 16];
const DEVICE: [u8; 16] = [5; 16];
const CONFIG_DOMAIN: &[u8] = b"atenea-one/config/v1\0";

fn u64_at(d: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(d[at..at + 8].try_into().unwrap())
}

/// Firma `body` como lo hace `issue_config`, pero sin exigir que el cuerpo sea válido.
fn file(issuer: &SigningKey, cert: &IssuerCert, body: &[u8]) -> Vec<u8> {
    let mut m = CONFIG_DOMAIN.to_vec();
    m.extend_from_slice(body);
    let mut b = b"AOCF\x01".to_vec();
    b.extend_from_slice(&cert.encode());
    b.extend_from_slice(&(body.len() as u32).to_be_bytes());
    b.extend_from_slice(body);
    b.extend_from_slice(&issuer.sign(&m).to_bytes());
    b
}

fuzz_target!(|data: &[u8]| {
    if data.len() < CONTROL {
        return;
    }
    let (root, issuer, rotated) = (signing_key([1; 32]), signing_key([2; 32]), signing_key([9; 32]));
    let root_pub = root.verifying_key().to_bytes();
    let identity = signing_key([3; 32]).verifying_key().to_bytes();
    let flags = data[56];

    let not_before = u64_at(data, 8);
    let not_after = not_before.saturating_add(u64_at(data, 16) % (90 * 86400 + 2));
    let next = (flags & 1 != 0).then(|| rotated.verifying_key().to_bytes());
    // Un certificado que `issue` rechaza no llega a firmarse: es el comportamiento correcto.
    let Ok(cert) = IssuerCert::issue(&root, NETWORK, u64_at(data, 0), not_before, not_after, issuer.verifying_key().to_bytes(), next) else {
        return;
    };

    let mut state = AgentState::new(NETWORK, DEVICE, root_pub);
    if flags & 2 != 0 {
        state.anchors.push(rotated.verifying_key().to_bytes());
    }
    state.max_config_version = u64_at(data, 24);
    state.max_issuer_serial = u64_at(data, 32);
    state.max_time_observed = u64_at(data, 40);
    let now = u64_at(data, 48);
    let local = LocalKeys { noise_pub: [6; 32], identity_pub: identity };

    let signed = file(&issuer, &cert, &data[CONTROL..]);
    let Ok(v) = verify_config(&signed, &local, &state, now) else { return };

    // Lo aceptado es canónico, es para este dispositivo y no hace retroceder nada.
    assert_eq!(v.config.encode().unwrap(), &data[CONTROL..], "el cuerpo aceptado no es canónico");
    assert_eq!(v.config.network_id, NETWORK);
    assert_eq!(v.config.device_id, DEVICE);
    assert_eq!(v.config.noise_pubkey, local.noise_pub);
    assert_eq!(v.config.identity_pubkey, local.identity_pub);
    assert_eq!(v.config.issuer_serial, cert.issuer_serial);
    assert!(v.config.config_version >= state.max_config_version, "config más antigua aceptada");
    assert!(cert.issuer_serial >= state.max_issuer_serial, "serie de emisor más antigua aceptada");
    assert!(v.state.max_config_version >= state.max_config_version);
    assert!(v.state.max_issuer_serial >= state.max_issuer_serial);
    assert!(v.state.max_time_observed >= state.max_time_observed);
    assert!(v.config.not_before <= now.saturating_add(30) && now < v.config.not_after.saturating_add(30), "fuera del lease");
    // Las anclas solo cambian por rotación anunciada y siempre queda al menos una.
    assert!((1..=2).contains(&v.state.anchors.len()));
    assert!(v.state.anchors.iter().all(|a| state.anchors.contains(a) || Some(*a) == next));
    // El estado resultante se puede persistir y volver a cargar.
    assert!(AgentState::from_bytes(&v.state.to_bytes()).is_ok_and(|s| s == v.state));
    // Reaplicar la misma configuración con el estado nuevo es idempotente (nunca un rollback).
    let again = verify_config(&signed, &local, &v.state, now).expect("la repetición exacta debe aceptarse");
    assert_eq!(again.config, v.config);
    assert_eq!(again.state, v.state);
});
