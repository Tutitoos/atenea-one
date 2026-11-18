// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! Redes gestionadas por el servidor (ADR-029). El servidor genera la raíz de la red, la guarda **cifrada** con una
//! clave de envoltura que vive en otro fichero (fuera de las copias de la base), y firma y renueva solo el certificado
//! de emisor. La raíz solo existe en claro en memoria durante la firma y se borra después. No se cambia ningún formato
//! firmado: el certificado es el de ADR-005 y los agentes lo aceptan igual que uno firmado a mano.
use crate::{AppState, api::ApiError, audit};
use atenea_identity::{IssuerCert, generate_seed, signing_key};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// Validez de cada certificado de emisor gestionado (ADR-029 D4) y fracción de vida tras la que se renueva.
const CERT_DAYS: u64 = 30;
const RENEW_AFTER_FRACTION: (u64, u64) = (2, 3);
const NONCE_LEN: usize = 24;
/// Cada cuánto revisa la tarea de renovación qué certificados están a punto de agotar su tiempo.
const RENEW_EVERY: Duration = Duration::from_secs(3600);

/// Clave de envoltura de las raíces gestionadas. `Debug` no la muestra.
#[derive(Clone)]
pub struct Wrap(XChaCha20Poly1305);

impl std::fmt::Debug for Wrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Wrap(..)")
    }
}

impl Wrap {
    /// Lee `64 hex` de un fichero regular 0400 o 0600, como la clave de emisor.
    pub fn load(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let metadata = std::fs::symlink_metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if !metadata.is_file()
                || !matches!(metadata.permissions().mode() & 0o777, 0o400 | 0o600)
            {
                return Err("La clave de envoltura debe ser un fichero regular 0400 o 0600".into());
            }
        }
        let mut text = std::fs::read_to_string(path)?;
        let decoded = hex::decode(text.trim());
        text.zeroize();
        let mut key: [u8; 32] = decoded?
            .try_into()
            .map_err(|_| "La clave de envoltura debe tener 64 dígitos hexadecimales")?;
        let wrap = Self::new(&key);
        key.zeroize();
        Ok(wrap)
    }

    pub fn new(key: &[u8; 32]) -> Self {
        Self(XChaCha20Poly1305::new(key.into()))
    }

    /// `nonce (24) || texto cifrado`. El identificador de la red va como dato asociado: una raíz copiada a la fila de
    /// otra red no descifra.
    fn seal(&self, net: Uuid, seed: &[u8; 32]) -> Result<Vec<u8>, ApiError> {
        let mut nonce = generate_seed();
        let nonce_bytes: [u8; NONCE_LEN] = nonce[..NONCE_LEN]
            .try_into()
            .map_err(|_| ApiError::internal())?;
        nonce.zeroize();
        let sealed = self
            .0
            .encrypt(
                XNonce::from_slice(&nonce_bytes),
                Payload {
                    msg: seed,
                    aad: net.as_bytes(),
                },
            )
            .map_err(|_| ApiError::internal())?;
        Ok([nonce_bytes.as_slice(), &sealed].concat())
    }

    fn open(&self, net: Uuid, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>, ApiError> {
        let (nonce, sealed) = blob
            .split_at_checked(NONCE_LEN)
            .ok_or_else(ApiError::internal)?;
        let mut plain = self
            .0
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: net.as_bytes(),
                },
            )
            .map_err(|_| ApiError::internal())?;
        let seed: Result<[u8; 32], _> = plain.as_slice().try_into();
        plain.zeroize();
        seed.map(Zeroizing::new).map_err(|_| ApiError::internal())
    }
}

/// Raíz nueva de una red gestionada: ancla pública y semilla cifrada. La semilla en claro no sale de esta función.
pub struct NewRoot {
    pub anchor: [u8; 32],
    pub wrapped: Vec<u8>,
    seed: Zeroizing<[u8; 32]>,
}

pub fn generate(wrap: &Wrap, net: Uuid) -> Result<NewRoot, ApiError> {
    let seed = Zeroizing::new(generate_seed());
    let anchor = signing_key(*seed).verifying_key().to_bytes();
    let wrapped = wrap.seal(net, &seed)?;
    Ok(NewRoot {
        anchor,
        wrapped,
        seed,
    })
}

/// Firma el certificado de emisor con la raíz y el serial dados, y lo deja en la red. La auditoría es del sistema.
async fn store(
    s: &AppState,
    tx: &mut sqlx::PgConnection,
    net: Uuid,
    org: Uuid,
    root_seed: &[u8; 32],
    serial: u64,
) -> Result<(), ApiError> {
    let now = crate::api::now() as u64;
    let cert = IssuerCert::issue(
        &signing_key(*root_seed),
        *net.as_bytes(),
        serial,
        now.saturating_sub(60),
        now + CERT_DAYS * 86_400,
        s.issuer.verifying_key().to_bytes(),
        None,
    )
    .map_err(|_| ApiError::internal())?;
    sqlx::query("UPDATE networks SET issuer_cert=$1 WHERE id=$2")
        .bind(cert.encode().as_slice())
        .bind(net)
        .execute(&mut *tx)
        .await?;
    audit::record(
        tx,
        "system",
        Uuid::nil(),
        Some(org),
        Some(net),
        "network.issuer_cert",
        net,
        json!({"serial": serial, "managed": true}),
    )
    .await
}

