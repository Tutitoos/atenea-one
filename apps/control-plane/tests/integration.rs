mod common;
use atenea_identity::{
    AgentState, DeviceBundle, LocalKeys, Selector, generate_seed, signing_key, verify_config,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Executor, Row};
use uuid::Uuid;

#[tokio::test]
async fn aislamiento_rbac_y_ultimo_owner() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let member = t.user().await;
    let stranger = t.user().await;
    let a = t.org(&owner).await;
    let b = t.org(&stranger).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, a, &root, "10.20.0.0/24", "fd20::/64")
        .await;
    let other = t
        .network(&stranger, b, &root, "10.21.0.0/24", "fd21::/64")
        .await;
    let r = t
        .json(
            "PUT",
            &format!("/api/v1/orgs/{a}/members/{}", member.user),
            Some(&owner),
            json!({"role":"member"}),
        )
        .await;
    assert!(r.status.is_success());
    for (method, path, body) in [
        ("GET", format!("/api/v1/networks/{other}"), json!(null)),
        ("GET", format!("/api/v1/orgs/{b}/networks"), json!(null)),
        ("DELETE", format!("/api/v1/networks/{other}"), json!(null)),
        (
            "POST",
            format!("/api/v1/networks/{other}/enrollment-tokens"),
            json!({"ttl_secs":600}),
        ),
        (
            "GET",
            format!("/api/v1/networks/{other}/devices"),
            json!(null),
        ),
        (
            "GET",
            format!("/api/v1/networks/{other}/policies"),
            json!(null),
        ),
        (
            "GET",
            format!("/api/v1/networks/{other}/audit"),
            json!(null),
        ),
    ] {
        assert_eq!(
            t.json(method, &path, Some(&owner), body).await.status,
            StatusCode::NOT_FOUND
        );
    }
    let orgs = t
        .json("GET", "/api/v1/orgs", Some(&owner), json!(null))
        .await;
    assert_eq!(orgs.json.as_array().unwrap().len(), 1);
    let dev = Device::new();
    let token = t.token(&owner, net, false).await;
    assert_eq!(t.enroll(&token, &dev).await.status, StatusCode::CREATED);
    for (method, path, body) in [
        (
            "POST",
            format!("/api/v1/networks/{net}/enrollment-tokens"),
            json!({"ttl_secs":600}),
        ),
        (
            "POST",
            format!("/api/v1/networks/{net}/devices/{}/approve", dev.id),
            json!({}),
        ),
        (
            "POST",
            format!("/api/v1/networks/{net}/devices/{}/revoke", dev.id),
            json!({}),
        ),
        (
            "POST",
            format!("/api/v1/networks/{net}/policies"),
            json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":0,"port_min":0,"port_max":0}),
        ),
        (
            "PUT",
            format!("/api/v1/orgs/{a}/members/{}", owner.user),
            json!({"role":"member"}),
        ),
    ] {
        assert_eq!(
            t.json(method, &path, Some(&member), body).await.status,
            StatusCode::FORBIDDEN
        );
    }
    for method in ["PUT", "DELETE"] {
        let r = t
            .json(
                method,
                &format!("/api/v1/orgs/{a}/members/{}", owner.user),
                Some(&owner),
                json!({"role":"member"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CONFLICT);
    }
    t.json(
        "PUT",
        &format!("/api/v1/orgs/{a}/members/{}", member.user),
        Some(&owner),
        json!({"role":"admin"}),
    )
    .await;
    assert_eq!(
        t.json(
            "PUT",
            &format!("/api/v1/orgs/{a}/members/{}", owner.user),
            Some(&member),
            json!({"role":"admin"})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let admin_token = t.token(&member, net, false).await;
    assert!(!admin_token.is_empty());
    t.finish().await;
}

#[tokio::test]
async fn csrf_sesion_absoluta_e_inactividad() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let session = t
        .json("GET", "/api/v1/session", Some(&s), json!(null))
        .await;
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.json["csrf_token"], s.csrf);
    for csrf in [None, Some("incorrecto")] {
        let mut r = Request::builder()
            .method("POST")
            .uri("/api/v1/orgs")
            .header("cookie", format!("atenea_session={}", s.token))
            .header("content-type", "application/json");
        if let Some(csrf) = csrf {
            r = r.header("x-csrf-token", csrf);
        }
        t.send(r.body(Body::from(r#"{"name":"CSRF"}"#)).unwrap())
            .await
            .error(StatusCode::FORBIDDEN, "csrf");
    }
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second'")
        .execute(&t.pool)
        .await
        .unwrap();
    assert_eq!(
        t.json("GET", "/api/v1/session", Some(&s), json!(null))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let idle = t.user().await;
    sqlx::query("UPDATE sessions SET last_seen_at=now()-interval '61 minutes' WHERE user_id=$1")
        .bind(idle.user)
        .execute(&t.pool)
        .await
        .unwrap();
    assert_eq!(
        t.json("GET", "/api/v1/session", Some(&idle), json!(null))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        t.json("GET", "/api/v1/session", None, json!(null))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let logout = t.user().await;
    assert_eq!(
        t.json("POST", "/auth/logout", Some(&logout), json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.json("GET", "/api/v1/session", Some(&logout), json!(null))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    t.finish().await;
}

#[tokio::test]
async fn tokens_caducados_inexistentes_usados_y_consumo_atomico() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let net = t
        .network(
            &s,
            org,
            &signing_key(generate_seed()),
            "10.22.0.0/24",
            "fd22::/64",
        )
        .await;
    let expired = t.token(&s, net, false).await;
    sqlx::query("UPDATE enrollment_tokens SET expires_at=now()-interval '1 second'")
        .execute(&t.pool)
        .await
        .unwrap();
    for token in [&expired, "inexistente"] {
        t.enroll(token, &Device::new())
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_token");
    }
    let token = t.token(&s, net, false).await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let app = t.app.clone();
        let token = token.clone();
        tasks.spawn(async move {
            use tower::ServiceExt;
            let d=Device::new();
            let request=Request::builder().method("POST").uri("/api/v1/enroll").header("content-type","application/json")
                .body(Body::from(json!({"token":token,"bundle":hex::encode(d.bundle.encode()),"name":"paralelo"}).to_string())).unwrap();
            let r=app.oneshot(request).await.unwrap();let status=r.status();
            let body=axum::body::to_bytes(r.into_body(),65536).await.unwrap();
            (status,serde_json::from_slice::<serde_json::Value>(&body).unwrap())
        });
    }
    let mut wins = 0;
    while let Some(r) = tasks.join_next().await {
        let (status, body) = r.unwrap();
        if status == StatusCode::CREATED {
            wins += 1;
        } else {
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(body["error"]["code"], "invalid_token");
        }
    }
    assert_eq!(wins, 1);
    t.enroll(&token, &Device::new())
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_token");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM devices WHERE network_id=$1")
        .bind(net)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    t.finish().await;
}

#[tokio::test]
async fn bundles_invalidos_y_colisiones_revierten_token() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let net = t
        .network(
            &s,
            org,
            &signing_key(generate_seed()),
            "10.23.0.0/24",
            "fd23::/64",
        )
        .await;
    let first = Device::new();
    let token = t.token(&s, net, false).await;
    assert_eq!(t.enroll(&token, &first).await.status, StatusCode::CREATED);
    let mut invalid = Device::new();
    invalid.bundle.proof[0] ^= 1;
    let token = t.token(&s, net, false).await;
    assert_eq!(
        t.enroll(&token, &invalid).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.enroll(&token, &Device::new()).await.status,
        StatusCode::CREATED
    );
    for collision in ["device", "noise", "identity"] {
        let mut dev = Device::new();
        if collision == "device" {
            dev.id = first.id;
        }
        if collision == "identity" {
            dev.key = first.key.clone();
        }
        let noise = if collision == "noise" {
            first.bundle.noise_pubkey
        } else {
            dev.bundle.noise_pubkey
        };
        dev.bundle = DeviceBundle::create(*dev.id.as_bytes(), noise, &dev.key);
        let token = t.token(&s, net, false).await;
        assert_eq!(
            t.enroll(&token, &dev).await.status,
            StatusCode::CONFLICT,
            "{collision}"
        );
        let consumed: bool = sqlx::query_scalar(
            "SELECT consumed_at IS NOT NULL FROM enrollment_tokens WHERE token_hash=$1",
        )
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .fetch_one(&t.pool)
        .await
        .unwrap();
        assert!(!consumed);
        assert_eq!(
            t.enroll(&token, &Device::new()).await.status,
            StatusCode::CREATED
        );
    }
    t.finish().await;
}

#[tokio::test]
async fn ipam_concurrente_libera_y_agota_red() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let net = t
        .network(
            &s,
            org,
            &signing_key(generate_seed()),
            "10.24.0.0/28",
            "fd24::/120",
        )
        .await;
    let mut devices = Vec::new();
    for _ in 0..14 {
        let d = Device::new();
        let token = t.token(&s, net, false).await;
        assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
        devices.push(d);
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
        assert!(r.unwrap().is_success());
    }
    let rows=sqlx::query("SELECT family,count(*) AS n,count(DISTINCT addr) AS distinct_n FROM ip_allocations WHERE network_id=$1 GROUP BY family")
        .bind(net).fetch_all(&t.pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.get::<i64, _>("n"), 14);
        assert_eq!(row.get::<i64, _>("distinct_n"), 14);
    }
    // La restricción única debe proteger también a un escritor SQL que eluda el bloqueo IPAM.
    let duplicate=sqlx::query("INSERT INTO ip_allocations (network_id,device_id,family,addr) SELECT network_id,device_id,family,addr FROM ip_allocations LIMIT 1").execute(&t.pool).await.unwrap_err();
    assert_eq!(
        duplicate.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    let extra = Device::new();
    let token = t.token(&s, net, false).await;
    t.enroll(&token, &extra).await;
    assert_eq!(
        t.action(&s, net, extra.id, "approve").await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        t.action(&s, net, devices[0].id, "approve").await.status,
        StatusCode::CONFLICT
    );
    let freed: Vec<String> = sqlx::query_scalar(
        "SELECT host(addr) FROM ip_allocations WHERE device_id=$1 ORDER BY family",
    )
    .bind(devices[0].id)
    .fetch_all(&t.pool)
    .await
    .unwrap();
    assert!(
        t.action(&s, net, devices[0].id, "revoke")
            .await
            .status
            .is_success()
    );
    assert!(
        t.action(&s, net, extra.id, "approve")
            .await
            .status
            .is_success()
    );
    let reused: Vec<String> = sqlx::query_scalar(
        "SELECT host(addr) FROM ip_allocations WHERE device_id=$1 ORDER BY family",
    )
    .bind(extra.id)
    .fetch_all(&t.pool)
    .await
    .unwrap();
    assert_eq!(freed, reused);
    t.finish().await;
}

#[tokio::test]
async fn emision_verificada_politicas_inversas_revocacion_y_autenticacion() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t.network(&s, org, &root, "10.25.0.0/24", "fd25::/64").await;
    let mut devices = Vec::new();
    for approved in [true, true, true, false] {
        let d = Device::new();
        let token = t.token(&s, net, approved).await;
        assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
        devices.push(d);
    }
    let a = &devices[0];
    let b = &devices[1];
    let revoked = &devices[2];
    let pending = &devices[3];
    t.config(a, now(), *Uuid::new_v4().as_bytes())
        .await
        .error(StatusCode::SERVICE_UNAVAILABLE, "issuer_unavailable");
    t.config(pending, now(), *Uuid::new_v4().as_bytes())
        .await
        .error(StatusCode::FORBIDDEN, "device_not_approved");
    t.certificate(&s, net, &root).await;
    let policy=t.json("POST",&format!("/api/v1/networks/{net}/policies"),Some(&s),json!({"src":{"kind":"device","device_id":a.id},"dst":{"kind":"any"},"proto":6,"port_min":443,"port_max":443})).await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.json);
    assert!(
        t.action(&s, net, revoked.id, "revoke")
            .await
            .status
            .is_success()
    );
    t.config(revoked, now(), *Uuid::new_v4().as_bytes())
        .await
        .error(StatusCode::FORBIDDEN, "device_not_approved");
    let local = LocalKeys {
        noise_pub: a.bundle.noise_pubkey,
        identity_pub: a.bundle.identity_pubkey,
    };
    let mut state = AgentState::new(
        *net.as_bytes(),
        *a.id.as_bytes(),
        root.verifying_key().to_bytes(),
    );
    let nonce = *Uuid::new_v4().as_bytes();
    let ts = now();
    let first = t.config(a, ts, nonce).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.json);
    assert_eq!(first.headers["content-type"], "application/octet-stream");
    assert_eq!(first.headers["cache-control"], "no-store");
    let verified = verify_config(&first.body, &local, &state, now()).unwrap();
    assert_eq!(verified.config.peers.len(), 1);
    assert_eq!(verified.config.peers[0].device_id, *b.id.as_bytes());
    assert_eq!(verified.config.mtu, 1400);
    assert_eq!(verified.config.tunnel_addrs.len(), 2);
    assert!(
        verified
            .config
            .rules
            .iter()
            .any(|r| r.src == Selector::Device(*a.id.as_bytes())
                && r.dst == Selector::Any
                && r.proto == 6
                && r.port_min == 443
                && r.port_max == 443)
    );
    assert!(verified.config.rules.iter().any(|r| r.src == Selector::Any
        && r.dst == Selector::Device(*a.id.as_bytes())
        && r.proto == 6
        && r.port_min == 0
        && r.port_max == 0));
    assert_eq!(
        first.headers["x-config-version"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        verified.config.config_version
    );
    state = verified.state;
    let second = t.config(a, now(), *Uuid::new_v4().as_bytes()).await;
    let verified = verify_config(&second.body, &local, &state, now()).unwrap();
    assert!(verified.config.config_version > state.max_config_version);
    t.config(a, ts, nonce)
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    for ts in [now() - 61, now() + 61] {
        t.config(a, ts, *Uuid::new_v4().as_bytes())
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }
    let mut forged = Device::new();
    forged.id = a.id;
    t.config(&forged, now(), *Uuid::new_v4().as_bytes())
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_signature");
    assert!(
        t.json(
            "DELETE",
            &format!("/api/v1/networks/{net}/policies/{}", id(&policy.json)),
            Some(&s),
            json!(null)
        )
        .await
        .status
        .is_success()
    );
    let empty = t.config(a, now(), *Uuid::new_v4().as_bytes()).await;
    let verified = verify_config(&empty.body, &local, &verified.state, now()).unwrap();
    assert!(verified.config.peers.is_empty());
    assert!(verified.config.rules.is_empty());
    t.finish().await;
}

#[tokio::test]
async fn auditoria_escrituras_y_rol_sin_update_delete() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t.network(&s, org, &root, "10.26.0.0/24", "fd26::/64").await;
    let token = t.token(&s, net, false).await;
    let dev = Device::new();
    t.enroll(&token, &dev).await;
    t.action(&s, net, dev.id, "approve").await;
    t.certificate(&s, net, &root).await;
    let member = t.user().await;
    t.json(
        "PUT",
        &format!("/api/v1/orgs/{org}/members/{}", member.user),
        Some(&s),
        json!({"role":"member"}),
    )
    .await;
    let p = t
        .json(
            "POST",
            &format!("/api/v1/networks/{net}/policies"),
            Some(&s),
            json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":0,"port_min":0,"port_max":0}),
        )
        .await;
    assert_eq!(p.status, StatusCode::CREATED);
    t.json(
        "PATCH",
        &format!("/api/v1/networks/{net}/devices/{}", dev.id),
        Some(&s),
        json!({"name":"renombrado","endpoints":["192.0.2.1:1234"]}),
    )
    .await;
    assert_eq!(
        t.config(&dev, now(), *Uuid::new_v4().as_bytes())
            .await
            .status,
        StatusCode::OK
    );
    t.json(
        "DELETE",
        &format!("/api/v1/networks/{net}/policies/{}", id(&p.json)),
        Some(&s),
        json!(null),
    )
    .await;
    t.action(&s, net, dev.id, "revoke").await;
    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_events")
        .fetch_all(&t.pool)
        .await
        .unwrap();
    for action in [
        "org.create",
        "member.set",
        "network.create",
        "network.issuer_cert",
        "token.create",
        "device.enroll",
        "device.approve",
        "device.revoke",
        "device.update",
        "policy.create",
        "policy.delete",
        "config.issue",
    ] {
        assert!(
            actions.iter().any(|a| a == action),
            "falta {action}: {actions:?}"
        );
    }
    let page = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/audit?limit=2"),
            Some(&member),
            json!(null),
        )
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(page.json.as_array().unwrap().len(), 2);
    let role = format!("atenea_test_role_{}", Uuid::new_v4().simple());
    t.pool
        .execute(format!("CREATE ROLE {role} NOLOGIN").as_str())
        .await
        .unwrap();
    atenea_control_plane::db::migrate(&t.pool, Some(&role))
        .await
        .unwrap();
    let mut conn = connection(&t.pool).await;
    conn.execute(format!("SET ROLE {role}").as_str())
        .await
        .unwrap();
    for statement in [
        "UPDATE audit_events SET action='alterado'",
        "DELETE FROM audit_events",
    ] {
        let e = conn.execute(statement).await.unwrap_err();
        assert_eq!(
            e.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    conn.execute("INSERT INTO audit_events (actor_kind,actor_id,action,target,detail) VALUES ('system','test','test.insert','test','{}')").await.unwrap();
    conn.execute("RESET ROLE").await.unwrap();
    drop(conn);
    t.pool
        .execute(format!("DROP OWNED BY {role}").as_str())
        .await
        .unwrap();
    t.pool
        .execute(format!("DROP ROLE {role}").as_str())
        .await
        .unwrap();
    t.finish().await;
}

#[test]
fn openapi_publicado_coincide_con_codigo() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.json");
    let checked: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("contrato OpenAPI publicado")).unwrap();
    let generated = serde_json::to_value(atenea_control_plane::openapi()).unwrap();
    assert_eq!(
        checked, generated,
        "regenere docs/api/openapi.json desde el código"
    );
}

