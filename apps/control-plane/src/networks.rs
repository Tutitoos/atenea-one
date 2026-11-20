// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
use crate::{AppState, api::ApiError, audit, auth, orgs};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use ipnet::IpNet;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;
pub async fn access(
    s: &AppState,
    h: &HeaderMap,
    net: Uuid,
    write: bool,
) -> Result<(Uuid, Uuid), ApiError> {
    access_as(s, h, net, write, if write { "admin" } else { "member" }).await
}
async fn org_of(s: &AppState, net: Uuid) -> Result<Uuid, ApiError> {
    sqlx::query_scalar("SELECT org_id FROM networks WHERE id=$1")
        .bind(net)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(ApiError::not_found)
}
/// Rol del usuario en la red por invitación (`viewer` o `enroller`, ADR-028 D6); nulo si no es miembro de la red.
pub(crate) async fn member_role(
    s: &AppState,
    user: Uuid,
    net: Uuid,
) -> Result<Option<String>, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT role FROM network_members WHERE network_id=$1 AND user_id=$2")
            .bind(net)
            .bind(user)
            .fetch_optional(&s.pool)
            .await?,
    )
}
pub(crate) async fn access_as(
    s: &AppState,
    h: &HeaderMap,
    net: Uuid,
    write: bool,
    role: &str,
) -> Result<(Uuid, Uuid), ApiError> {
    let u = auth::session(s, h, write).await?;
    let org = org_of(s, net).await?;
    match orgs::role(s, u, org, role).await {
        Ok(()) => Ok((u, org)),
        Err(e) => match member_role(s, u, net).await? {
            // Un miembro por invitación solo lee los datos generales de la red; nada que exija admin u owner.
            Some(_) if !write && role == "member" => Ok((u, org)),
            Some(_) => Err(ApiError::forbidden("forbidden")),
            None => Err(e),
        },
    }
}
/// Generar códigos y aprobar dispositivos: admin u owner de la organización, o `enroller` de la red (ADR-028 D6).
/// El tercer valor es `true` cuando solo es `enroller`: quien aprueba así debe aportar la huella completa.
pub async fn access_enroll(
    s: &AppState,
    h: &HeaderMap,
    net: Uuid,
) -> Result<(Uuid, Uuid, bool), ApiError> {
    let u = auth::session(s, h, true).await?;
    let org = org_of(s, net).await?;
    match orgs::role(s, u, org, "admin").await {
        Ok(()) => Ok((u, org, false)),
        Err(e) => match member_role(s, u, net).await?.as_deref() {
            Some("enroller") => Ok((u, org, true)),
            Some(_) => Err(ApiError::forbidden("forbidden")),
            None => Err(e),
        },
    }
}
/// Dispositivos aprobados por red (ADR-031): el tope de 256 pasa a 500.
pub const MAX_APPROVED: i64 = 500;

