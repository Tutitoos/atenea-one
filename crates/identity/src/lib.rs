//! Configuración firmada v1: codificación única y verificación sin E/S ni reloj propio.
mod codec;
mod config;
mod verify;
use blake2::{Blake2s256, Digest};
use codec::Reader;
use ed25519_dalek::{Signature, Signer};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
use std::{
    fmt,
    net::{IpAddr, SocketAddr},
};
pub use verify::verify_config;

pub type NetworkId = [u8; 16];
pub type DeviceId = [u8; 16];
pub type PublicKey = [u8; 32];
pub const MAX_BODY_LEN: usize = 131072;
pub const MAX_CONFIG_VALIDITY_SECS: u64 = 3600;
pub const MAX_CERT_VALIDITY_SECS: u64 = 90 * 86400;
pub const CLOCK_TOLERANCE_SECS: u64 = 30;
const CERT_DOMAIN: &[u8] = b"atenea-one/issuer-cert/v1\0";
const CONFIG_DOMAIN: &[u8] = b"atenea-one/config/v1\0";
const BUNDLE_DOMAIN: &[u8] = b"atenea-one/device-bundle/v1\0";

pub fn generate_seed() -> [u8; 32] {
    let mut b = [0; 32];
    getrandom::fill(&mut b).expect("RNG del sistema");
    b
}
pub fn signing_key(seed: [u8; 32]) -> SigningKey {
    SigningKey::from_bytes(&seed)
}
fn hash(b: &[u8]) -> [u8; 32] {
    Blake2s256::digest(b).into()
}
pub fn key_id(public: &PublicKey) -> [u8; 8] {
    hash(public)[..8].try_into().expect("longitud fija")
}
fn message(domain: &[u8], b: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(domain.len() + b.len());
    m.extend_from_slice(domain);
    m.extend_from_slice(b);
    m
}
fn sign(key: &SigningKey, domain: &[u8], b: &[u8]) -> [u8; 64] {
    key.sign(&message(domain, b)).to_bytes()
}
fn check(key: &PublicKey, domain: &[u8], b: &[u8], sig: &[u8; 64]) -> Result<(), Error> {
    VerifyingKey::from_bytes(key)
        .map_err(|_| Error::BadSignature)?
        .verify_strict(&message(domain, b), &Signature::from_bytes(sig))
        .map_err(|_| Error::BadSignature)
}
fn valid_anchor(k: &PublicKey) -> bool {
    *k != [0; 32] && VerifyingKey::from_bytes(k).is_ok_and(|v| !v.is_weak())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    pub addr: IpAddr,
    pub len: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub device_id: DeviceId,
    pub noise_pubkey: PublicKey,
    pub addrs: Vec<IpAddr>,
    pub endpoints: Vec<SocketAddr>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    Any,
    Device(DeviceId),
    Prefix(Prefix),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub src: Selector,
    pub dst: Selector,
    pub proto: u8,
    pub port_min: u16,
    pub port_max: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceConfig {
    pub network_id: NetworkId,
    pub device_id: DeviceId,
    pub issuer_serial: u64,
    pub config_version: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub noise_pubkey: PublicKey,
    pub identity_pubkey: PublicKey,
    pub mtu: u16,
    pub tunnel_addrs: Vec<IpAddr>,
    pub network_prefixes: Vec<Prefix>,
    pub peers: Vec<Peer>,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuerCert {
    pub root_key_id: [u8; 8],
    pub network_id: NetworkId,
    pub issuer_serial: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub issuer_pubkey: PublicKey,
    pub next_anchor: PublicKey,
    pub signature: [u8; 64],
}
impl IssuerCert {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        root: &SigningKey,
        network_id: NetworkId,
        issuer_serial: u64,
        not_before: u64,
        not_after: u64,
        issuer_pubkey: PublicKey,
        next_anchor: Option<PublicKey>,
    ) -> Result<Self, Error> {
        let next_anchor = next_anchor.unwrap_or([0; 32]);
        if not_after <= not_before
            || not_after - not_before > MAX_CERT_VALIDITY_SECS
            || (next_anchor != [0; 32] && !valid_anchor(&next_anchor))
        {
            return Err(Error::InvalidStructure);
        }
        let mut c = Self {
            root_key_id: key_id(&root.verifying_key().to_bytes()),
            network_id,
            issuer_serial,
            not_before,
            not_after,
            issuer_pubkey,
            next_anchor,
            signature: [0; 64],
        };
        c.signature = sign(root, CERT_DOMAIN, &c.encode()[..114]);
        Ok(c)
    }
    pub fn encode(&self) -> [u8; 178] {
        let mut b = Vec::with_capacity(178);
        b.extend_from_slice(&[1, 1]);
        b.extend_from_slice(&self.root_key_id);
        b.extend_from_slice(&self.network_id);
        for n in [self.issuer_serial, self.not_before, self.not_after] {
            b.extend_from_slice(&n.to_be_bytes());
        }
        b.extend_from_slice(&self.issuer_pubkey);
        b.extend_from_slice(&self.next_anchor);
        b.extend_from_slice(&self.signature);
        b.try_into().expect("longitud fija")
    }
    pub fn parse(b: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(b);
        r.header(1)?;
        let c = Self {
            root_key_id: r.array()?,
            network_id: r.array()?,
            issuer_serial: r.u64()?,
            not_before: r.u64()?,
            not_after: r.u64()?,
            issuer_pubkey: r.array()?,
            next_anchor: r.array()?,
            signature: r.array()?,
        };
        r.end()?;
        Ok(c)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceBundle {
    pub device_id: DeviceId,
    pub noise_pubkey: PublicKey,
    pub identity_pubkey: PublicKey,
    pub proof: [u8; 64],
}
impl DeviceBundle {
    pub fn create(device_id: DeviceId, noise_pubkey: PublicKey, identity: &SigningKey) -> Self {
        let mut b = Self {
            device_id,
            noise_pubkey,
            identity_pubkey: identity.verifying_key().to_bytes(),
            proof: [0; 64],
        };
        b.proof = sign(identity, BUNDLE_DOMAIN, &b.encode()[..82]);
        b
    }
    pub fn encode(&self) -> [u8; 146] {
        let mut b = [0; 146];
        b[..2].copy_from_slice(&[1, 3]);
        b[2..18].copy_from_slice(&self.device_id);
        b[18..50].copy_from_slice(&self.noise_pubkey);
        b[50..82].copy_from_slice(&self.identity_pubkey);
        b[82..].copy_from_slice(&self.proof);
        b
    }
    pub fn parse(b: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(b);
        r.header(3)?;
        let b = Self {
            device_id: r.array()?,
            noise_pubkey: r.array()?,
            identity_pubkey: r.array()?,
            proof: r.array()?,
        };
        r.end()?;
        Ok(b)
    }
    pub fn verify(&self) -> Result<(), Error> {
        check(
            &self.identity_pubkey,
            BUNDLE_DOMAIN,
            &self.encode()[..82],
            &self.proof,
        )
    }
    pub fn fingerprint(&self) -> String {
        let h = hash(&self.encode());
        h[..16]
            .chunks(2)
            .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
            .collect::<Vec<_>>()
            .join("-")
    }
}
pub fn issue_config(
    issuer: &SigningKey,
    cert: &IssuerCert,
    cfg: &DeviceConfig,
) -> Result<Vec<u8>, Error> {
    if cert.issuer_pubkey != issuer.verifying_key().to_bytes()
        || cert.network_id != cfg.network_id
        || cert.issuer_serial != cfg.issuer_serial
    {
        return Err(Error::InvalidStructure);
    }
    let body = cfg.encode()?;
    let mut b = Vec::with_capacity(251 + body.len());
    b.extend_from_slice(b"AOCF\x01");
    b.extend_from_slice(&cert.encode());
    b.extend_from_slice(&(body.len() as u32).to_be_bytes());
    b.extend_from_slice(&body);
    b.extend_from_slice(&sign(issuer, CONFIG_DOMAIN, &body));
    Ok(b)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentState {
    pub network_id: NetworkId,
    pub device_id: DeviceId,
    pub anchors: Vec<PublicKey>,
    pub max_config_version: u64,
    pub max_issuer_serial: u64,
    pub max_time_observed: u64,
    pub last_config_hash: [u8; 32],
}
impl AgentState {
    pub fn new(network_id: NetworkId, device_id: DeviceId, anchor: PublicKey) -> Self {
        Self {
            network_id,
            device_id,
            anchors: vec![anchor],
            max_config_version: 0,
            max_issuer_serial: 0,
            max_time_observed: 0,
            last_config_hash: [0; 32],
        }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = b"AOST\x01".to_vec();
        b.extend_from_slice(&self.network_id);
        b.extend_from_slice(&self.device_id);
        b.push(self.anchors.len() as u8);
        for a in &self.anchors {
            b.extend_from_slice(a);
        }
        for n in [
            self.max_config_version,
            self.max_issuer_serial,
            self.max_time_observed,
        ] {
            b.extend_from_slice(&n.to_be_bytes());
        }
        b.extend_from_slice(&self.last_config_hash);
        b
    }
    pub fn from_bytes(b: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(b);
        if r.array::<4>()? != *b"AOST" || r.u8()? != 1 {
            return Err(Error::UnsupportedVersion);
        }
        let network_id = r.array()?;
        let device_id = r.array()?;
        let n = r.count8(1, 2)?;
        let mut anchors = Vec::with_capacity(n);
        for _ in 0..n {
            let a = r.array()?;
            if !valid_anchor(&a) || anchors.contains(&a) {
                return Err(Error::Malformed);
            }
            anchors.push(a);
        }
        let s = Self {
            network_id,
            device_id,
            anchors,
            max_config_version: r.u64()?,
            max_issuer_serial: r.u64()?,
            max_time_observed: r.u64()?,
            last_config_hash: r.array()?,
        };
        r.end()?;
        Ok(s)
    }
}
pub struct LocalKeys {
    pub noise_pub: PublicKey,
    pub identity_pub: PublicKey,
}
pub struct Verified {
    pub config: DeviceConfig,
    pub state: AgentState,
    pub noop: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Malformed,
    UnsupportedVersion,
    TooLarge,
    InvalidStructure,
    BadSignature,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "codificación inválida",
            Self::UnsupportedVersion => "versión o tipo no admitido",
            Self::TooLarge => "mensaje demasiado grande",
            Self::InvalidStructure => "estructura inválida",
            Self::BadSignature => "firma inválida",
        })
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
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
}
impl ConfigError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::UnsupportedVersion => "unsupported_version",
            Self::Malformed => "malformed",
            Self::UnknownRoot => "unknown_root",
            Self::BadCertSignature => "bad_cert_signature",
            Self::CertExpired => "cert_expired",
            Self::CertNotYetValid => "cert_not_yet_valid",
            Self::IssuerRollback => "issuer_rollback",
            Self::BadConfigSignature => "bad_config_signature",
            Self::ScopeMismatch => "scope_mismatch",
            Self::WrongDevice => "wrong_device",
            Self::ConfigRollback => "config_rollback",
            Self::Expired => "expired",
            Self::NotYetValid => "not_yet_valid",
            Self::ClockAnomaly => "clock_anomaly",
            Self::InvalidStructure => "invalid_structure",
        }
    }
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for ConfigError {}
#[cfg(test)]
mod tests;
