// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-031: conexiones automáticas por aprobación, orden de aprobación, excepciones y capacidad config_v3.
mod common;

use atenea_identity::{
    AgentState, LocalKeys, SigningKey, generate_seed, signing_key, verify_config,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Device, Session, TestApp, id, now, report_at};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

/// Red de prueba con el ajuste indicado. La raíz es de la red, no se necesita para la API.
async fn network(t: &TestApp, s: &Session, auto: bool, v4: &str, v6: &str) -> Uuid {
    let org = t.org(s).await;
    let root = signing_key(generate_seed());
    t.network_with(s, org, &root, v4, v6, auto).await
}

/// Dispositivo nuevo con token; con `auto` queda aprobado al inscribirse.
async fn enrolled(t: &TestApp, s: &Session, net: Uuid, auto: bool) -> Device {
    let d = Device::new();
    let token = t.token(s, net, auto).await;
    assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
    d
}

async fn approve(t: &TestApp, s: &Session, net: Uuid, d: &Device) {
    let r = t.action(s, net, d.id, "approve").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
}

async fn set_auto(t: &TestApp, s: &Session, net: Uuid, on: bool) {
    let r = t
        .json(
            "PATCH",
            &format!("/api/v1/networks/{net}"),
            Some(s),
            json!({"auto_connect": on}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
}

/// Orden de aprobación y ajuste vigente en esa aprobación.
async fn order(t: &TestApp, d: &Device) -> (i64, bool) {
    let row =
        sqlx::query("SELECT approval_order,auto_connect_on_approval FROM devices WHERE id=$1")
            .bind(d.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    (
        row.get("approval_order"),
        row.get("auto_connect_on_approval"),
    )
}

async fn capability(t: &TestApp, d: &Device) -> bool {
    sqlx::query_scalar("SELECT config_v3 FROM devices WHERE id=$1")
        .bind(d.id)
        .fetch_one(&t.pool)
        .await
        .unwrap()
}

/// Pares automáticos según la API, ordenados.
async fn auto_peers(t: &TestApp, s: &Session, net: Uuid, d: &Device) -> Vec<Uuid> {
    let r = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices/{}/auto-peers", d.id),
            Some(s),
            Value::Null,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let mut peers: Vec<Uuid> = r
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().parse::<Uuid>().unwrap())
        .collect();
    peers.sort();
    peers
}

/// Pares de la configuración firmada de `d`, ordenados. Los automáticos se emiten igual que los manuales.
async fn config_peers(t: &TestApp, root: &SigningKey, net: Uuid, d: &Device) -> Vec<Uuid> {
    let reply = t.config(d, now(), *Uuid::new_v4().as_bytes()).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    let local = LocalKeys {
        noise_pub: d.bundle.noise_pubkey,
        identity_pub: d.bundle.identity_pubkey,
    };
    let state = AgentState::new(
        *net.as_bytes(),
        *d.id.as_bytes(),
        root.verifying_key().to_bytes(),
    );
    let verified = verify_config(&reply.body, &local, &state, now()).unwrap();
    let mut peers: Vec<Uuid> = verified
        .config
        .peers
        .iter()
        .map(|p| Uuid::from_bytes(p.device_id))
        .collect();
    peers.sort();
    peers
}

fn ids(devices: &[&Device]) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = devices.iter().map(|d| d.id).collect();
    out.sort();
    out
}

async fn audits(t: &TestApp, net: Uuid, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE network_id=$1 AND action=$2")
        .bind(net)
        .bind(action)
        .fetch_one(&t.pool)
        .await
        .unwrap()
}

/// Deja pasar la ventana de 5 s entre informes aceptados del mismo dispositivo.
async fn spend_window(t: &TestApp, d: &Device) {
    sqlx::query(
        "UPDATE device_reports SET reported_at=now()-interval '6 seconds' WHERE device_id=$1",
    )
    .bind(d.id)
    .execute(&t.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn red_nueva_nace_con_ajuste_activo_y_patch_lo_cambia_con_auditoria() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    // Sin el campo, la red nace con la conexión automática activa (ADR-031).
    let created = t
        .json(
            "POST",
            &format!("/api/v1/orgs/{org}/networks"),
            Some(&s),
            json!({"name":"activa","ipv4_cidr":"10.130.0.0/16","ipv6_cidr":"fd30::/64","root_anchor":hex::encode(root.verifying_key().to_bytes())}),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.json);
    assert_eq!(created.json["auto_connect"], true);
    let net = id(&created.json);
    let path = format!("/api/v1/networks/{net}");
    assert_eq!(
        t.json("GET", &path, Some(&s), Value::Null).await.json["auto_connect"],
        true
    );
    let off = t
        .json("PATCH", &path, Some(&s), json!({"auto_connect":false}))
        .await;
    assert_eq!(off.status, StatusCode::OK, "{}", off.json);
    assert_eq!(off.json["auto_connect"], false);
    // Repetir el mismo valor no cambia nada ni audita.
    assert_eq!(
        t.json("PATCH", &path, Some(&s), json!({"auto_connect":false}))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(audits(&t, net, "network.auto_connect").await, 1);
    let detail: Value = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE network_id=$1 AND action='network.auto_connect'",
    )
    .bind(net)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(detail, json!({"previous":true,"auto_connect":false}));
    t.json("PATCH", &path, Some(&s), json!({}))
        .await
        .error(StatusCode::BAD_REQUEST, "invalid_request");
    t.finish().await;
}

#[tokio::test]
async fn ejemplo_del_adr_orden_ajuste_excepciones_y_revocacion() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t
        .network_with(&s, org, &root, "10.131.0.0/16", "fd31::/64", false)
        .await;
    t.certificate(&s, net, &root).await;
    let a = enrolled(&t, &s, net, false).await;
    approve(&t, &s, net, &a).await;
    let b = enrolled(&t, &s, net, false).await;
    approve(&t, &s, net, &b).await;
    set_auto(&t, &s, net, true).await;
    let c = enrolled(&t, &s, net, false).await;
    approve(&t, &s, net, &c).await;
    set_auto(&t, &s, net, false).await;
    let d = enrolled(&t, &s, net, false).await;
    approve(&t, &s, net, &d).await;
    assert_eq!(order(&t, &a).await, (1, false));
    assert_eq!(order(&t, &b).await, (2, false));
    assert_eq!(order(&t, &c).await, (3, true));
    assert_eq!(order(&t, &d).await, (4, false));
    assert_eq!(auto_peers(&t, &s, net, &a).await, ids(&[&c]));
    assert_eq!(auto_peers(&t, &s, net, &b).await, ids(&[&c]));
    assert_eq!(auto_peers(&t, &s, net, &c).await, ids(&[&a, &b]));
    assert!(auto_peers(&t, &s, net, &d).await.is_empty());

    // Una política manual entre A y D sigue vigente con independencia del ajuste.
    let policy = t
        .json(
            "POST",
            &format!("/api/v1/networks/{net}/policies"),
            Some(&s),
            json!({"src":{"kind":"device","device_id":a.id},"dst":{"kind":"device","device_id":d.id},"proto":0}),
        )
        .await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.json);
    // La configuración firmada recoge lo mismo: automáticos y manuales salen como pares.
    assert_eq!(config_peers(&t, &root, net, &a).await, ids(&[&c, &d]));
    assert_eq!(config_peers(&t, &root, net, &c).await, ids(&[&a, &b]));

    // Quitar A–C. Una segunda vez ya no es automática y responde 404.
    let except = |x: &Device, y: &Device| {
        format!(
            "/api/v1/networks/{net}/devices/{}/auto-peers/{}",
            x.id, y.id
        )
    };
    assert_eq!(
        t.json("DELETE", &except(&a, &c), Some(&s), Value::Null)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    t.json("DELETE", &except(&c, &a), Some(&s), Value::Null)
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(audits(&t, net, "auto_peer.block").await, 1);
    assert!(auto_peers(&t, &s, net, &a).await.is_empty());
    assert_eq!(auto_peers(&t, &s, net, &c).await, ids(&[&b]));
    assert_eq!(auto_peers(&t, &s, net, &b).await, ids(&[&c]));
    // Una configuración nueva de C (reconexión) no restaura la pareja: la excepción persiste.
    assert_eq!(config_peers(&t, &root, net, &a).await, ids(&[&d]));
    assert_eq!(config_peers(&t, &root, net, &c).await, ids(&[&b]));

    // Revocar B lo excluye por la vía automática y por la configuración.
    assert!(t.action(&s, net, b.id, "revoke").await.status.is_success());
    assert!(auto_peers(&t, &s, net, &c).await.is_empty());
    assert!(auto_peers(&t, &s, net, &b).await.is_empty());
    assert!(config_peers(&t, &root, net, &c).await.is_empty());

    // El ajuste no toca la política manual.
    set_auto(&t, &s, net, true).await;
    set_auto(&t, &s, net, false).await;
    let policies = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/policies"),
            Some(&s),
            Value::Null,
        )
        .await;
    assert_eq!(policies.json.as_array().unwrap().len(), 1);
    t.finish().await;
}