/// Tope de dispositivos vivos de una red de organización personal (ADR-028 D4). Se llama con la fila de la red
/// bloqueada. Las organizaciones normales no tienen tope.
pub async fn check_device_quota(
    tx: &mut PgConnection,
    net: Uuid,
    limit: u32,
) -> Result<(), ApiError> {
    let full: bool = sqlx::query_scalar(
        "SELECT o.personal AND (SELECT count(*) FROM devices WHERE network_id=n.id AND status<>'revoked') >= $2 \
         FROM networks n JOIN organizations o ON o.id=n.org_id WHERE n.id=$1",
    )
    .bind(net)
    .bind(i64::from(limit))
    .fetch_one(&mut *tx)
    .await?;
    if full {
        return Err(ApiError::conflict("device_limit"));
    }
    Ok(())
}
#[derive(Deserialize)]
pub struct Ipv4Pool {
    pub start: String,
    pub end: String,
}
#[derive(Deserialize)]
pub struct NewNetwork {
    pub name: String,
    /// Sin CIDR se asignan solos (ADR-029 D2).
    pub ipv4_cidr: Option<String>,
    pub ipv6_cidr: Option<String>,
    /// `false` crea una red solo IPv4; por defecto la red tiene también IPv6.
    pub ipv6: Option<bool>,
    /// Rango de reparto IPv4: los equipos solo reciben direcciones entre `start` y `end`.
    pub ipv4_pool: Option<Ipv4Pool>,
    /// Con ancla la raíz es del usuario (modo propio, ADR-028); sin ella la red es gestionada por el servidor.
    pub root_anchor: Option<String>,
    /// Conexión automática de los dispositivos aprobados (ADR-031); sin valor, activa.
    pub auto_connect: Option<bool>,
}
pub(crate) fn value(r: &PgRow) -> Result<Value, ApiError> {
    let certificate = r.get::<Option<&[u8]>, _>("issuer_cert");
    let expires = certificate
        .map(|bytes| {
            let cert =
                atenea_identity::IssuerCert::parse(bytes).map_err(|_| ApiError::internal())?;
            let timestamp = i64::try_from(cert.not_after).map_err(|_| ApiError::internal())?;
            chrono::DateTime::from_timestamp(timestamp, 0)
                .map(|date| date.to_rfc3339())
                .ok_or_else(ApiError::internal)
        })
        .transpose()?;
    let pool = r
        .get::<Option<String>, _>("ps")
        .zip(r.get::<Option<String>, _>("pe"))
        .map(|(start, end)| json!({"start": start, "end": end}));
    Ok(
        json!({"id":r.get::<Uuid,_>("id"),"org_id":r.get::<Uuid,_>("org_id"),"name":r.get::<String,_>("name"),"ipv4_cidr":r.get::<String,_>("v4"),"ipv6_cidr":r.get::<Option<String>,_>("v6"),"ipv4_pool":pool,"root_anchor":hex::encode(r.get::<&[u8],_>("root_anchor")),"root_mode":r.get::<String,_>("root_mode"),"has_issuer_cert":certificate.is_some(),"issuer_cert_not_after":expires,"join_policy":r.get::<String,_>("join_policy"),"auto_connect":r.get::<bool,_>("auto_connect")}),
    )
}
/// Bloques de la red (IPv4 y, si tiene, IPv6) de una fila con columnas `v4` y `v6`.
pub(crate) fn prefixes(r: &PgRow) -> Result<Vec<atenea_identity::Prefix>, ApiError> {
    ["v4", "v6"]
        .iter()
        .filter_map(|k| r.get::<Option<String>, _>(*k))
        .map(|text| {
            let n: IpNet = text.parse().map_err(|_| ApiError::internal())?;
            Ok(atenea_identity::Prefix {
                addr: n.network(),
                len: n.prefix_len(),
            })
        })
        .collect()
}
pub(crate) const SELECT: &str = "SELECT id,org_id,name,ipv4_cidr::text v4,ipv6_cidr::text v6,host(ipv4_pool_start) ps,host(ipv4_pool_end) pe,root_anchor,root_mode,issuer_cert,join_policy,auto_connect FROM networks";
/// Código de red (`ATENEA1-…`) con la URL pública del servidor; 409 si ésta o el nombre no caben en el formato.
/// Con `with_connectivity` y el servidor configurado (ADR-018) lleva además el STUN y el hash de la CA del relay;
/// si con eso el texto supera 512 caracteres, `409 code_too_long` (con `?conectividad=0` sigue siendo válido).
pub fn join_code(
    s: &AppState,
    net: Uuid,
    name: &str,
    root: &[u8],
    token: Option<&str>,
    with_connectivity: bool,
) -> Result<String, ApiError> {
    use atenea_identity::join_code::{Connectivity, JoinCode, JoinCodeError};
    let connectivity = s
        .connectivity
        .as_ref()
        .filter(|_| with_connectivity)
        .map(|c| Connectivity {
            stun_host: c.stun_host.clone(),
            relay_ca_sha256: c.relay_ca.as_ref().map(|ca| ca.sha256),
        });
    JoinCode {
        url: s.config.public_url.clone(),
        network_id: net.into_bytes(),
        root_anchor: root.try_into().map_err(|_| ApiError::internal())?,
        name: name.to_owned(),
        token: token.map(str::to_owned),
        connectivity,
    }
    .encode()
    .map_err(|e| match e {
        JoinCodeError::InvalidName => ApiError::conflict("invalid_network_name_for_code"),
        JoinCodeError::InvalidUrl => ApiError::conflict("invalid_public_url_for_code"),
        JoinCodeError::TooLong => ApiError::conflict("code_too_long"),
        _ => ApiError::internal(),
    })
}

