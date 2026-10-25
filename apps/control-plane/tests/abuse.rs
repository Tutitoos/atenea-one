mod common;
use atenea_control_plane::config::Config;
use atenea_identity::{generate_seed, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::*;
use serde_json::json;
use uuid::Uuid;

fn enroll_request(peer: &str, real_ip: Option<&str>) -> Request<Body> {
    let d = Device::new();
    let mut b = Request::builder()
        .method("POST")
        .uri("/api/v1/enroll")
        .header("content-type", "application/json");
    if let Some(ip) = real_ip {
        b = b.header("x-real-ip", ip);
    }
    let mut r = b
        .body(Body::from(
            json!({"token":"inexistente","bundle":hex::encode(d.bundle.encode()),"name":"límite"})
                .to_string(),
        ))
        .unwrap();
    r.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
            peer.parse().unwrap(),
            1,
        )));
    r
}

#[tokio::test]
async fn ip_cliente_solo_se_toma_de_proxy_confiable() {
    let Some(t) = TestApp::configured(Config {
        enroll_per_minute: 2,
        trusted_proxies: vec!["198.51.100.0/24".parse().unwrap()],
        ..Config::default()
    })
    .await
    else {
        return;
    };
    // Tras el proxy confiable cada cliente real tiene su cubo.
    for (ip, status) in [
        ("203.0.113.1", StatusCode::UNAUTHORIZED),
        ("203.0.113.1", StatusCode::UNAUTHORIZED),
        ("203.0.113.1", StatusCode::TOO_MANY_REQUESTS),
        ("203.0.113.2", StatusCode::UNAUTHORIZED),
    ] {
        let r = t.send(enroll_request("198.51.100.7", Some(ip))).await;
        assert_eq!(r.status, status, "{ip}");
    }
    // Un peer no confiable no puede elegir su cubo con la cabecera.
    for (spoof, status) in [
        ("203.0.113.10", StatusCode::UNAUTHORIZED),
        ("203.0.113.11", StatusCode::UNAUTHORIZED),
        ("203.0.113.12", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let r = t.send(enroll_request("192.0.2.50", Some(spoof))).await;
        assert_eq!(r.status, status, "{spoof}");
    }
    t.finish().await;
}

#[tokio::test]
async fn login_limitado_por_ip_y_con_tope_de_flujos() {
    let Some(t) = TestApp::configured(Config {
        login_per_minute: 1,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    let login = || {
        let mut r = Request::builder()
            .uri("/auth/login")
            .body(Body::empty())
            .unwrap();
        r.extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                "192.0.2.9".parse().unwrap(),
                1,
            )));
        r
    };
    assert_ne!(t.send(login()).await.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(t.send(login()).await.status, StatusCode::TOO_MANY_REQUESTS);
    t.finish().await;

    // Tope: con los flujos vigentes llenos se rechaza antes de consultar al IdP.
    let Some(t) = TestApp::new().await else {
        return;
    };
    sqlx::query("INSERT INTO oidc_flows(state_hash,pkce_verifier,nonce,expires_at) SELECT sha256(convert_to(g::text,'UTF8')),'v','n',now()+interval '5 minutes' FROM generate_series(1,10000) g")
        .execute(&t.pool)
        .await
        .unwrap();
    t.send(login())
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    t.finish().await;
}

#[tokio::test]
async fn purga_flujos_y_sesiones_caducados() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let live = t.user().await;
    sqlx::query("INSERT INTO oidc_flows(state_hash,pkce_verifier,nonce,expires_at) VALUES ('\\x01','v','n',now()-interval '1 minute'),('\\x02','v','n',now()+interval '5 minutes')")
        .execute(&t.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(id,user_id,csrf_hash,csrf_token,created_at,expires_at,last_seen_at) SELECT sha256('viejo'),user_id,csrf_hash,csrf_token,now()-interval '9 hours',now()-interval '1 hour',now()-interval '2 hours' FROM sessions LIMIT 1")
        .execute(&t.pool)
        .await
        .unwrap();
    atenea_control_plane::auth::purge(&t.pool).await.unwrap();
    let flows: i64 = sqlx::query_scalar("SELECT count(*) FROM oidc_flows")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!((flows, sessions), (1, 1));
    let _ = live;
    t.finish().await;
}

async fn approved(t: &TestApp) -> (Session, Uuid, Device) {
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t.network(&s, org, &root, "10.61.0.0/24", "fd61::/64").await;
    let token = t.token(&s, net, true).await;
    let d = Device::new();
    assert_eq!(t.enroll(&token, &d).await.status, StatusCode::CREATED);
    t.certificate(&s, net, &root).await;
    (s, net, d)
}

#[tokio::test]
async fn config_limitada_por_dispositivo_y_auditoria_acotada() {
    let Some(t) = TestApp::configured(Config {
        device_per_minute: 2,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    let (_, _, d) = approved(&t).await;
    for expected in [
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::TOO_MANY_REQUESTS,
    ] {
        let r = t.config(&d, now(), *Uuid::new_v4().as_bytes()).await;
        assert_eq!(r.status, expected, "{}", r.json);
    }
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='config.issue'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(audited, 1);
    t.finish().await;
}

#[tokio::test]
async fn dispositivo_pendiente_no_toma_el_bloqueo_de_la_red() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    let net = t.network(&s, org, &root, "10.62.0.0/24", "fd62::/64").await;
    let token = t.token(&s, net, false).await;
    let d = Device::new();
    t.enroll(&token, &d).await;
    // Con la fila de la red bloqueada, un pendiente se rechaza sin esperar al bloqueo.
    let mut c = connection(&t.pool).await;
    sqlx::query("BEGIN").execute(&mut c).await.unwrap();
    sqlx::query("SELECT id FROM networks WHERE id=$1 FOR UPDATE")
        .bind(net)
        .execute(&mut c)
        .await
        .unwrap();
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        t.config(&d, now(), *Uuid::new_v4().as_bytes()),
    )
    .await
    .expect("no debe esperar al bloqueo");
    r.error(StatusCode::FORBIDDEN, "device_not_approved");
    sqlx::query("ROLLBACK").execute(&mut c).await.unwrap();
    t.finish().await;
}

#[tokio::test]
async fn estado_con_firma_invalida_no_bloquea_la_fila_del_dispositivo() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (_, _, d) = approved(&t).await;
    let mut c = connection(&t.pool).await;
    sqlx::query("BEGIN").execute(&mut c).await.unwrap();
    sqlx::query("SELECT id FROM devices WHERE id=$1 FOR UPDATE")
        .bind(d.id)
        .execute(&mut c)
        .await
        .unwrap();
    let other = Device::new();
    let header = atenea_identity::api_auth::sign_request(
        &other.key,
        *d.id.as_bytes(),
        "POST",
        "/api/v1/device/status",
        b"{}",
        now(),
        [7; 16],
    );
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/device/status")
        .header("authorization", header)
        .body(Body::from("{}"))
        .unwrap();
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), t.send(request))
        .await
        .expect("la firma se verifica antes de bloquear");
    r.error(StatusCode::UNAUTHORIZED, "invalid_signature");
    sqlx::query("ROLLBACK").execute(&mut c).await.unwrap();
    t.finish().await;
}