#[tokio::test]
async fn pendiente_y_aprobacion_repetida_no_cambian_orden_ni_ajuste() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true, "10.135.0.0/16", "fd35::/64").await;
    let p = enrolled(&t, &s, net, false).await;
    assert_eq!(order(&t, &p).await, (0, false));
    assert!(auto_peers(&t, &s, net, &p).await.is_empty());
    approve(&t, &s, net, &p).await;
    assert_eq!(order(&t, &p).await, (1, true));
    t.action(&s, net, p.id, "approve")
        .await
        .error(StatusCode::CONFLICT, "device_not_pending");
    assert_eq!(order(&t, &p).await, (1, true));
    t.finish().await;
}

#[tokio::test]
async fn auto_peers_y_excepciones_respetan_la_red() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true, "10.136.0.0/16", "fd36::/64").await;
    let other = network(&t, &s, true, "10.137.0.0/16", "fd37::/64").await;
    let a = enrolled(&t, &s, net, true).await;
    let b = enrolled(&t, &s, net, true).await;
    let foreign = enrolled(&t, &s, other, true).await;
    t.json(
        "GET",
        &format!("/api/v1/networks/{other}/devices/{}/auto-peers", a.id),
        Some(&s),
        Value::Null,
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    // Un dispositivo consigo mismo, o una pareja que hoy no es automática, no admite excepción.
    t.json(
        "DELETE",
        &format!(
            "/api/v1/networks/{net}/devices/{}/auto-peers/{}",
            a.id, a.id
        ),
        Some(&s),
        Value::Null,
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    t.json(
        "DELETE",
        &format!(
            "/api/v1/networks/{net}/devices/{}/auto-peers/{}",
            a.id, foreign.id
        ),
        Some(&s),
        Value::Null,
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(auto_peers(&t, &s, net, &a).await, ids(&[&b]));
    t.finish().await;
}

#[tokio::test]
async fn alta_con_token_automatica_fija_orden_y_consume_el_token() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true, "10.133.0.0/16", "fd33::/64").await;
    let first = enrolled(&t, &s, net, true).await;
    assert_eq!(order(&t, &first).await, (1, true));
    let status: String = sqlx::query_scalar("SELECT status FROM devices WHERE id=$1")
        .bind(first.id)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(status, "approved");
    let second = Device::new();
    let token = t.token(&s, net, true).await;
    let spent = t.enroll(&token, &second).await;
    assert_eq!(spent.status, StatusCode::CREATED, "{}", spent.json);
    assert_eq!(spent.json["status"], "approved");
    assert_eq!(order(&t, &second).await, (2, true));
    t.enroll(&token, &Device::new())
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_token");
    assert_eq!(auto_peers(&t, &s, net, &first).await, ids(&[&second]));
    t.finish().await;
}