/// `?conectividad=0` pide el código **sin** bloque (clientes anteriores a la v0.1.9); `1` o ausente, el de siempre.
#[derive(Deserialize)]
pub struct CodeQuery {
    conectividad: Option<String>,
}
impl CodeQuery {
    pub fn wants_connectivity(&self) -> Result<bool, ApiError> {
        match self.conectividad.as_deref() {
            None | Some("1") => Ok(true),
            Some("0") => Ok(false),
            _ => Err(ApiError::bad("invalid_request")),
        }
    }
}

/// Lo que lleva el código, para mostrarlo sin decodificarlo: `null` si no lleva bloque.
pub fn connectivity_json(s: &AppState, with_connectivity: bool) -> Value {
    match s.connectivity.as_ref().filter(|_| with_connectivity) {
        Some(c) => json!({
            "stun_host": c.stun_host,
            "relay_ca_sha256": c.relay_ca.as_ref().map(|ca| hex::encode(ca.sha256)),
        }),
        None => Value::Null,
    }
}
pub async fn set_join_policy(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = access(&s, &h, net, true).await?;
    let policy = match b.get("policy").and_then(Value::as_str) {
        Some(p @ ("cerrada" | "con_solicitud")) => p,
        _ => return Err(ApiError::bad("invalid_request")),
    };
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE networks SET join_policy=$2 WHERE id=$1")
        .bind(net)
        .bind(policy)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network.join_policy",
        net,
        json!({"policy":policy}),
    )
    .await?;
    tx.commit().await?;
    s.panel.changed(net, "network");
    s.panel.changed(net, "audit");
    Ok(Json(json!({"policy":policy})))
}
pub async fn join_code_get(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    Query(q): Query<CodeQuery>,
    h: HeaderMap,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    let with_connectivity = q.wants_connectivity()?;
    access_as(&s, &h, net, false, "admin").await?;
    let r = sqlx::query("SELECT name,root_anchor,join_policy FROM networks WHERE id=$1")
        .bind(net)
        .fetch_one(&s.pool)
        .await?;
    let (name, root) = (
        r.get::<String, _>("name"),
        r.get::<Vec<u8>, _>("root_anchor"),
    );
    let code = join_code(&s, net, &name, &root, None, with_connectivity)?;
    let anchor: [u8; 32] = root.try_into().map_err(|_| ApiError::internal())?;
    Ok((
        [("cache-control", "no-store")],
        Json(
            json!({"code":code,"root_anchor_fingerprint":atenea_identity::anchor_fingerprint(&anchor),"join_policy":r.get::<String,_>("join_policy"),"name":name,"url":s.config.public_url,"connectivity":connectivity_json(&s, with_connectivity)}),
        ),
    ))
}
pub async fn create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(b): Json<NewNetwork>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let u = auth::session(&s, &h, true).await?;
    orgs::role(&s, u, org, "admin").await?;
    orgs::name(&b.name)?;
    // ADR-029: sin ancla la red es gestionada (el servidor genera y cifra la raíz); con ancla, la raíz es del usuario.
    let managed = b.root_anchor.is_none();
    let auto_connect = b.auto_connect.unwrap_or(true);
    if managed && s.wrap.is_none() {
        return Err(ApiError::conflict("managed_unavailable"));
    }
    let parse = |text: &Option<String>| -> Result<Option<IpNet>, ApiError> {
        text.as_deref()
            .map(|t| t.parse().map_err(|_| ApiError::bad("invalid_cidr")))
            .transpose()
    };
    let (given_v4, given_v6) = (parse(&b.ipv4_cidr)?, parse(&b.ipv6_cidr)?);
    let provided_root: Option<[u8; 32]> = b
        .root_anchor
        .as_deref()
        .map(|text| {
            hex::decode(text)
                .ok()
                .and_then(|x| x.try_into().ok())
                .filter(|root: &[u8; 32]| {
                    *root != [0; 32]
                        && atenea_identity::VerifyingKey::from_bytes(root)
                            .is_ok_and(|k| !k.is_weak())
                })
                .ok_or_else(|| ApiError::bad("invalid_root"))
        })
        .transpose()?;
    let id = Uuid::now_v7();
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM organizations WHERE id=$1 FOR UPDATE")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    // ADR-028 D4: tope de redes de una organización personal; las demás no tienen tope.
    let full: bool = sqlx::query_scalar(
        "SELECT personal AND (SELECT count(*) FROM networks WHERE org_id=$1) >= $2 FROM organizations WHERE id=$1",
    )
    .bind(org)
    .bind(i64::from(s.config.personal_max_networks))
    .fetch_one(&mut *tx)
    .await?;
    if full {
        return Err(ApiError::conflict("network_limit"));
    }
    // Rango de reparto: inicio y fin IPv4 en orden. Sin bloque dado, la red es el bloque más pequeño que lo contiene.
    let pool = b
        .ipv4_pool
        .as_ref()
        .map(|p| {
            let parse = |t: &str| t.parse::<std::net::Ipv4Addr>();
            match (parse(&p.start), parse(&p.end)) {
                (Ok(start), Ok(end)) if start <= end => Ok((start, end)),
                _ => Err(ApiError::bad("invalid_pool")),
            }
        })
        .transpose()?;
    let v4 = match (given_v4, pool) {
        (Some(net), _) => net,
        (None, Some((start, end))) => {
            enclosing(start, end).ok_or_else(|| ApiError::bad("invalid_pool"))?
        }
        (None, None) => auto_v4(&mut tx).await?,
    };
    // `ipv6: false` crea una red solo IPv4; pedirlo junto con un bloque IPv6 es contradictorio.
    let v6 = match (b.ipv6.unwrap_or(true), given_v6) {
        (false, Some(_)) => return Err(ApiError::bad("invalid_cidr")),
        (false, None) => None,
        (true, given) => Some(given.unwrap_or_else(|| auto_v6(id))),
    };
    let prefix = |n: &IpNet| atenea_identity::Prefix {
        addr: n.addr(),
        len: n.prefix_len(),
    };
    if !v4.addr().is_ipv4()
        || v4.addr() != v4.network()
        || !prefix(&v4).valid_route()
        || v6.as_ref().is_some_and(|v6| {
            !v6.addr().is_ipv6() || v6.addr() != v6.network() || !prefix(v6).valid_route()
        })
    {
        return Err(ApiError::bad("invalid_cidr"));
    }
    if pool.is_some_and(|(start, end)| {
        let (start, end) = (std::net::IpAddr::V4(start), std::net::IpAddr::V4(end));
        !v4.contains(&start) || !v4.contains(&end) || start == v4.network() || end == v4.broadcast()
    }) {
        return Err(ApiError::bad("invalid_pool"));
    }
    let overlap:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM networks WHERE org_id=$1 AND (ipv4_cidr && $2::cidr OR ipv6_cidr && $3::cidr))").bind(org).bind(v4.to_string()).bind(v6.map(|n| n.to_string())).fetch_one(&mut *tx).await?;
    if overlap {
        return Err(ApiError::conflict("overlapping_network"));
    }
    let new_root = match (&provided_root, s.wrap.as_deref()) {
        (None, Some(wrap)) => Some(crate::managed::generate(wrap, id)?),
        _ => None,
    };
    let anchor = provided_root
        .or_else(|| new_root.as_ref().map(|r| r.anchor))
        .ok_or_else(ApiError::internal)?;
    sqlx::query("INSERT INTO networks(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor,root_mode,root_wrapped,ipv4_pool_start,ipv4_pool_end,auto_connect) VALUES($1,$2,$3,$4::cidr,$5::cidr,$6,$7,$8,$9::inet,$10::inet,$11)")
        .bind(id)
        .bind(org)
        .bind(&b.name)
        .bind(v4.to_string())
        .bind(v6.map(|n| n.to_string()))
        .bind(anchor.as_slice())
        .bind(if managed { "managed" } else { "own" })
        .bind(new_root.as_ref().map(|r| r.wrapped.as_slice()))
        .bind(pool.map(|(start, _)| start.to_string()))
        .bind(pool.map(|(_, end)| end.to_string()))
        .bind(auto_connect)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(id),
        "network.create",
        id,
        if managed {
            json!({"managed": true})
        } else {
            json!({})
        },
    )
    .await?;
    if let Some(root) = &new_root {
        crate::managed::sign_first(&s, &mut tx, id, org, root).await?;
    }
    let row = sqlx::query(&format!("{SELECT} WHERE id=$1"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    s.panel.changed(id, "network");
    s.panel.changed(id, "audit");
    Ok((StatusCode::CREATED, Json(value(&row)?)))
}
/// Siguiente /24 libre del bloque privado reservado `10.64.0.0/10` (ADR-029 D2), sin solaparse con ninguna red
/// existente de ningún usuario: un equipo puede unirse a varias redes sin conflicto de rutas.
async fn auto_v4(tx: &mut PgConnection) -> Result<IpNet, ApiError> {
    let used: Vec<IpNet> = sqlx::query_scalar::<_, String>(
        "SELECT ipv4_cidr::text FROM networks WHERE ipv4_cidr && '10.64.0.0/10'::cidr",
    )
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .filter_map(|text| text.parse().ok())
    .collect();
    (0..16384u32)
        .filter_map(|i| {
            let addr = std::net::Ipv4Addr::from(0x0A40_0000 + (i << 8));
            ipnet::Ipv4Net::new(addr, 24).ok().map(IpNet::V4)
        })
        .find(|candidate| {
            !used
                .iter()
                .any(|u| u.contains(candidate) || candidate.contains(u))
        })
        .ok_or_else(|| ApiError::conflict("address_pool_exhausted"))
}
/// ULA /64 derivado de bits aleatorios del identificador (`fd` + 40 bits): sin consultar la base, con colisión
/// despreciable; si ocurriera, el solape se rechaza como cualquier otro.
fn auto_v6(id: Uuid) -> IpNet {
    let mut bytes = [0u8; 16];
    bytes[0] = 0xfd;
    bytes[1..6].copy_from_slice(&id.as_bytes()[9..14]);
    IpNet::V6(ipnet::Ipv6Net::new(std::net::Ipv6Addr::from(bytes), 64).expect("prefijo 64 válido"))
}
pub async fn list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let u = auth::session(&s, &h, false).await?;
    orgs::role(&s, u, org, "member").await?;
    let rows = sqlx::query(&format!("{SELECT} WHERE org_id=$1 ORDER BY id"))
        .bind(org)
        .fetch_all(&s.pool)
        .await?;
    Ok(Json(json!(
        rows.iter().map(value).collect::<Result<Vec<_>, _>>()?
    )))
}
pub async fn get(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    access(&s, &h, net, false).await?;
    let r = sqlx::query(&format!("{SELECT} WHERE id=$1"))
        .bind(net)
        .fetch_one(&s.pool)
        .await?;
    Ok(Json(value(&r)?))
}