/// Primer certificado de una red recién creada, con la raíz que aún está en memoria.
pub async fn sign_first(
    s: &AppState,
    tx: &mut sqlx::PgConnection,
    net: Uuid,
    org: Uuid,
    root: &NewRoot,
) -> Result<(), ApiError> {
    store(s, tx, net, org, &root.seed, 1).await
}

/// ¿Ha pasado la fracción de vida tras la que se renueva?
fn due(cert: &IssuerCert, now: u64) -> bool {
    let life = cert.not_after.saturating_sub(cert.not_before);
    now >= cert.not_before + life * RENEW_AFTER_FRACTION.0 / RENEW_AFTER_FRACTION.1
}

/// Renueva el certificado de una red gestionada con un serial mayor. Descifra la raíz solo aquí.
async fn renew(s: &AppState, wrap: &Wrap, net: Uuid) -> Result<(), ApiError> {
    let mut tx = s.pool.begin().await?;
    let row: Option<(Uuid, Vec<u8>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT org_id, root_wrapped, issuer_cert FROM networks WHERE id=$1 AND root_mode='managed' FOR UPDATE",
    )
    .bind(net)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((org, wrapped, installed)) = row else {
        return Ok(());
    };
    let current = installed.as_deref().and_then(|b| IssuerCert::parse(b).ok());
    // Otra instancia pudo renovarla entre la lectura y el bloqueo.
    if current
        .as_ref()
        .is_some_and(|c| !due(c, crate::api::now() as u64))
    {
        return Ok(());
    }
    let seed = wrap.open(net, &wrapped)?;
    let serial = current.map_or(1, |c| c.issuer_serial + 1);
    store(s, &mut tx, net, org, &seed, serial).await?;
    tx.commit().await?;
    s.panel.changed(net, "network");
    s.panel.changed(net, "audit");
    Ok(())
}

/// Un barrido: renueva las redes gestionadas sin certificado o con el certificado en su último tercio.
pub async fn renew_due(s: &AppState) -> Result<usize, ApiError> {
    let Some(wrap) = s.wrap.as_deref() else {
        return Ok(0);
    };
    let rows: Vec<(Uuid, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT id, issuer_cert FROM networks WHERE root_mode='managed'")
            .fetch_all(&s.pool)
            .await?;
    let now = crate::api::now() as u64;
    let mut renewed = 0;
    for (net, installed) in rows {
        let must = installed
            .as_deref()
            .and_then(|b| IssuerCert::parse(b).ok())
            .is_none_or(|c| due(&c, now));
        if !must {
            continue;
        }
        // Un fallo en una red no impide renovar las demás; el siguiente barrido lo reintenta.
        match renew(s, wrap, net).await {
            Ok(()) => renewed += 1,
            Err(_) => {
                tracing::warn!("No se pudo renovar el certificado de emisor de una red gestionada")
            }
        }
    }
    Ok(renewed)
}

pub async fn renewer(s: AppState, mut stop: tokio::sync::watch::Receiver<bool>) {
    let mut timer = tokio::time::interval(RENEW_EVERY);
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = timer.tick() => {
                if renew_due(&s).await.is_err() {
                    tracing::warn!("No se pudo revisar la renovación de certificados gestionados");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_raiz_cifrada_solo_abre_con_su_clave_y_su_red() {
        let wrap = Wrap::new(&[7; 32]);
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let root = generate(&wrap, a).unwrap();
        assert!(
            !root.wrapped.windows(32).any(|w| w == root.seed.as_slice()),
            "la semilla no aparece en claro"
        );
        assert_eq!(*wrap.open(a, &root.wrapped).unwrap(), *root.seed);
        assert!(
            wrap.open(b, &root.wrapped).is_err(),
            "la fila de otra red no descifra"
        );
        assert!(
            Wrap::new(&[8; 32]).open(a, &root.wrapped).is_err(),
            "otra clave no descifra"
        );
        let mut tampered = root.wrapped.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(wrap.open(a, &tampered).is_err(), "un cambio se detecta");
        assert!(wrap.open(a, &[0; 5]).is_err());
    }

    #[test]
    fn el_certificado_se_renueva_en_su_ultimo_tercio() {
        let key = signing_key([9; 32]);
        let cert = IssuerCert::issue(
            &key,
            [1; 16],
            1,
            0,
            3000,
            key.verifying_key().to_bytes(),
            None,
        )
        .unwrap();
        assert!(!due(&cert, 1999));
        assert!(due(&cert, 2000));
    }
}
