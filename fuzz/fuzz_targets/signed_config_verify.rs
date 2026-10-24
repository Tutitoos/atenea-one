//! `verify_config` sobre ficheros arbitrarios: nunca pánico; lo aceptado se re-codifica sin error
//! y el estado devuelto no retrocede.
#![no_main]
use atenea_identity::{AgentState, LocalKeys, signing_key, verify_config};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let root = signing_key([1; 32]).verifying_key().to_bytes();
    let mut state = AgentState::new([4; 16], [5; 16], root);
    state.max_config_version = 1;
    state.max_issuer_serial = 1;
    state.max_time_observed = 1000;
    let local = LocalKeys { noise_pub: [6; 32], identity_pub: signing_key([3; 32]).verifying_key().to_bytes() };
    if let Ok(v) = verify_config(data, &local, &state, 1000) {
        assert!(v.config.encode().is_ok());
        assert!(v.state.max_config_version >= state.max_config_version);
        assert!(v.state.max_issuer_serial >= state.max_issuer_serial);
        assert!(v.state.max_time_observed >= state.max_time_observed);
        assert!(v.state.anchors.len() <= 2 && !v.state.anchors.is_empty());
    }
});