/// Cambios de la red (ADR-031): nombre y/o conexión automática. Sin ninguno de los dos, 400 invalid_request.
#[derive(Deserialize)]
pub struct NetworkUpdate {
    name: Option<String>,
    auto_connect: Option<bool>,
}
pub async fn update(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    Json(body): Json<NetworkUpdate>,
) -> Result<Json<Value>, ApiError> {
    let (u, org) = access(&s, &h, net, true).await?;
    if body.name.is_none() && body.auto_connect.is_none() {
        return Err(ApiError::bad("invalid_request"));
    }
    if let Some(name) = &body.name {
        orgs::name(name)?;
    }
    let mut tx = s.pool.begin().await?;
    // La fila de la red serializa estos cambios con las aprobaciones: todas ven el mismo ajuste (ADR-031).
    let old =
        sqlx::query("SELECT name,root_anchor,auto_connect FROM networks WHERE id=$1 FOR UPDATE")
            .bind(net)
            .fetch_one(&mut *tx)
            .await?;
    if let Some(name) = &body.name {
        join_code(
            &s,
            net,
            name,
            &old.get::<Vec<u8>, _>("root_anchor"),
            None,
            // El nombre solo debe caber en el formato; si con la conectividad no cupiera, el código sigue
            // disponible con `?conectividad=0` y el `409 code_too_long` lo avisa al pedirlo.
            false,
        )?;
        sqlx::query("UPDATE networks SET name=$2 WHERE id=$1")
            .bind(net)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            "user",
            u,
            Some(org),
            Some(net),
            "network.rename",
            net,
            json!({"previous_name":old.get::<String,_>("name"),"name":name}),
        )
        .await?;
    }
    if let Some(auto_connect) = body.auto_connect {
        let previous: bool = old.get("auto_connect");
        if auto_connect != previous {
            sqlx::query("UPDATE networks SET auto_connect=$2 WHERE id=$1")
                .bind(net)
                .bind(auto_connect)
                .execute(&mut *tx)
                .await?;
            audit::record(
                &mut tx,
                "user",
                u,
                Some(org),
                Some(net),
                "network.auto_connect",
                net,
                json!({"previous":previous,"auto_connect":auto_connect}),
            )
            .await?;
        }
    }
    let row = sqlx::query(&format!("{SELECT} WHERE id=$1"))
        .bind(net)
        .fetch_one(&mut *tx)
        .await?;
    let response = value(&row)?;
    tx.commit().await?;
    s.panel.changed(net, "network");
    s.panel.changed(net, "audit");
    Ok(Json(response))
}
pub async fn remove(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (u, org) = access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    let name: String = sqlx::query_scalar("DELETE FROM networks WHERE id=$1 RETURNING name")
        .bind(net)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::not_found)?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network.delete",
        net,
        json!({"name":name}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn certificate(
    State(s): State<AppState>,
    Path(net): Path<Uuid>,
    h: HeaderMap,
    b: Bytes,
) -> Result<StatusCode, ApiError> {
    let (u, org) = access(&s, &h, net, true).await?;
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT root_anchor,issuer_cert,root_mode FROM networks WHERE id=$1 FOR UPDATE",
    )
    .bind(net)
    .fetch_one(&mut *tx)
    .await?;
    // ADR-029: el servidor firma y renueva solo el certificado de una red gestionada; subir uno a mano no tiene sentido.
    if row.get::<String, _>("root_mode") == "managed" {
        return Err(ApiError::conflict("managed_network"));
    }
    let root: [u8; 32] = row
        .get::<Vec<u8>, _>("root_anchor")
        .try_into()
        .map_err(|_| ApiError::internal())?;
    let installed: Option<Vec<u8>> = row.get("issuer_cert");
    let cert =
        atenea_identity::IssuerCert::parse(&b).map_err(|_| ApiError::bad("invalid_certificate"))?;
    cert.verify(&root, *net.as_bytes(), crate::api::now() as u64)
        .map_err(|_| ApiError::bad("invalid_certificate"))?;
    if cert.issuer_pubkey != s.issuer.verifying_key().to_bytes() {
        return Err(ApiError::bad("wrong_issuer"));
    }
    // Los agentes rechazan un serial menor que el mayor que ya vieron (`issuer_rollback`) y pierden el lease al
    // caducar su última configuración: retroceder el certificado instalado cortaría la red. Un certificado
    // almacenado ilegible no impide repararlo con uno válido; el mismo serial es una renovación idempotente.
    if let Some(current) = installed
        .as_deref()
        .and_then(|bytes| atenea_identity::IssuerCert::parse(bytes).ok())
        && cert.issuer_serial < current.issuer_serial
    {
        return Err(ApiError::conflict("serial_rollback"));
    }
    sqlx::query("UPDATE networks SET issuer_cert=$1 WHERE id=$2")
        .bind(b.as_ref())
        .bind(net)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        "user",
        u,
        Some(org),
        Some(net),
        "network.issuer_cert",
        net,
        json!({"serial":cert.issuer_serial}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn allocate(c: &mut PgConnection, net: Uuid, dev: Uuid) -> Result<(), ApiError> {
    for attempt in 0..2 {
        sqlx::query("SAVEPOINT ipam_attempt")
            .execute(&mut *c)
            .await?;
        match allocate_once(c, net, dev).await {
            Ok(()) => {
                sqlx::query("RELEASE SAVEPOINT ipam_attempt")
                    .execute(&mut *c)
                    .await?;
                return Ok(());
            }
            Err(error) => {
                sqlx::query("ROLLBACK TO SAVEPOINT ipam_attempt")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("RELEASE SAVEPOINT ipam_attempt")
                    .execute(&mut *c)
                    .await?;
                if attempt == 1 || error.code != "conflict" {
                    return Err(error);
                }
            }
        }
    }
    Err(ApiError::conflict("ipam_conflict"))
}
async fn allocate_once(c: &mut PgConnection, net: Uuid, dev: Uuid) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT ipv4_cidr::text v4,ipv6_cidr::text v6,host(ipv4_pool_start) ps,host(ipv4_pool_end) pe FROM networks WHERE id=$1 FOR UPDATE",
    )
    .bind(net)
    .fetch_one(&mut *c)
    .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM devices WHERE network_id=$1 AND status='approved'",
    )
    .bind(net)
    .fetch_one(&mut *c)
    .await?;
    if count >= MAX_APPROVED {
        return Err(ApiError::conflict("network_full"));
    }
    let number = |a: std::net::IpAddr| match a {
        std::net::IpAddr::V4(a) => u128::from(u32::from(a)),
        std::net::IpAddr::V6(a) => u128::from(a),
    };
    // Rango de reparto IPv4 opcional: sin él se reparte todo el bloque salvo la dirección de red y la de difusión.
    let pool = |key: &str| -> Result<Option<u128>, ApiError> {
        row.get::<Option<String>, _>(key)
            .map(|t| t.parse().map(number).map_err(|_| ApiError::internal()))
            .transpose()
    };
    let (pool_start, pool_end) = (pool("ps")?, pool("pe")?);
    for (family, key) in [(4_i16, "v4"), (6, "v6")] {
        // Una red sin IPv6 no reparte direcciones de esa familia.
        let Some(text) = row.get::<Option<String>, _>(key) else {
            continue;
        };
        let prefix: IpNet = text.parse().map_err(|_| ApiError::internal())?;
        let used: Vec<String> = sqlx::query_scalar(
            "SELECT host(addr) FROM ip_allocations WHERE network_id=$1 AND family=$2",
        )
        .bind(net)
        .bind(family)
        .fetch_all(&mut *c)
        .await?;
        let used: std::collections::HashSet<std::net::IpAddr> =
            used.iter().filter_map(|s| s.parse().ok()).collect();
        let (mut candidate, last) = match (family, pool_start, pool_end) {
            (4, Some(start), Some(end)) => (start - 1, Some(end)),
            _ => (number(prefix.network()), None),
        };
        let address = loop {
            candidate = candidate
                .checked_add(1)
                .ok_or_else(|| ApiError::conflict("network_full"))?;
            if last.is_some_and(|end| candidate > end) {
                return Err(ApiError::conflict("network_full"));
            }
            let a = if family == 4 {
                if candidate > u128::from(u32::MAX) {
                    return Err(ApiError::conflict("network_full"));
                }
                std::net::IpAddr::V4((candidate as u32).into())
            } else {
                std::net::IpAddr::V6(candidate.into())
            };
            if !prefix.contains(&a) || family == 4 && a == prefix.broadcast() {
                return Err(ApiError::conflict("network_full"));
            }
            if !used.contains(&a) {
                break a;
            }
        };
        sqlx::query("INSERT INTO ip_allocations(network_id,device_id,family,addr) VALUES($1,$2,$3,$4::inet)").bind(net).bind(dev).bind(family).bind(address.to_string()).execute(&mut *c).await?;
    }
    Ok(())
}
/// Bloque privado más pequeño (de /8 a /30) que contiene el rango sin que `start` sea su dirección de red ni `end` la de difusión.
fn enclosing(start: std::net::Ipv4Addr, end: std::net::Ipv4Addr) -> Option<IpNet> {
    (8..=30u8).rev().find_map(|len| {
        let net = ipnet::Ipv4Net::new(start, len).ok()?.trunc();
        (net.contains(&end) && start != net.network() && end != net.broadcast())
            .then_some(IpNet::V4(net))
    })
}