#[tokio::test]
async fn aprobaciones_concurrentes_numeran_sin_repetir_y_conectan_todos() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true, "10.132.0.0/16", "fd32::/64").await;
    let mut devices = Vec::new();
    for _ in 0..8 {
        devices.push(enrolled(&t, &s, net, false).await);
    }
    let mut tasks = tokio::task::JoinSet::new();
    for d in &devices {
        use tower::ServiceExt;
        let app = t.app.clone();
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/networks/{net}/devices/{}/approve", d.id))
            .header("cookie", format!("atenea_session={}", s.token))
            .header("x-csrf-token", &s.csrf)
            .body(Body::empty())
            .unwrap();
        tasks.spawn(async move { app.oneshot(request).await.unwrap().status() });
    }
    while let Some(r) = tasks.join_next().await {
        assert_eq!(r.unwrap(), StatusCode::OK);
    }
    let mut positions = Vec::new();
    for d in &devices {
        let (position, auto) = order(&t, d).await;
        assert!(auto);
        positions.push(position);
    }
    positions.sort_unstable();
    assert_eq!(positions, (1..=8).collect::<Vec<i64>>());
    for d in &devices {
        assert_eq!(auto_peers(&t, &s, net, d).await.len(), 7);
    }
    t.finish().await;
}