#[tokio::test]
async fn cidr_de_red_debe_ser_privado_y_no_demasiado_amplio() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let s = t.user().await;
    let org = t.org(&s).await;
    let root = signing_key(generate_seed());
    for (v4, v6) in [
        ("0.0.0.0/0", "fd63::/64"),
        ("8.0.0.0/8", "fd63::/64"),
        ("10.0.0.0/7", "fd63::/64"),
        ("10.63.0.0/24", "::/0"),
        ("10.63.0.0/24", "2001:db8::/32"),
        ("10.63.0.0/24", "fc00::/16"),
    ] {
        let r = t.json("POST", &format!("/api/v1/orgs/{org}/networks"), Some(&s), json!({"name":Uuid::new_v4().to_string(),"ipv4_cidr":v4,"ipv6_cidr":v6,"root_anchor":hex::encode(root.verifying_key().to_bytes())})).await;
        r.error(StatusCode::BAD_REQUEST, "invalid_cidr");
    }
    t.network(&s, org, &root, "10.0.0.0/8", "fc00::/32").await;
    // La restricción de base de datos también protege altas fuera de la API.
    let direct = sqlx::query("INSERT INTO networks(id,org_id,name,ipv4_cidr,ipv6_cidr,root_anchor) VALUES($1,$2,'x','8.0.0.0/8','fd64::/64',$3)")
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(vec![1u8; 32])
        .execute(&t.pool)
        .await;
    assert!(direct.is_err());
    t.finish().await;
}

