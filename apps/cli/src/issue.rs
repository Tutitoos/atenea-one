//! Emisión offline de claves y configuraciones firmadas (contrato `signed-config-v1`).
//! Corre en la estación de administración o en el emisor, nunca en el agente.

use atenea_crypto::{StaticKeypair, hex};
use atenea_identity::{
    DeviceBundle, DeviceConfig, DeviceId, IssuerCert, Peer, Prefix, Rule, Selector, SigningKey,
    generate_seed, issue_config, signing_key,
};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Escribe un fichero nuevo con permisos 0600; no sobrescribe.
pub(crate) fn write_secret(path: &Path, content: &str) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{content}").map_err(|e| e.to_string())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(bytes).map_err(|e| e.to_string())
}

/// Escritura atómica (temporal + rename): el agente nunca lee un fichero a medias.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_seed(path: &Path) -> Result<SigningKey, String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(format!(
            "{}: permisos demasiado abiertos (se exige 0600)",
            path.display()
        ));
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(signing_key(hex::decode::<32>(&text).ok_or_else(|| {
        format!("{}: se esperan 64 dígitos hex", path.display())
    })?))
}

pub fn root_init(out: &Path) -> Result<(), String> {
    let seed = generate_seed();
    write_secret(out, &hex::encode(&seed))?;
    println!(
        "{}",
        hex::encode(&signing_key(seed).verifying_key().to_bytes())
    );
    Ok(())
}

pub fn issuer_init(
    root: &Path,
    network_id: &str,
    serial: u64,
    valid_days: u64,
    next_anchor: Option<&str>,
    out_dir: &Path,
) -> Result<(), String> {
    let root = read_seed(root)?;
    let network_id =
        hex::decode::<16>(network_id).ok_or("network-id: se esperan 32 dígitos hex")?;
    let next = match next_anchor {
        Some(h) => Some(hex::decode::<32>(h).ok_or("next-anchor: se esperan 64 dígitos hex")?),
        None => None,
    };
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let seed = generate_seed();
    let issuer_pub = signing_key(seed).verifying_key().to_bytes();
    let now = now_secs();
    let cert = IssuerCert::issue(
        &root,
        network_id,
        serial,
        now,
        now + valid_days * 86400,
        issuer_pub,
        next,
    )
    .map_err(|e| format!("certificado: {e:?}"))?;
    write_secret(&out_dir.join("issuer.key"), &hex::encode(&seed))?;
    write_new(&out_dir.join("issuer.cert"), &cert.encode())?;
    println!("{}", hex::encode(&issuer_pub));
    Ok(())
}
#[derive(clap::Args)]
pub struct IssuerCertArgs {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    issuer_key: PathBuf,
    #[arg(long)]
    network_id: String,
    #[arg(long)]
    serial: u64,
    #[arg(long)]
    valid_days: u64,
    #[arg(long)]
    next_anchor: Option<String>,
    #[arg(long)]
    out: PathBuf,
}

pub fn issuer_cert(args: IssuerCertArgs) -> Result<(), String> {
    let root = read_seed(&args.root)?;
    let issuer = read_seed(&args.issuer_key)?;
    let network =
        hex::decode::<16>(&args.network_id).ok_or("network-id: se esperan 32 dígitos hex")?;
    if !(1..=90).contains(&args.valid_days) {
        return Err("valid-days debe estar entre 1 y 90".into());
    }
    let next = args
        .next_anchor
        .as_deref()
        .map(|s| hex::decode::<32>(s).ok_or("next-anchor: se esperan 64 dígitos hex"))
        .transpose()?;
    let now = now_secs();
    let cert = IssuerCert::issue(
        &root,
        network,
        args.serial,
        now,
        now + args.valid_days * 86400,
        issuer.verifying_key().to_bytes(),
        next,
    )
    .map_err(|e| format!("certificado: {e:?}"))?;
    write_new(&args.out, &cert.encode())
}