#[tokio::test]
async fn sin_capacidad_v3_la_aprobacion_se_rechaza_entera_y_se_acepta_al_anunciarla() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true, "10.122.0.0/16", "fd22::/64").await;
    // 129 agentes anteriores con conexión automática. Cada uno tiene 128 pares y 256 reglas (dos por par): es el
    // máximo v1/v2. Un par más por agente, al aprobar al siguiente, ya no cabe en ellos.
    let mut legacy = Vec::new();
    for n in 1..=129_u32 {
        let d = Device::new();
        sqlx::query("INSERT INTO devices (id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,approved_at,approval_order,auto_connect_on_approval) VALUES ($1,$2,'anterior','approved',$3,$4,$5,now(),$6,true)")
            .bind(d.id).bind(net).bind(d.bundle.noise_pubkey.to_vec()).bind(d.bundle.identity_pubkey.to_vec()).bind(d.bundle.encode().to_vec()).bind(i64::from(n))
            .execute(&t.pool).await.unwrap();
        for (family, addr) in [
            (4_i16, format!("10.122.{}.{}", n / 256, n % 256)),
            (6_i16, format!("fd22::{n:x}")),
        ] {
            sqlx::query("INSERT INTO ip_allocations (network_id,device_id,family,addr) VALUES ($1,$2,$3,$4::inet)")
                .bind(net).bind(d.id).bind(family).bind(addr).execute(&t.pool).await.unwrap();
        }
        legacy.push(d);
    }
    assert_eq!(auto_peers(&t, &s, net, &legacy[0]).await.len(), 128);
    // Un agente nuevo que anuncia v3 entra pendiente. Su aprobación añade un par a cada anterior (129 pares y 258
    // reglas): debe fallar entera, sin cambios parciales, y nombrar a un anterior.
    let newcomer = Device::new();
    let token = t.token(&s, net, false).await;
    let enrolled_reply = t
        .json(
            "POST",
            "/api/v1/enroll",
            None,
            json!({"token":token,"bundle":hex::encode(newcomer.bundle.encode()),"name":"nuevo","config_v3":true}),
        )
        .await;
    assert_eq!(
        enrolled_reply.status,
        StatusCode::CREATED,
        "{}",
        enrolled_reply.json
    );
    let refused = t.action(&s, net, newcomer.id, "approve").await;
    refused.error(StatusCode::CONFLICT, "config_v3_required");
    let blamed = refused.json["error"]["device_id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    assert!(legacy.iter().any(|d| d.id == blamed));
    let approved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM devices WHERE network_id=$1 AND status='approved'",
    )
    .bind(net)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(approved, 129);
    let state: String = sqlx::query_scalar("SELECT status FROM devices WHERE id=$1")
        .bind(newcomer.id)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(state, "pending");
    assert_eq!(order(&t, &newcomer).await, (0, false));
    let addresses: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ip_allocations WHERE device_id=$1")
            .bind(newcomer.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(addresses, 0);
    // Los anteriores anuncian v3 con su consulta firmada; después la aprobación sí se acepta.
    for d in &legacy {
        let r = report_at(
            &t,
            d,
            "/api/v1/device/status?config_v3=true",
            json!({"paths":[]}),
        )
        .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.json);
    }
    approve(&t, &s, net, &newcomer).await;
    assert_eq!(order(&t, &newcomer).await, (130, true));
    assert_eq!(auto_peers(&t, &s, net, &newcomer).await.len(), 129);
    assert!(
        auto_peers(&t, &s, net, &legacy[0])
            .await
            .contains(&newcomer.id)
    );
    t.finish().await;
}

