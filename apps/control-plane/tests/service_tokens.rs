// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;

use atenea_control_plane::config::Config;
use atenea_identity::{generate_seed, signing_key};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Reply, Session, TestApp, id};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
use uuid::Uuid;

async fn get(t: &TestApp, path: &str, token: &str) -> Reply {
    t.send(
        Request::builder()
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

async fn mint(t: &TestApp, owner: &Session, org: Uuid) -> (Uuid, String) {
    let r = t
        .json(
            "POST",
            &format!("/api/v1/orgs/{org}/service-tokens"),
            Some(owner),
            json!({"name":"asistente de ejemplo","ttl_secs":3600}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    (id(&r.json), r.json["token"].as_str().unwrap().to_owned())
}

/// Organización con una red y un token de servicio.
async fn fixture(t: &TestApp) -> (Session, Uuid, Uuid, Uuid, String) {
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.77.0.0/24", "fd77:77::/64")
        .await;
    let (token_id, token) = mint(t, &owner, org).await;
    (owner, org, net, token_id, token)
}

#[tokio::test]
async fn token_reads_inventory_of_own_org() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (_owner, org, net, _, token) = fixture(&t).await;
    for path in [
        "/api/v1/orgs".to_owned(),
        "/api/v1/server".to_owned(),
        format!("/api/v1/orgs/{org}/networks"),
        format!("/api/v1/networks/{net}"),
        format!("/api/v1/networks/{net}/devices"),
        format!("/api/v1/networks/{net}/policies"),
        format!("/api/v1/networks/{net}/routes"),
        format!("/api/v1/networks/{net}/dns"),
        format!("/api/v1/networks/{net}/audit"),
        format!("/api/v1/networks/{net}/metrics"),
    ] {
        let r = get(&t, &path, &token).await;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.json);
    }
    let orgs = get(&t, "/api/v1/orgs", &token).await.json;
    assert_eq!(orgs.as_array().unwrap().len(), 1);
    assert_eq!(orgs[0]["role"], "member");
    // ADR-015: el usuario de servicio es member y no ve correos de nadie.
    let members = get(&t, &format!("/api/v1/orgs/{org}/members"), &token).await;
    assert_eq!(members.status, StatusCode::OK);
    assert!(
        members
            .json
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["email"].is_null())
    );
    t.finish().await;
}

#[tokio::test]
async fn token_cannot_write() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (_owner, _org, _net, _, token) = fixture(&t).await;
    let audit_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    let document = serde_json::to_value(atenea_control_plane::openapi()).unwrap();
    let mut checked = 0;
    for (path, item) in document["paths"].as_object().unwrap() {
        for method in ["post", "put", "patch", "delete"] {
            if item.get(method).is_none() {
                continue;
            }
            let mut uri = path.clone();
            while let (Some(start), Some(end)) = (uri.find('{'), uri.find('}')) {
                uri.replace_range(start..=end, &Uuid::new_v4().to_string());
            }
            let mut request = Request::builder()
                .method(method.to_uppercase().as_str())
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap();
            request.extensions_mut().insert(axum::extract::ConnectInfo(
                std::net::SocketAddr::from(([127, 0, 0, 1], 1234)),
            ));
            t.send(request)
                .await
                .error(StatusCode::FORBIDDEN, "service_token_read_only");
            checked += 1;
        }
    }
    assert!(checked >= 20, "rutas de escritura comprobadas: {checked}");
    let audit_after: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(audit_before, audit_after);
    t.finish().await;
}

#[tokio::test]
async fn token_other_org_is_404() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (_owner, _org, _net, _, token) = fixture(&t).await;
    let (_other, other_org, other_net, _, _) = fixture(&t).await;
    for path in [
        format!("/api/v1/orgs/{other_org}/networks"),
        format!("/api/v1/orgs/{other_org}/members"),
        format!("/api/v1/networks/{other_net}"),
        format!("/api/v1/networks/{other_net}/devices"),
    ] {
        get(&t, &path, &token)
            .await
            .error(StatusCode::NOT_FOUND, "not_found");
    }
    t.finish().await;
}

#[tokio::test]
async fn token_expired_and_revoked_are_401_invalid_session() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (owner, org, _net, token_id, token) = fixture(&t).await;
    let path = format!("/api/v1/orgs/{org}/networks");
    assert_eq!(get(&t, &path, &token).await.status, StatusCode::OK);
    let unknown = get(&t, &path, &format!("atsvc_{}", "A".repeat(43))).await;
    unknown.error(StatusCode::UNAUTHORIZED, "invalid_session");
    // Caducado: indistinguible de un token desconocido.
    sqlx::query("UPDATE service_tokens SET expires_at = now() - interval '1 second' WHERE id=$1")
        .bind(token_id)
        .execute(&t.pool)
        .await
        .unwrap();
    let expired = get(&t, &path, &token).await;
    expired.error(StatusCode::UNAUTHORIZED, "invalid_session");
    assert_eq!(expired.json, unknown.json);
    // Revocado de inmediato.
    let (second_id, second) = mint(&t, &owner, org).await;
    assert_eq!(get(&t, &path, &second).await.status, StatusCode::OK);
    let r = t
        .json(
            "DELETE",
            &format!("/api/v1/orgs/{org}/service-tokens/{second_id}"),
            Some(&owner),
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    get(&t, &path, &second)
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_session");
    t.finish().await;
}