pub fn device_init(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let noise = StaticKeypair::generate();
    let seed = generate_seed();
    let identity = signing_key(seed);
    let mut device_id = [0u8; 16];
    device_id.copy_from_slice(&generate_seed()[..16]);
    let bundle = DeviceBundle::create(device_id, noise.public, &identity);
    write_secret(&dir.join("noise.key"), &hex::encode(noise.private_bytes()))?;
    write_secret(&dir.join("identity.key"), &hex::encode(&seed))?;
    write_new(&dir.join("bundle.bin"), &bundle.encode())?;
    println!("device_id={}", hex::encode(&device_id));
    println!("noise_public={}", hex::encode(&noise.public));
    println!("fingerprint={}", bundle.fingerprint());
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    network_id: String,
    device_bundle: PathBuf,
    config_version: Option<u64>,
    not_before: Option<u64>,
    validity_secs: u64,
    mtu: Option<u16>,
    tunnel_addrs: Vec<IpAddr>,
    network_prefixes: Vec<String>,
    #[serde(default, rename = "peer")]
    peers: Vec<SpecPeer>,
    #[serde(default, rename = "rule")]
    rules: Vec<SpecRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecPeer {
    bundle: PathBuf,
    addrs: Vec<IpAddr>,
    #[serde(default)]
    endpoints: Vec<SocketAddr>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecRule {
    src: String,
    dst: String,
    proto: Option<String>,
    ports: Option<String>,
}

fn load_bundle(path: &Path) -> Result<DeviceBundle, String> {
    let b = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bundle = DeviceBundle::parse(&b).map_err(|e| format!("{}: {e:?}", path.display()))?;
    bundle
        .verify()
        .map_err(|_| format!("{}: prueba de posesión inválida", path.display()))?;
    eprintln!(
        "  paquete {} huella {}  (verifíquela por otro canal)",
        path.display(),
        bundle.fingerprint()
    );
    Ok(bundle)
}

fn parse_prefix(s: &str) -> Result<Prefix, String> {
    let (a, l) = s
        .split_once('/')
        .ok_or_else(|| format!("prefijo sin /longitud: {s}"))?;
    Ok(Prefix {
        addr: a.parse().map_err(|_| format!("dirección inválida: {a}"))?,
        len: l.parse().map_err(|_| format!("longitud inválida: {l}"))?,
    })
}

fn parse_selector(s: &str, own: &DeviceId, peers: &[DeviceId]) -> Result<Selector, String> {
    match s {
        "any" => Ok(Selector::Any),
        "self" => Ok(Selector::Device(*own)),
        _ => {
            if let Some(n) = s.strip_prefix("peer:") {
                let i: usize = n.parse().map_err(|_| format!("selector inválido: {s}"))?;
                return peers
                    .get(i)
                    .map(|d| Selector::Device(*d))
                    .ok_or_else(|| format!("peer inexistente: {s}"));
            }
            if let Some(p) = s.strip_prefix("prefix:") {
                return Ok(Selector::Prefix(parse_prefix(p)?));
            }
            Err(format!(
                "selector inválido: {s} (any | self | peer:N | prefix:CIDR)"
            ))
        }
    }
}

fn parse_proto(s: Option<&str>) -> Result<u8, String> {
    match s.unwrap_or("any") {
        "any" => Ok(0),
        "icmp" => Ok(1),
        "tcp" => Ok(6),
        "udp" => Ok(17),
        "icmpv6" => Ok(58),
        n => n.parse().map_err(|_| format!("proto inválido: {n}")),
    }
}

fn parse_ports(s: Option<&str>) -> Result<(u16, u16), String> {
    let Some(s) = s else { return Ok((0, 0)) };
    let bad = || format!("puertos inválidos: {s}");
    match s.split_once('-') {
        Some((a, b)) => Ok((a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?)),
        None => {
            let p: u16 = s.parse().map_err(|_| bad())?;
            Ok((p, p))
        }
    }
}

/// Emite el fichero de configuración firmado de un dispositivo a partir de una especificación TOML.
pub fn issue(issuer_key: &Path, issuer_cert: &Path, spec: &Path, out: &Path) -> Result<(), String> {
    let issuer = read_seed(issuer_key)?;
    let cert_bytes =
        fs::read(issuer_cert).map_err(|e| format!("{}: {e}", issuer_cert.display()))?;
    let cert = IssuerCert::parse(&cert_bytes).map_err(|e| format!("certificado: {e:?}"))?;
    let text = fs::read_to_string(spec).map_err(|e| format!("{}: {e}", spec.display()))?;
    let s: Spec = toml::from_str(&text).map_err(|e| format!("{}: {e}", spec.display()))?;
    let network_id =
        hex::decode::<16>(&s.network_id).ok_or("network_id: se esperan 32 dígitos hex")?;
    if network_id != cert.network_id {
        return Err(
            "la red de la especificación no coincide con la del certificado de emisor".into(),
        );
    }
    let own = load_bundle(&s.device_bundle)?;
    let mut peers = Vec::new();
    for p in &s.peers {
        let b = load_bundle(&p.bundle)?;
        peers.push(Peer {
            device_id: b.device_id,
            noise_pubkey: b.noise_pubkey,
            addrs: p.addrs.clone(),
            endpoints: p.endpoints.clone(),
        });
    }
    let peer_ids: Vec<DeviceId> = peers.iter().map(|p| p.device_id).collect();
    let rules = s
        .rules
        .iter()
        .map(|r| {
            let (port_min, port_max) = parse_ports(r.ports.as_deref())?;
            Ok(Rule {
                src: parse_selector(&r.src, &own.device_id, &peer_ids)?,
                dst: parse_selector(&r.dst, &own.device_id, &peer_ids)?,
                proto: parse_proto(r.proto.as_deref())?,
                port_min,
                port_max,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let not_before = s.not_before.unwrap_or_else(now_secs);
    let mut cfg = DeviceConfig {
        network_id,
        device_id: own.device_id,
        issuer_serial: cert.issuer_serial,
        // Milisegundos Unix por defecto: crece en cada reemisión sin llevar un contador.
        config_version: s.config_version.unwrap_or_else(now_millis),
        not_before,
        not_after: not_before + s.validity_secs,
        noise_pubkey: own.noise_pubkey,
        identity_pubkey: own.identity_pubkey,
        mtu: s.mtu.unwrap_or(1400),
        tunnel_addrs: s.tunnel_addrs,
        network_prefixes: s
            .network_prefixes
            .iter()
            .map(|p| parse_prefix(p))
            .collect::<Result<_, _>>()?,
        peers,
        rules,
    };
    cfg.canonicalize();
    let file = issue_config(&issuer, &cert, &cfg).map_err(|e| format!("emisión: {e:?}"))?;
    write_atomic(out, &file)?;
    eprintln!(
        "emitida configuración v{} válida hasta {}",
        cfg.config_version, cfg.not_after
    );
    Ok(())
}