#[tokio::test]
async fn limite_256_dispositivos_aunque_queden_direcciones() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let net = t
        .network(
            &s,
            org,
            &signing_key(generate_seed()),
            "10.27.0.0/16",
            "fd27::/64",
        )
        .await;
    // La preparación SQL evita 256 llamadas sin sustituir la base ni el camino de aprobación comprobado.
    for n in 1..=256_u32 {
        let d = Device::new();
        sqlx::query("INSERT INTO devices (id,network_id,name,status,noise_pubkey,identity_pubkey,bundle,approved_at) VALUES ($1,$2,'capacidad','approved',$3,$4,$5,now())")
            .bind(d.id).bind(net).bind(d.bundle.noise_pubkey.to_vec()).bind(d.bundle.identity_pubkey.to_vec()).bind(d.bundle.encode().to_vec()).execute(&t.pool).await.unwrap();
        for (family, addr) in [
            (4_i16, format!("10.27.{}.{}", n / 256, n % 256)),
            (6_i16, format!("fd27::{n:x}")),
        ] {
            sqlx::query("INSERT INTO ip_allocations (network_id,device_id,family,addr) VALUES ($1,$2,$3,$4::inet)")
                .bind(net).bind(d.id).bind(family).bind(addr).execute(&t.pool).await.unwrap();
        }
    }
    let extra = Device::new();
    let token = t.token(&s, net, false).await;
    assert_eq!(t.enroll(&token, &extra).await.status, StatusCode::CREATED);
    t.action(&s, net, extra.id, "approve")
        .await
        .error(StatusCode::CONFLICT, "network_full");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ip_allocations WHERE device_id=$1")
        .bind(extra.id)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    t.finish().await;
}

