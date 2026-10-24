use crate::{AppState, api::ApiError};
use atenea_identity::{DeviceConfig, IssuerCert, Peer, Prefix, Rule, Selector, api_auth};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;
pub struct Device {
    pub id: Uuid,
    pub noise: [u8; 32],
    pub identity: [u8; 32],
    pub addrs: Vec<std::net::IpAddr>,
    pub endpoints: Vec<std::net::SocketAddr>,
}
pub async fn load_devices(c: &mut PgConnection, net: Uuid) -> Result<Vec<Device>, ApiError> {
    let rows=sqlx::query("SELECT id,noise_pubkey,identity_pubkey,endpoints FROM devices WHERE network_id=$1 AND status='approved' ORDER BY id").bind(net).fetch_all(&mut *c).await?;
    let mut devices = Vec::new();
    for r in rows {
        let id: Uuid = r.get("id");
        let addrs: Vec<String> = sqlx::query_scalar(
            "SELECT host(addr) FROM ip_allocations WHERE device_id=$1 ORDER BY family",
        )
        .bind(id)
        .fetch_all(&mut *c)
        .await?;
        devices.push(Device {
            id,
            noise: r
                .get::<Vec<u8>, _>("noise_pubkey")
                .try_into()
                .map_err(|_| ApiError::internal())?,
            identity: r
                .get::<Vec<u8>, _>("identity_pubkey")
                .try_into()
                .map_err(|_| ApiError::internal())?,
            addrs: addrs
                .iter()
                .map(|a| a.parse().map_err(|_| ApiError::internal()))
                .collect::<Result<_, _>>()?,
            endpoints: serde_json::from_value(r.get("endpoints"))
                .map_err(|_| ApiError::internal())?,
        });
    }
    Ok(devices)
}
fn matches(s: &Selector, d: &Device) -> bool {
    match s {
        Selector::Any => true,
        Selector::Device(id) => id == d.id.as_bytes(),
        Selector::Prefix(p) => {
            ipnet::IpNet::new(p.addr, p.len).is_ok_and(|n| d.addrs.iter().any(|a| n.contains(a)))
        }
    }
}
pub fn compile(
    own: &Device,
    devices: &[Device],
    policies: &[Rule],
) -> Result<(Vec<Peer>, Vec<Rule>), ApiError> {
    let mut peers = Vec::new();
    let mut rules = Vec::new();
    for r in policies {
        let relevant = matches(&r.src, own) || matches(&r.dst, own);
        if !relevant {
            continue;
        }
        let viable = |s: &Selector| match s {
            Selector::Device(id) => devices.iter().any(|d| d.id.as_bytes() == id),
            _ => true,
        };
        if !viable(&r.src) || !viable(&r.dst) {
            continue;
        }
        for other in devices {
            if other.id != own.id
                && ((matches(&r.src, own) && matches(&r.dst, other))
                    || (matches(&r.dst, own) && matches(&r.src, other)))
                && !peers
                    .iter()
                    .any(|p: &Peer| p.device_id == *other.id.as_bytes())
            {
                peers.push(Peer {
                    device_id: *other.id.as_bytes(),
                    noise_pubkey: other.noise,
                    addrs: other.addrs.clone(),
                    endpoints: other.endpoints.clone(),
                });
            }
        }
        for rule in [
            r.clone(),
            Rule {
                src: r.dst.clone(),
                dst: r.src.clone(),
                proto: r.proto,
                port_min: 0,
                port_max: 0,
            },
        ] {
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    }
    if peers.len() > 256 || rules.len() > 256 {
        return Err(ApiError::conflict("policy_overflow"));
    }
    Ok((peers, rules))
}
pub async fn get(
    State(s): State<AppState>,
    h: HeaderMap,
    uri: Uri,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let invalid = || ApiError(StatusCode::UNAUTHORIZED, "invalid_signature");
    let signed = api_auth::parse_authorization(
        h.get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    let now = crate::api::now() as u64;
    if now.abs_diff(signed.ts) > 60 {
        return Err(invalid());
    }
    let dev = Uuid::from_bytes(signed.device_id);
    let row = sqlx::query("SELECT network_id,identity_pubkey FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(invalid)?;
    let key: [u8; 32] = row
        .get::<Vec<u8>, _>("identity_pubkey")
        .try_into()
        .map_err(|_| invalid())?;
    api_auth::verify_request(
        &signed,
        &key,
        "GET",
        uri.path_and_query().map_or(uri.path(), |p| p.as_str()),
        &body,
    )
    .map_err(|_| invalid())?;
    {
        let mut nonces = s.nonces.lock().map_err(|_| ApiError::internal())?;
        nonces.retain(|_, ts| now.saturating_sub(*ts) <= 120);
        if nonces.insert((dev, signed.nonce), now).is_some() {
            return Err(invalid());
        }
    }
    let net: Uuid = row.get("network_id");
    let mut tx = s.pool.begin().await?;
    let n=sqlx::query("SELECT org_id,issuer_cert,ipv4_cidr::text v4,ipv6_cidr::text v6 FROM networks WHERE id=$1 FOR UPDATE").bind(net).fetch_one(&mut *tx).await?;
    let status: String = sqlx::query_scalar("SELECT status FROM devices WHERE id=$1")
        .bind(dev)
        .fetch_one(&mut *tx)
        .await?;
    if status != "approved" {
        return Err(ApiError::forbidden("device_not_approved"));
    }
    let unavailable = || ApiError(StatusCode::SERVICE_UNAVAILABLE, "issuer_unavailable");
    let cert = IssuerCert::parse(
        &n.get::<Option<Vec<u8>>, _>("issuer_cert")
            .ok_or_else(unavailable)?,
    )
    .map_err(|_| unavailable())?;
    let end = now
        .checked_add(s.config.config_validity_secs)
        .ok_or_else(unavailable)?;
    if cert.not_before > now
        || cert.not_after < end
        || s.config.config_validity_secs == 0
        || s.config.config_validity_secs > 3600
    {
        return Err(unavailable());
    }
    let devices = load_devices(&mut tx, net).await?;
    let own = devices
        .iter()
        .find(|d| d.id == dev)
        .ok_or_else(ApiError::internal)?;
    let policies = crate::policies::rules(&mut tx, net).await?;
    let (peers, rules) = compile(own, &devices, &policies)?;
    let last: Option<i64> =
        sqlx::query_scalar("SELECT max(version) FROM configuration_versions WHERE device_id=$1")
            .bind(dev)
            .fetch_one(&mut *tx)
            .await?;
    let version = chrono::Utc::now().timestamp_millis().max(
        last.unwrap_or(0)
            .checked_add(1)
            .ok_or_else(ApiError::internal)?,
    );
    let mut cfg = DeviceConfig {
        network_id: *net.as_bytes(),
        device_id: *dev.as_bytes(),
        issuer_serial: cert.issuer_serial,
        config_version: version as u64,
        not_before: now,
        not_after: end,
        noise_pubkey: own.noise,
        identity_pubkey: own.identity,
        mtu: 1400,
        tunnel_addrs: own.addrs.clone(),
        network_prefixes: ["v4", "v6"]
            .iter()
            .map(|k| {
                let p: ipnet::IpNet = n
                    .get::<String, _>(*k)
                    .parse()
                    .map_err(|_| ApiError::internal())?;
                Ok(Prefix {
                    addr: p.network(),
                    len: p.prefix_len(),
                })
            })
            .collect::<Result<_, ApiError>>()?,
        peers,
        rules,
    };
    cfg.canonicalize();
    let file =
        atenea_identity::issue_config(&s.issuer, &cert, &cfg).map_err(|_| ApiError::internal())?;
    let hash = Sha256::digest(&file);
    sqlx::query("INSERT INTO configuration_versions(device_id,version,config_hash,not_before,not_after,issuer_serial) VALUES($1,$2,$3,to_timestamp($4),to_timestamp($5),$6)").bind(dev).bind(version).bind(hash.as_slice()).bind(now as f64).bind(end as f64).bind(cert.issuer_serial as i64).execute(&mut *tx).await?;
    sqlx::query("UPDATE devices SET last_config_at=now() WHERE id=$1")
        .bind(dev)
        .execute(&mut *tx)
        .await?;
    let org: Uuid = n.get("org_id");
    crate::audit::record(
        &mut tx,
        "device",
        dev,
        Some(org),
        Some(net),
        "config.issue",
        dev,
        serde_json::json!({"version":version,"hash":hex::encode(hash)}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "devices");
    s.panel.changed(net, "audit");
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("cache-control", "no-store"),
            ("x-config-version", &version.to_string()),
        ],
        file,
    )
        .into_response())
}
