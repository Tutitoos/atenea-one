use super::*;
use std::{fs, path::Path};

#[test]
fn inspection_existing_vectors() {
    let anchors = [
        signing_key([9; 32]).verifying_key().to_bytes(),
        signing_key([1; 32]).verifying_key().to_bytes(),
    ];
    let read = |name: &str| {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/identity")
                .join(name),
        )
        .unwrap()
    };
    assert!(inspect_config(&read("valid_config.bin"), &anchors, 1000).is_ok());
    use ConfigError::*;
    for (error, now) in [
        (TooLarge, 1000),
        (UnsupportedVersion, 1000),
        (Malformed, 1000),
        (UnknownRoot, 1000),
        (BadCertSignature, 1000),
        (CertExpired, 2030),
        (CertNotYetValid, 769),
        (BadConfigSignature, 1000),
        (ScopeMismatch, 1000),
        (Expired, 1230),
        (NotYetValid, 869),
        (InvalidStructure, 1000),
    ] {
        assert_eq!(
            inspect_config(&read(&format!("neg_{}.bin", error.code())), &anchors, now).unwrap_err(),
            error
        );
    }
    for (error, now) in [
        (IssuerRollback, 1000),
        (ConfigRollback, 1000),
        (ClockAnomaly, 969),
    ] {
        assert!(
            inspect_config(&read(&format!("neg_{}.bin", error.code())), &anchors, now).is_ok(),
            "{}",
            error.code()
        );
    }
    // Este vector también rompe el selector de dispositivo: el paso 14 sigue siendo obligatorio.
    assert_eq!(
        inspect_config(&read("neg_wrong_device.bin"), &anchors, 1000).unwrap_err(),
        InvalidStructure
    );
    let (cert, mut cfg) = inspect_config(&read("valid_config.bin"), &anchors, 1000).unwrap();
    let local = LocalKeys {
        noise_pub: cfg.noise_pubkey,
        identity_pub: cfg.identity_pubkey,
    };
    let state = AgentState::new(cfg.network_id, cfg.device_id, anchors[1]);
    cfg.device_id = [9; 16];
    cfg.rules.clear();
    let foreign = issue_config(&signing_key([2; 32]), &cert, &cfg).unwrap();
    assert_eq!(
        verify_config(&foreign, &local, &state, 1000).err(),
        Some(WrongDevice)
    );
    assert!(inspect_config(&foreign, &anchors, 1000).is_ok());
    assert_eq!(
        inspect_config(&read("valid_config.bin"), &anchors[..1], 1000).unwrap_err(),
        UnknownRoot
    );
    let unknown = [anchors[0], signing_key([8; 32]).verifying_key().to_bytes()];
    assert_eq!(
        inspect_config(&read("valid_config.bin"), &unknown, 1000).unwrap_err(),
        UnknownRoot
    );
}
