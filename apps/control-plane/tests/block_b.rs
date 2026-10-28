// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::*;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

async fn membership(t: &TestApp, owner: &Session, org: Uuid, user: &Session, role: &str) {
    let r = t
        .json(
            "PUT",
            &format!("/api/v1/orgs/{org}/members/{}", user.user),
            Some(owner),
            json!({"role":role}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
}

#[tokio::test]
async fn members_email_lookup_privacy_roles_and_audit() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let admin = t.user().await;
    let member = t.user().await;
    let target = t.user().await;
    let foreign = t.user().await;
    let org = t.org(&owner).await;
    membership(&t, &owner, org, &admin, "admin").await;
    membership(&t, &owner, org, &member, "member").await;
    sqlx::query("UPDATE users SET email='Alice@example.test',display_name='Alicia' WHERE id=$1")
        .bind(target.user)
        .execute(&t.pool)
        .await
        .unwrap();
    let path = format!("/api/v1/orgs/{org}/members");
    for denied in [&admin, &member] {
        t.json(
            "POST",
            &path,
            Some(denied),
            json!({"email":"Alice@example.test","role":"member"}),
        )
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    }
    t.json(
        "POST",
        &path,
        Some(&foreign),
        json!({"email":"Alice@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    for email in [
        "alice",
        "alice@example.test.evil",
        " Alice@example.test",
        "missing@example.test",
    ] {
        t.json(
            "POST",
            &path,
            Some(&owner),
            json!({"email":email,"role":"member"}),
        )
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    }
    t.json(
        "POST",
        &path,
        Some(&owner),
        json!({"email":"Alice@example.test","role":"invalid"}),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_role");
    for (email, role, previous) in [
        ("ALICE@EXAMPLE.TEST", "member", Value::Null),
        ("Alice@example.test", "admin", json!("member")),
    ] {
        let r = t
            .json(
                "POST",
                &path,
                Some(&owner),
                json!({"email":email,"role":role}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
        assert_eq!(
            r.json,
            json!({"user_id":target.user,"role":role,"display_name":"Alicia","email":"Alice@example.test"})
        );
        let detail: Value = sqlx::query_scalar("SELECT detail FROM audit_events WHERE action='member.set' AND target=$1 ORDER BY id DESC LIMIT 1").bind(target.user.to_string()).fetch_one(&t.pool).await.unwrap();
        assert_eq!(detail, json!({"previous_role":previous,"role":role}));
    }
    membership(&t, &owner, org, &target, "member").await;
    let detail: Value = sqlx::query_scalar("SELECT detail FROM audit_events WHERE action='member.set' AND target=$1 ORDER BY id DESC LIMIT 1").bind(target.user.to_string()).fetch_one(&t.pool).await.unwrap();
    assert_eq!(detail, json!({"previous_role":"admin","role":"member"}));
    for viewer in [&owner, &admin, &member] {
        let r = t.json("GET", &path, Some(viewer), Value::Null).await;
        assert_eq!(r.status, StatusCode::OK);
        let alice = r
            .json
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["user_id"] == target.user.to_string())
            .unwrap();
        assert_eq!(alice["display_name"], "Alicia");
        assert_eq!(
            alice["email"],
            if viewer.user == member.user {
                Value::Null
            } else {
                json!("Alice@example.test")
            }
        );
    }
    t.json("GET", &path, Some(&foreign), Value::Null)
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    sqlx::query("UPDATE users SET email='owner@example.test' WHERE id=$1")
        .bind(owner.user)
        .execute(&t.pool)
        .await
        .unwrap();
    t.json(
        "POST",
        &path,
        Some(&owner),
        json!({"email":"owner@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::CONFLICT, "last_owner");
    let r = t
        .json(
            "POST",
            &path,
            Some(&owner),
            json!({"email":"Alice@example.test","role":"owner"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["role"], "owner");
    let bad_csrf = Session {
        user: owner.user,
        token: owner.token.clone(),
        csrf: "incorrecto".into(),
    };
    t.json(
        "POST",
        &path,
        Some(&bad_csrf),
        json!({"email":"Alice@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::FORBIDDEN, "csrf");
    t.json(
        "POST",
        &path,
        None,
        json!({"email":"Alice@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::UNAUTHORIZED, "invalid_session");
    sqlx::query("UPDATE users SET email=NULL,display_name=NULL WHERE id=$1")
        .bind(target.user)
        .execute(&t.pool)
        .await
        .unwrap();
    let r = t.json("GET", &path, Some(&member), Value::Null).await;
    let alice = r
        .json
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["user_id"] == target.user.to_string())
        .unwrap();
    assert_eq!(alice["display_name"], Value::Null);
    assert_eq!(alice["email"], Value::Null);
    t.finish().await;
}

#[tokio::test]
async fn email_lookup_rate_is_per_session_and_ambiguous_email_is_not_selected() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let other = t.user().await;
    let org = t.org(&owner).await;
    membership(&t, &owner, org, &other, "owner").await;
    let path = format!("/api/v1/orgs/{org}/members");
    t.json(
        "POST",
        &path,
        Some(&owner),
        json!({"email":"test@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::CONFLICT, "conflict");
    for _ in 1..10 {
        t.json(
            "POST",
            &path,
            Some(&owner),
            json!({"email":"missing@example.test","role":"member"}),
        )
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    }
    t.json(
        "POST",
        &path,
        Some(&owner),
        json!({"email":"missing@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    t.json(
        "POST",
        &path,
        Some(&other),
        json!({"email":"missing@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    let second = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sessions SELECT $1,user_id,csrf_hash,csrf_token,now(),now()+interval '8 hours',now() FROM sessions WHERE user_id=$2 LIMIT 1").bind(sha2::Sha256::digest(second.as_bytes()).to_vec()).bind(owner.user).execute(&t.pool).await.unwrap();
    let session = Session {
        user: owner.user,
        token: second,
        csrf: owner.csrf.clone(),
    };
    t.json(
        "POST",
        &path,
        Some(&session),
        json!({"email":"missing@example.test","role":"member"}),
    )
    .await
    .error(StatusCode::NOT_FOUND, "not_found");
    t.finish().await;
}
use sha2::Digest;

#[tokio::test]
async fn rename_organization_and_network_permissions_constraints_and_audit() {
    let Some(t) = TestApp::configured(atenea_control_plane::config::Config {
        public_url: "https://panel.example.test".into(),
        ..Default::default()
    })
    .await
    else {
        return;
    };
    let owner = t.user().await;
    let admin = t.user().await;
    let member = t.user().await;
    let foreign = t.user().await;
    let org = t.org(&owner).await;
    let other_org = t.org(&foreign).await;
    membership(&t, &owner, org, &admin, "admin").await;
    membership(&t, &owner, org, &member, "member").await;
    let root = atenea_identity::signing_key(atenea_identity::generate_seed());
    let net = t
        .network(&owner, org, &root, "10.42.0.0/24", "fd42::/64")
        .await;
    let other = t
        .network(&owner, org, &root, "10.43.0.0/24", "fd43::/64")
        .await;
    let foreign_net = t
        .network(&foreign, other_org, &root, "10.42.0.0/24", "fd42::/64")
        .await;
    let org_path = format!("/api/v1/orgs/{org}");
    let net_path = format!("/api/v1/networks/{net}");
    for denied in [&admin, &member] {
        t.json("PATCH", &org_path, Some(denied), json!({"name":"Nuevo"}))
            .await
            .error(StatusCode::FORBIDDEN, "forbidden");
    }
    t.json("PATCH", &org_path, Some(&foreign), json!({"name":"Nuevo"}))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    let old_org: String = sqlx::query_scalar("SELECT name FROM organizations WHERE id=$1")
        .bind(org)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    let r = t
        .json(
            "PATCH",
            &org_path,
            Some(&owner),
            json!({"name":"Organización nueva"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json, json!({"id":org,"name":"Organización nueva"}));
    let detail: Value = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE action='org.rename' AND org_id=$1",
    )
    .bind(org)
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(
        detail,
        json!({"previous_name":old_org,"name":"Organización nueva"})
    );
    for bad in [" ".to_string(), "x".repeat(256)] {
        for path in [&org_path, &net_path] {
            t.json("PATCH", path, Some(&owner), json!({"name":bad}))
                .await
                .error(StatusCode::BAD_REQUEST, "invalid_name");
        }
    }
    t.json("PATCH", &net_path, Some(&member), json!({"name":"Nueva"}))
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    t.json("PATCH", &net_path, Some(&foreign), json!({"name":"Nueva"}))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    let code_path = format!("{net_path}/join-code");
    let code = t
        .json("GET", &code_path, Some(&owner), Value::Null)
        .await
        .json["code"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_name = atenea_identity::join_code::JoinCode::decode(&code)
        .unwrap()
        .name;
    for actor in [&owner, &admin] {
        let r = t
            .json("PATCH", &net_path, Some(actor), json!({"name":"Red nueva"}))
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
        assert_eq!(r.json["name"], "Red nueva");
    }
    assert_eq!(
        atenea_identity::join_code::JoinCode::decode(&code)
            .unwrap()
            .name,
        old_name
    );
    let current = t.json("GET", &code_path, Some(&owner), Value::Null).await;
    assert_eq!(
        atenea_identity::join_code::JoinCode::decode(current.json["code"].as_str().unwrap())
            .unwrap()
            .name,
        "Red nueva"
    );
    let detail:Value=sqlx::query_scalar("SELECT detail FROM audit_events WHERE action='network.rename' AND network_id=$1 ORDER BY id LIMIT 1").bind(net).fetch_one(&t.pool).await.unwrap();
    assert_eq!(detail, json!({"previous_name":old_name,"name":"Red nueva"}));
    t.json(
        "PATCH",
        &format!("/api/v1/networks/{other}"),
        Some(&owner),
        json!({"name":"Red nueva"}),
    )
    .await
    .error(StatusCode::CONFLICT, "conflict");
    assert_eq!(
        t.json(
            "PATCH",
            &format!("/api/v1/networks/{foreign_net}"),
            Some(&foreign),
            json!({"name":"Red nueva"})
        )
        .await
        .status,
        StatusCode::OK
    );
    t.json(
        "PATCH",
        &net_path,
        Some(&owner),
        json!({"name":"x".repeat(129)}),
    )
    .await
    .error(StatusCode::CONFLICT, "invalid_network_name_for_code");
    let bad_csrf = Session {
        user: owner.user,
        token: owner.token.clone(),
        csrf: "incorrecto".into(),
    };
    for path in [&org_path, &net_path] {
        t.json("PATCH", path, Some(&bad_csrf), json!({"name":"No"}))
            .await
            .error(StatusCode::FORBIDDEN, "csrf");
    }
    t.finish().await;
}

#[tokio::test]
async fn network_certificate_expiry_uses_stored_certificate_without_exposing_it() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let foreign = t.user().await;
    let org = t.org(&owner).await;
    let root = atenea_identity::signing_key(atenea_identity::generate_seed());
    let net = t
        .network(&owner, org, &root, "10.44.0.0/24", "fd44::/64")
        .await;
    let path = format!("/api/v1/networks/{net}");
    for list in [false, true] {
        let url = if list {
            format!("/api/v1/orgs/{org}/networks")
        } else {
            path.clone()
        };
        let r = t.json("GET", &url, Some(&owner), Value::Null).await;
        let network = if list { &r.json[0] } else { &r.json };
        assert_eq!(network.get("issuer_cert_not_after"), Some(&Value::Null));
    }
    let expires = now() + 2400;
    let cert = atenea_identity::IssuerCert::issue(
        &root,
        *net.as_bytes(),
        7,
        now() - 60,
        expires,
        t.issuer.verifying_key().to_bytes(),
        None,
    )
    .unwrap();
    let r = t
        .send(
            Request::builder()
                .method("PUT")
                .uri(format!("{path}/issuer-cert"))
                .header("cookie", format!("atenea_session={}", owner.token))
                .header("x-csrf-token", &owner.csrf)
                .body(Body::from(cert.encode().to_vec()))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    for url in [path.clone(), format!("/api/v1/orgs/{org}/networks")] {
        let r = t.json("GET", &url, Some(&owner), Value::Null).await;
        let network = if r.json.is_array() {
            &r.json[0]
        } else {
            &r.json
        };
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(
                network["issuer_cert_not_after"].as_str().unwrap()
            )
            .unwrap()
            .timestamp(),
            cert.not_after as i64
        );
        assert!(network.get("issuer_cert").is_none());
        t.json("GET", &url, Some(&foreign), Value::Null)
            .await
            .error(StatusCode::NOT_FOUND, "not_found");
    }
    t.finish().await;
}

#[tokio::test]
async fn session_server_information_requires_membership_and_limits_rate() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let user = t.user().await;
    t.json("GET", "/api/v1/server", None, Value::Null)
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_session");
    t.json("GET", "/api/v1/server", Some(&user), Value::Null)
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    let org = t.org(&user).await;
    for _ in 0..60 {
        let r = t
            .json("GET", "/api/v1/server", Some(&user), Value::Null)
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
        assert_eq!(r.headers["cache-control"], "no-store");
        assert_eq!(
            r.json["control_plane"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            r.json["control_plane"]["commit"],
            option_env!("ATENEA_BUILD_COMMIT").unwrap_or("unknown")
        );
        assert_eq!(r.json.as_object().unwrap().len(), 2);
        assert!(
            (chrono::DateTime::parse_from_rfc3339(r.json["server_time"].as_str().unwrap())
                .unwrap()
                .timestamp()
                - now() as i64)
                .abs()
                < 5
        );
    }
    t.json("GET", "/api/v1/server", Some(&user), Value::Null)
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    let member = t.user().await;
    membership(&t, &user, org, &member, "member").await;
    assert_eq!(
        t.json("GET", "/api/v1/server", Some(&member), Value::Null)
            .await
            .status,
        StatusCode::OK
    );
    let count = sqlx::query("SELECT count(*) AS n FROM audit_events WHERE action='member.set'")
        .fetch_one(&t.pool)
        .await
        .unwrap()
        .get::<i64, _>("n");
    assert_eq!(count, 1);
    t.finish().await;
}

#[tokio::test]
async fn rename_network_rejects_incompatible_server_url_without_mutation() {
    let Some(t) = TestApp::configured(atenea_control_plane::config::Config {
        public_url: "http://panel.example.test".into(),
        ..Default::default()
    })
    .await
    else {
        return;
    };
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let root = atenea_identity::signing_key(atenea_identity::generate_seed());
    let net = t
        .network(&owner, org, &root, "10.45.0.0/24", "fd45::/64")
        .await;
    let path = format!("/api/v1/networks/{net}");
    let before = t.json("GET", &path, Some(&owner), Value::Null).await.json;
    t.json(
        "PATCH",
        &path,
        Some(&owner),
        json!({"name":"Nombre válido"}),
    )
    .await
    .error(StatusCode::CONFLICT, "invalid_public_url_for_code");
    assert_eq!(
        t.json("GET", &path, Some(&owner), Value::Null).await.json,
        before
    );
    let audit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action='network.rename'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(audit, 0);
    t.finish().await;
}