#[tokio::test]
async fn capacidad_config_v3_llega_por_alta_union_y_consulta_firmada() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let net = network(&t, &s, true, "10.134.0.0/16", "fd34::/64").await;
    // La inscripción toma el campo JSON; ausente equivale a un agente anterior sin capacidad.
    let token = t.token(&s, net, false).await;
    let upgraded = Device::new();
    let enrolled_v3 = t
        .json(
            "POST",
            "/api/v1/enroll",
            None,
            json!({"token":token,"bundle":hex::encode(upgraded.bundle.encode()),"name":"v3","config_v3":true}),
        )
        .await;
    assert_eq!(
        enrolled_v3.status,
        StatusCode::CREATED,
        "{}",
        enrolled_v3.json
    );
    assert!(capability(&t, &upgraded).await);
    let legacy = enrolled(&t, &s, net, false).await;
    assert!(!capability(&t, &legacy).await);
    approve(&t, &s, net, &upgraded).await;
    approve(&t, &s, net, &legacy).await;

    // La consulta firmada anuncia la capacidad; sin consulta no rebaja lo ya anunciado.
    assert_eq!(
        report_at(
            &t,
            &legacy,
            "/api/v1/device/status?config_v3=true",
            json!({"paths":[]})
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(capability(&t, &legacy).await);
    assert_eq!(audits(&t, net, "device.config_v3").await, 1);
    spend_window(&t, &legacy).await;
    assert_eq!(
        report_at(&t, &legacy, "/api/v1/device/status", json!({"paths":[]}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(capability(&t, &legacy).await);
    // Repetir la misma declaración no cambia la marca ni audita.
    spend_window(&t, &legacy).await;
    assert_eq!(
        report_at(
            &t,
            &legacy,
            "/api/v1/device/status?config_v3=true",
            json!({"paths":[]})
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(capability(&t, &legacy).await);
    assert_eq!(audits(&t, net, "device.config_v3").await, 1);
    // Rebaja explícita: también se audita.
    spend_window(&t, &legacy).await;
    assert_eq!(
        report_at(
            &t,
            &legacy,
            "/api/v1/device/status?config_v3=false",
            json!({"paths":[]})
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(!capability(&t, &legacy).await);
    assert_eq!(audits(&t, net, "device.config_v3").await, 2);
    // Valor inválido: 400 y sin cambios.
    spend_window(&t, &legacy).await;
    report_at(
        &t,
        &legacy,
        "/api/v1/device/status?config_v3=si",
        json!({"paths":[]}),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_request");
    assert!(!capability(&t, &legacy).await);
    // Una firma hecha sin consulta no autoriza una consulta añadida.
    let body = b"{\"paths\":[]}".to_vec();
    let auth = atenea_identity::api_auth::sign_request(
        &legacy.key,
        *legacy.id.as_bytes(),
        "POST",
        "/api/v1/device/status",
        &body,
        now(),
        *Uuid::new_v4().as_bytes(),
    );
    let forged = t
        .send(
            Request::builder()
                .method("POST")
                .uri("/api/v1/device/status?config_v3=true")
                .header("content-type", "application/json")
                .header("authorization", auth)
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
    forged.error(StatusCode::UNAUTHORIZED, "invalid_signature");
    assert!(!capability(&t, &legacy).await);
    assert_eq!(audits(&t, net, "device.config_v3").await, 2);

    // Solicitud de unión: el mismo campo JSON.
    let policy = t
        .json(
            "PUT",
            &format!("/api/v1/networks/{net}/join-policy"),
            Some(&s),
            json!({"policy":"con_solicitud"}),
        )
        .await;
    assert_eq!(policy.status, StatusCode::OK, "{}", policy.json);
    let requested = Device::new();
    let joined = t
        .json(
            "POST",
            &format!("/api/v1/networks/{net}/join-requests"),
            None,
            json!({"bundle":hex::encode(requested.bundle.encode()),"name":"solicitud","config_v3":true}),
        )
        .await;
    assert_eq!(joined.status, StatusCode::CREATED, "{}", joined.json);
    assert!(capability(&t, &requested).await);
    t.finish().await;
}