#[tokio::test]
async fn limites_politicas_validan_selectores_puertos_y_reglas_compiladas() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let net = t
        .network(
            &s,
            org,
            &signing_key(generate_seed()),
            "10.28.0.0/24",
            "fd28::/64",
        )
        .await;
    let d = Device::new();
    let token = t.token(&s, net, true).await;
    assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
    let path = format!("/api/v1/networks/{net}/policies");
    for policy in [
        json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":6,"port_min":100,"port_max":99}),
        json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":1,"port_min":80,"port_max":80}),
        json!({"src":{"kind":"any"},"dst":{"kind":"any"},"proto":99,"port_min":0,"port_max":0}),
        json!({"src":{"kind":"device","device_id":Uuid::new_v4()},"dst":{"kind":"any"},"proto":0,"port_min":0,"port_max":0}),
        json!({"src":{"kind":"prefix","prefix":"192.0.2.0/24"},"dst":{"kind":"any"},"proto":0,"port_min":0,"port_max":0}),
        json!({"src":{"kind":"prefix","prefix":"fd29::/64"},"dst":{"kind":"any"},"proto":0,"port_min":0,"port_max":0}),
        json!({"src":{"kind":"desconocido"},"dst":{"kind":"any"},"proto":0,"port_min":0,"port_max":0}),
    ] {
        let r = t.json("POST", &path, Some(&s), policy).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.json);
    }
    // Cada prefijo distinto produce dos reglas relevantes: directa e inversa sin puertos.
    for n in 2..=129 {
        let r=t.json("POST",&path,Some(&s),json!({"src":{"kind":"any"},"dst":{"kind":"prefix","prefix":format!("10.28.0.{n}/32")},"proto":6,"port_min":443,"port_max":443})).await;
        assert_eq!(r.status, StatusCode::CREATED, "prefijo {n}: {}", r.json);
    }
    t.json("POST",&path,Some(&s),json!({"src":{"kind":"any"},"dst":{"kind":"prefix","prefix":"10.28.0.130/32"},"proto":6,"port_min":443,"port_max":443})).await.error(StatusCode::CONFLICT,"policy_overflow");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM access_policies WHERE network_id=$1")
        .bind(net)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 128);
    t.finish().await;
}