#[tokio::test]
async fn token_cannot_create_or_list_tokens() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (_owner, org, net, _, token) = fixture(&t).await;
    get(&t, &format!("/api/v1/orgs/{org}/service-tokens"), &token)
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    get(&t, "/api/v1/session", &token)
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    let post = t
        .send(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/orgs/{org}/service-tokens"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"x","ttl_secs":3600}"#))
                .unwrap(),
        )
        .await;
    post.error(StatusCode::FORBIDDEN, "service_token_read_only");
    // Eventos WebSocket: handshake real con el token en lugar de la cookie.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = t.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let mut request = format!("ws://{address}/api/v1/networks/{net}/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "http://127.0.0.1:8080".parse().unwrap());
    match connect_async(request).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => assert_eq!(r.status(), 403),
        other => panic!("el token no debe abrir eventos: {:?}", other.map(|_| ())),
    }
    server.abort();
    t.finish().await;
}

#[tokio::test]
async fn token_cannot_read_join_code() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (owner, org, net, _, token) = fixture(&t).await;
    get(&t, &format!("/api/v1/networks/{net}/join-code"), &token)
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    // El owner no puede ascender al usuario interno: sería saltarse el solo lectura.
    let service_user: Uuid =
        sqlx::query_scalar("SELECT user_id FROM service_tokens WHERE org_id=$1")
            .bind(org)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    let r = t
        .json(
            "PUT",
            &format!("/api/v1/orgs/{org}/members/{service_user}"),
            Some(&owner),
            json!({"role":"admin"}),
        )
        .await;
    r.error(StatusCode::FORBIDDEN, "forbidden");
    get(&t, &format!("/api/v1/networks/{net}/join-code"), &token)
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    t.finish().await;
}

#[tokio::test]
async fn only_owner_creates_token() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let admin = t.user().await;
    let stranger = t.user().await;
    sqlx::query("INSERT INTO memberships(org_id,user_id,role) VALUES($1,$2,'admin')")
        .bind(org)
        .bind(admin.user)
        .execute(&t.pool)
        .await
        .unwrap();
    let body = json!({"name":"x","ttl_secs":3600});
    let path = format!("/api/v1/orgs/{org}/service-tokens");
    t.json("POST", &path, Some(&admin), body.clone())
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    t.json("POST", &path, Some(&stranger), body.clone())
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    t.json("GET", &path, Some(&admin), json!({}))
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    for (bad, code) in [
        (json!({"name":"x","ttl_secs":3599}), "invalid_ttl"),
        (json!({"name":"x","ttl_secs":7776001}), "invalid_ttl"),
        (json!({"name":" ","ttl_secs":3600}), "invalid_name"),
    ] {
        t.json("POST", &path, Some(&owner), bad)
            .await
            .error(StatusCode::BAD_REQUEST, code);
    }
    t.finish().await;
}

#[tokio::test]
async fn token_value_shown_once_and_stored_hashed() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let (token_id, token) = mint(&t, &owner, org).await;
    assert!(
        token.starts_with("atsvc_") && token.len() == 6 + 43,
        "{token}"
    );
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM service_tokens WHERE id=$1")
        .bind(token_id)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(stored, Sha256::digest(token.as_bytes()).to_vec());
    let list = t
        .json(
            "GET",
            &format!("/api/v1/orgs/{org}/service-tokens"),
            Some(&owner),
            json!({}),
        )
        .await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.json.as_array().unwrap().len(), 1);
    let item = &list.json[0];
    for field in [
        "id",
        "name",
        "created_at",
        "expires_at",
        "last_used_at",
        "revoked_at",
    ] {
        assert!(item.get(field).is_some(), "falta {field}");
    }
    assert!(!list.json.to_string().contains(&token));
    assert!(item.get("token").is_none() && item.get("token_hash").is_none());
    // Ninguna fila guarda el valor en claro.
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE detail::text LIKE '%' || $1 || '%'",
    )
    .bind(&token)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
    t.finish().await;
}

#[tokio::test]
async fn use_audited_once_per_hour() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let (_owner, org, _net, token_id, token) = fixture(&t).await;
    let count = |t: &TestApp| {
        let pool = t.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM audit_events WHERE action='service_token.use'",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let path = format!("/api/v1/orgs/{org}/networks");
    for _ in 0..3 {
        assert_eq!(get(&t, &path, &token).await.status, StatusCode::OK);
    }
    assert_eq!(count(&t).await, 1);
    sqlx::query("UPDATE service_tokens SET last_used_at = now() - interval '2 hours' WHERE id=$1")
        .bind(token_id)
        .execute(&t.pool)
        .await
        .unwrap();
    assert_eq!(get(&t, &path, &token).await.status, StatusCode::OK);
    assert_eq!(count(&t).await, 2);
    for action in ["service_token.create"] {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action=$1")
            .bind(action)
            .fetch_one(&t.pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
    t.finish().await;
}

#[tokio::test]
async fn token_is_rate_limited() {
    let Some(t) = TestApp::configured(Config {
        service_per_minute: 2,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    let (_owner, org, _net, _, token) = fixture(&t).await;
    let path = format!("/api/v1/orgs/{org}/networks");
    assert_eq!(get(&t, &path, &token).await.status, StatusCode::OK);
    assert_eq!(get(&t, &path, &token).await.status, StatusCode::OK);
    get(&t, &path, &token)
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    t.finish().await;
}
