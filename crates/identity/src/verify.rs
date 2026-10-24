use crate::*;
fn parsed_error(e: Error) -> ConfigError {
    match e {
        Error::UnsupportedVersion => ConfigError::UnsupportedVersion,
        Error::TooLarge => ConfigError::TooLarge,
        _ => ConfigError::Malformed,
    }
}
fn expired(now: u64, end: u64) -> bool {
    u128::from(now) >= u128::from(end) + u128::from(CLOCK_TOLERANCE_SECS)
}
fn early(now: u64, start: u64) -> bool {
    now < start.saturating_sub(CLOCK_TOLERANCE_SECS)
}
fn inspect_chain(
    file: &[u8],
    anchors: &[PublicKey],
    now: u64,
    min_serial: Option<u64>,
) -> Result<(usize, IssuerCert, DeviceConfig), ConfigError> {
    use ConfigError::*;
    // 1: el tamaño declarado se comprueba incluso antes de despachar la versión.
    if file.len() > 251 + MAX_BODY_LEN {
        return Err(TooLarge);
    }
    let declared = file
        .get(183..187)
        .map(|b| u32::from_be_bytes(b.try_into().expect("longitud fija")) as usize);
    if declared.is_some_and(|n| n > MAX_BODY_LEN) {
        return Err(TooLarge);
    }
    // 2: todas las cabeceras conocidas preceden al parseo estricto.
    if file.get(..4).is_some_and(|b| b != b"AOCF")
        || file.get(4).is_some_and(|v| *v != 1)
        || file.get(5..7).is_some_and(|b| b != [1, 1])
        || file.get(187..189).is_some_and(|b| b != [1, 2])
    {
        return Err(UnsupportedVersion);
    }
    // 3: longitudes exactas, contadores, orden, reservados y rangos del formato.
    let n = declared.ok_or(Malformed)?;
    if file.len() != 251 + n {
        return Err(Malformed);
    }
    let cert = IssuerCert::parse(&file[5..183]).map_err(parsed_error)?;
    if cert.next_anchor != [0; 32] && !valid_anchor(&cert.next_anchor) {
        return Err(Malformed);
    }
    let body = &file[187..187 + n];
    let cfg = DeviceConfig::parse_wire(body).map_err(parsed_error)?;
    let signature = file[187 + n..].try_into().map_err(|_| Malformed)?;
    // 4–8: cadena de confianza y serial, antes de confiar en el cuerpo.
    let root = anchors
        .iter()
        .position(|a| key_id(a) == cert.root_key_id)
        .ok_or(UnknownRoot)?;
    check(&anchors[root], CERT_DOMAIN, &file[5..119], &cert.signature)
        .map_err(|_| BadCertSignature)?;
    if cert.not_after <= cert.not_before
        || cert.not_after - cert.not_before > MAX_CERT_VALIDITY_SECS
        || expired(now, cert.not_after)
    {
        return Err(CertExpired);
    }
    if early(now, cert.not_before) {
        return Err(CertNotYetValid);
    }
    if min_serial.is_some_and(|serial| cert.issuer_serial < serial) {
        return Err(IssuerRollback);
    }
    check(&cert.issuer_pubkey, CONFIG_DOMAIN, body, &signature).map_err(|_| BadConfigSignature)?;
    Ok((root, cert, cfg))
}

/// Inspecciona la cadena y el cuerpo sin identidad local, persistencia ni protección antirrollback.
pub fn inspect_config(
    file: &[u8],
    anchors: &[PublicKey],
    now: u64,
) -> Result<(IssuerCert, DeviceConfig), ConfigError> {
    let (_, cert, cfg) = inspect_chain(file, anchors, now, None)?;
    if cfg.network_id != cert.network_id || cfg.issuer_serial != cert.issuer_serial {
        return Err(ConfigError::ScopeMismatch);
    }
    check_lease(&cfg, now)?;
    if !cfg.semantic_valid() {
        return Err(ConfigError::InvalidStructure);
    }
    Ok((cert, cfg))
}

fn check_lease(cfg: &DeviceConfig, now: u64) -> Result<(), ConfigError> {
    if expired(now, cfg.not_after) {
        return Err(ConfigError::Expired);
    }
    if early(now, cfg.not_before) {
        return Err(ConfigError::NotYetValid);
    }
    Ok(())
}

/// Verifica los catorce pasos sin modificar el estado recibido. El llamador persiste antes de aplicar.
pub fn verify_config(
    file: &[u8],
    local: &LocalKeys,
    state: &AgentState,
    now: u64,
) -> Result<Verified, ConfigError> {
    use ConfigError::*;
    let (root, cert, cfg) =
        inspect_chain(file, &state.anchors, now, Some(state.max_issuer_serial))?;
    // 9–11: ámbito, identidad y protección contra rollback (el hash cubre TODO el fichero).
    if cfg.network_id != cert.network_id
        || cfg.issuer_serial != cert.issuer_serial
        || cfg.network_id != state.network_id
    {
        return Err(ScopeMismatch);
    }
    if cfg.device_id != state.device_id
        || cfg.noise_pubkey != local.noise_pub
        || cfg.identity_pubkey != local.identity_pub
    {
        return Err(WrongDevice);
    }
    let digest = hash(file);
    let noop = cfg.config_version == state.max_config_version && digest == state.last_config_hash;
    if cfg.config_version < state.max_config_version
        || (cfg.config_version == state.max_config_version && !noop)
    {
        return Err(ConfigRollback);
    }
    // 12–14: un no-op también caduca y no evita la comprobación del reloj.
    check_lease(&cfg, now)?;
    if early(now, state.max_time_observed) {
        return Err(ClockAnomaly);
    }
    if !cfg.semantic_valid() {
        return Err(InvalidStructure);
    }
    let mut updated = state.clone();
    updated.max_time_observed = updated.max_time_observed.max(now);
    if !noop {
        updated.max_config_version = cfg.config_version;
        updated.max_issuer_serial = cert.issuer_serial;
        updated.last_config_hash = digest;
        // Primero retirar la raíz anterior; luego anunciar la siguiente sin desplazar la raíz firmante.
        if root + 1 == updated.anchors.len() && root > 0 {
            updated.anchors.remove(0);
        }
        if cert.next_anchor != [0; 32] && !updated.anchors.contains(&cert.next_anchor) {
            if updated.anchors.len() == 2 {
                updated.anchors.pop();
            }
            updated.anchors.push(cert.next_anchor);
        }
    }
    Ok(Verified {
        config: cfg,
        state: updated,
        noop,
    })
}