#[tokio::test]
async fn cidr_solapados_y_certificados_fuera_de_confianza() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t.network(&s, org, &root, "10.29.0.0/24", "fd29::/64").await;
    for (v4, v6) in [
        ("10.29.0.128/25", "fd30::/64"),
        ("10.30.0.0/24", "fd29::/80"),
    ] {
        let r=t.json("POST",&format!("/api/v1/orgs/{org}/networks"),Some(&s),json!({"name":Uuid::new_v4().to_string(),"ipv4_cidr":v4,"ipv6_cidr":v6,"root_anchor":hex::encode(root.verifying_key().to_bytes())})).await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.json);
    }
    let foreign = signing_key(generate_seed());
    let certs = [
        atenea_identity::IssuerCert::issue(
            &root,
            *Uuid::new_v4().as_bytes(),
            1,
            now() - 60,
            now() + 3600,
            t.issuer.verifying_key().to_bytes(),
            None,
        )
        .unwrap(),
        atenea_identity::IssuerCert::issue(
            &foreign,
            *net.as_bytes(),
            1,
            now() - 60,
            now() + 3600,
            t.issuer.verifying_key().to_bytes(),
            None,
        )
        .unwrap(),
        atenea_identity::IssuerCert::issue(
            &root,
            *net.as_bytes(),
            1,
            now() - 4000,
            now() - 100,
            t.issuer.verifying_key().to_bytes(),
            None,
        )
        .unwrap(),
        atenea_identity::IssuerCert::issue(
            &root,
            *net.as_bytes(),
            1,
            now() + 3600,
            now() + 7200,
            t.issuer.verifying_key().to_bytes(),
            None,
        )
        .unwrap(),
    ];
    let mut bodies: Vec<Vec<u8>> = certs.iter().map(|c| c.encode().to_vec()).collect();
    bodies.push(vec![0; 177]);
    let mut damaged = atenea_identity::IssuerCert::issue(
        &root,
        *net.as_bytes(),
        1,
        now() - 60,
        now() + 3600,
        t.issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap()
    .encode()
    .to_vec();
    damaged[177] ^= 1;
    bodies.push(damaged);
    for body in bodies {
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/networks/{net}/issuer-cert"))
            .header("cookie", format!("atenea_session={}", s.token))
            .header("x-csrf-token", &s.csrf)
            .header("content-type", "application/octet-stream")
            .body(Body::from(body))
            .unwrap();
        let r = t.send(request).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.json);
    }
    let certificate: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT issuer_cert FROM networks WHERE id=$1")
            .bind(net)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert!(certificate.is_none());
    t.certificate(&s, net, &root).await;
    t.finish().await;
}

#[tokio::test]
async fn limite_enrolamiento_separa_ips_de_origen() {
    let config = atenea_control_plane::config::Config {
        enroll_per_minute: 2,
        ..Default::default()
    };
    let Some(t) = TestApp::configured(config).await else {
        return;
    };
    for (ip, status) in [
        ("192.0.2.10", StatusCode::UNAUTHORIZED),
        ("192.0.2.10", StatusCode::UNAUTHORIZED),
        ("192.0.2.10", StatusCode::TOO_MANY_REQUESTS),
        ("192.0.2.11", StatusCode::UNAUTHORIZED),
    ] {
        let d = Device::new();
        let mut request=Request::builder().method("POST").uri("/api/v1/enroll").header("content-type","application/json")
            .body(Body::from(json!({"token":"token-inexistente","bundle":hex::encode(d.bundle.encode()),"name":"límite"}).to_string())).unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                ip.parse().unwrap(),
                12345,
            )));
        let r = t.send(request).await;
        assert_eq!(r.status, status, "origen {ip}: {}", r.json);
    }
    t.finish().await;
}