#[tokio::test]
async fn endpoints_fijados_se_sanean() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (s, net, d) = approved(&t).await;
    for bad in [
        json!(["127.0.0.1:1234"]),
        json!(["224.0.0.1:1234"]),
        json!(["0.0.0.0:1234"]),
        json!(["[::1]:1234"]),
        json!(["192.0.2.1:0"]),
    ] {
        let r = t
            .json(
                "PATCH",
                &format!("/api/v1/networks/{net}/devices/{}", d.id),
                Some(&s),
                json!({"endpoints":bad}),
            )
            .await;
        r.error(StatusCode::BAD_REQUEST, "invalid_endpoints");
    }
    sqlx::query("UPDATE devices SET endpoints=$1 WHERE id=$2")
        .bind(json!(["127.0.0.1:1234", "192.0.2.8:443", "[fe80::1]:443"]))
        .bind(d.id)
        .execute(&t.pool)
        .await
        .unwrap();
    let mut connection = t.pool.acquire().await.unwrap();
    let devices = atenea_control_plane::issue::load_devices(&mut connection, net)
        .await
        .unwrap();
    assert_eq!(
        devices[0].endpoints,
        vec!["192.0.2.8:443".parse::<std::net::SocketAddr>().unwrap()]
    );
    drop(connection);
    t.finish().await;
}

#[tokio::test]
async fn token_sin_cache_y_revocacion_idempotente() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (s, net, d) = approved(&t).await;
    let r = t
        .json(
            "POST",
            &format!("/api/v1/networks/{net}/enrollment-tokens"),
            Some(&s),
            json!({"ttl_secs":600}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.headers["cache-control"], "no-store");
    assert_eq!(r.headers["x-content-type-options"], "nosniff");
    assert_eq!(
        t.action(&s, net, d.id, "revoke").await.status,
        StatusCode::OK
    );
    let first: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT revoked_at FROM devices WHERE id=$1")
            .bind(d.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(
        t.action(&s, net, d.id, "revoke").await.status,
        StatusCode::OK
    );
    let second: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT revoked_at FROM devices WHERE id=$1")
            .bind(d.id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(first, second);
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='device.revoke'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(audits, 1);
    t.action(&s, net, Uuid::new_v4(), "revoke")
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    t.finish().await;
}

#[tokio::test]
async fn tope_de_flujos_oidc_es_duro_bajo_concurrencia() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    sqlx::query("INSERT INTO oidc_flows(state_hash,pkce_verifier,nonce,expires_at) SELECT sha256(convert_to(g::text,'UTF8')),'v','n',now()+interval '5 minutes' FROM generate_series(1,9990) g")
        .execute(&t.pool)
        .await
        .unwrap();
    let tasks: Vec<_> = (0..40u32)
        .map(|i| {
            let pool = t.pool.clone();
            tokio::spawn(async move {
                atenea_control_plane::auth::insert_flow(&pool, &i.to_be_bytes(), "v", "n")
                    .await
                    .is_ok()
            })
        })
        .collect();
    let mut ok = 0;
    for task in tasks {
        ok += usize::from(task.await.unwrap());
    }
    assert_eq!(ok, 10);
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM oidc_flows")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(total, 10_000);
    t.finish().await;
}
