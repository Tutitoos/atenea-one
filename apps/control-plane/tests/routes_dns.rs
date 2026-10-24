mod common;
use atenea_identity::{AgentState, LocalKeys, generate_seed, signing_key, verify_config};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::*;
use serde_json::{Value, json};
use uuid::Uuid;
async fn signed(t: &TestApp, d: &Device, path: &str, value: Value) -> Reply {
    let bytes = serde_json::to_vec(&value).unwrap();
    let authorization = atenea_identity::api_auth::sign_request(
        &d.key,
        *d.id.as_bytes(),
        "POST",
        path,
        &bytes,
        now(),
        *Uuid::new_v4().as_bytes(),
    );
    t.send(
        Request::builder()
            .method("POST")
            .uri(path)
            .header("authorization", authorization)
            .header("content-type", "application/json")
            .body(Body::from(bytes))
            .unwrap(),
    )
    .await
}
async fn declare(t: &TestApp, d: &Device, routes: Value) -> Reply {
    signed(t, d, "/api/v1/device/routes", json!({"routes":routes})).await
}
async fn reset(t: &TestApp, d: &Device) {
    sqlx::query("UPDATE devices SET routes_declared_at=NULL WHERE id=$1")
        .bind(d.id)
        .execute(&t.pool)
        .await
        .unwrap();
}
fn verify(
    bytes: &[u8],
    net: Uuid,
    d: &Device,
    root: &atenea_identity::SigningKey,
) -> atenea_identity::DeviceConfig {
    verify_config(
        bytes,
        &LocalKeys {
            noise_pub: d.bundle.noise_pubkey,
            identity_pub: d.bundle.identity_pubkey,
        },
        &AgentState::new(
            *net.as_bytes(),
            *d.id.as_bytes(),
            root.verifying_key().to_bytes(),
        ),
        now(),
    )
    .unwrap()
    .config
}
#[tokio::test]
async fn rutas_dns_emision_y_autorizacion() {
    let Some(t) = TestApp::new().await else {
        return;
    };
    let owner = t.user().await;
    let foreign = t.user().await;
    let member = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.77.0.0/24", "fd77::/64")
        .await;
    sqlx::query("INSERT INTO memberships(org_id,user_id,role) VALUES($1,$2,'member')")
        .bind(org)
        .bind(member.user)
        .execute(&t.pool)
        .await
        .unwrap();
    let router = Device::new();
    let client = Device::new();
    let pending = Device::new();
    for d in [&router, &client] {
        let token = t.token(&owner, net, true).await;
        assert_eq!(t.enroll(&token, d).await.status, StatusCode::CREATED);
    }
    let token = t.token(&owner, net, false).await;
    t.enroll(&token, &pending).await;
    declare(&t, &pending, json!([]))
        .await
        .error(StatusCode::FORBIDDEN, "device_not_approved");
    t.certificate(&owner, net, &root).await;
    let first = t.config(&client, now(), *Uuid::new_v4().as_bytes()).await;
    let cfg = verify(&first.body, net, &client, &root);
    assert!(cfg.advertised.is_empty() && cfg.dns.servers.is_empty());
    let body = cfg.encode().unwrap();
    assert_eq!(body[0], 1);
    assert_eq!(&first.body[187..187 + body.len()], body.as_slice());
    for bad in [
        json!(["203.0.113.0/24"]),
        json!(["0.0.0.0/0"]),
        json!(["::/0"]),
        json!(["fc00::/7"]),
        json!(["10.0.0.0/7"]),
        json!(["192.168.1.1/24"]),
        json!(["192.168.0.0/16", "192.168.1.0/24"]),
        json!(vec!["192.168.1.0/24"; 9]),
    ] {
        assert_eq!(
            declare(&t, &router, bad).await.status,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        declare(&t, &router, json!(["192.168.50.0/24"]))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    declare(&t, &router, json!([]))
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    let routes_path = format!("/api/v1/networks/{net}/routes");
    t.json("GET", &routes_path, Some(&foreign), json!({}))
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    let routes = t.json("GET", &routes_path, Some(&member), json!({})).await;
    let route = routes.json[0]["id"].as_str().unwrap();
    let approve = format!("{routes_path}/{route}/approve");
    t.json("POST", &approve, Some(&member), json!({"snat":true}))
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    assert!(
        t.json("POST", &approve, Some(&owner), json!({}))
            .await
            .status
            .is_client_error()
    );
    assert_eq!(
        t.json("POST", &approve, Some(&owner), json!({"snat":true}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    reset(&t, &router).await;
    assert_eq!(
        declare(&t, &router, json!(["192.168.50.0/24", "10.77.0.0/25"]))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let routes = t.json("GET", &routes_path, Some(&owner), json!({})).await;
    assert_eq!(
        routes
            .json
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["prefix"] == "192.168.50.0/24")
            .unwrap()["status"],
        "approved"
    );
    let overlap = routes
        .json
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["prefix"] == "10.77.0.0/25")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    t.json(
        "POST",
        &format!("{routes_path}/{overlap}/approve"),
        Some(&owner),
        json!({"snat":false}),
    )
    .await
    .error(StatusCode::CONFLICT, "route_overlap");
    assert_eq!(
        declare(&t, &client, json!(["192.168.50.0/25"]))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let routes = t.json("GET", &routes_path, Some(&owner), json!({})).await;
    let overlap = routes
        .json
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["device_id"] == client.id.to_string())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    t.json(
        "POST",
        &format!("{routes_path}/{overlap}/approve"),
        Some(&owner),
        json!({"snat":false}),
    )
    .await
    .error(StatusCode::CONFLICT, "route_overlap");
    let dns_path = format!("/api/v1/networks/{net}/dns");
    for invalid in [
        json!({"servers":["8.8.8.8"],"domains":["lab.internal"]}),
        json!({"servers":[],"domains":["Lab.internal"]}),
        json!({"servers":[],"domains":["."]}),
    ] {
        assert_eq!(
            t.json("PUT", &dns_path, Some(&owner), invalid).await.status,
            StatusCode::BAD_REQUEST
        );
    }
    let dns = json!({"servers":["192.168.50.53","10.77.0.1"],"domains":["lab.internal"]});
    assert_eq!(
        t.json("PUT", &dns_path, Some(&member), dns.clone())
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.json("PUT", &dns_path, Some(&owner), dns.clone())
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.json("GET", &dns_path, Some(&member), json!({}))
            .await
            .json,
        dns
    );
    let p=t.json("POST",&format!("/api/v1/networks/{net}/policies"),Some(&owner),json!({"src":{"kind":"device","device_id":client.id},"dst":{"kind":"prefix","prefix":"192.168.50.0/25"},"proto":0})).await;
    assert_eq!(p.status, StatusCode::CREATED, "{}", p.json);
    let issued = t.config(&client, now(), *Uuid::new_v4().as_bytes()).await;
    assert_eq!(issued.status, StatusCode::OK, "{}", issued.json);
    let cfg = verify(&issued.body, net, &client, &root);
    assert_eq!(cfg.encode().unwrap()[0], 2);
    assert_eq!(cfg.peers.len(), 1);
    assert_eq!(
        cfg.peers[0].routes[0].addr,
        "192.168.50.0".parse::<std::net::IpAddr>().unwrap()
    );
    assert_eq!(cfg.dns.servers.len(), 2);
    let issued = t.config(&router, now(), *Uuid::new_v4().as_bytes()).await;
    let cfg = verify(&issued.body, net, &router, &root);
    assert_eq!(cfg.advertised.len(), 1);
    assert!(cfg.advertised[0].snat);
    let status = json!({"paths":[],"forwarding_ok":true,"dns":"applied","routes":[{"prefix":"192.168.50.0/24","state":"installed"}]});
    assert_eq!(
        signed(&t, &router, "/api/v1/device/status", status)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let inventory = t
        .json(
            "GET",
            &format!("/api/v1/networks/{net}/devices"),
            Some(&owner),
            json!({}),
        )
        .await;
    let report = &inventory
        .json
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == router.id.to_string())
        .unwrap()["report"];
    assert_eq!(report["forwarding_ok"], true);
    assert_eq!(report["dns"], "applied");
    assert_eq!(report["routes"][0]["state"], "installed");
    assert_eq!(
        t.json(
            "POST",
            &format!("{routes_path}/{route}/reject"),
            Some(&owner),
            json!({})
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let issued = t.config(&client, now(), *Uuid::new_v4().as_bytes()).await;
    assert_eq!(issued.status, StatusCode::OK, "{}", issued.json);
    let cfg = verify(&issued.body, net, &client, &root);
    assert!(cfg.peers.is_empty());
    assert!(cfg.rules.is_empty());
    assert_eq!(
        cfg.dns.servers,
        vec!["10.77.0.1".parse::<std::net::IpAddr>().unwrap()]
    );
    reset(&t, &router).await;
    assert_eq!(
        declare(&t, &router, json!([])).await.status,
        StatusCode::NO_CONTENT
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM device_routes WHERE device_id=$1")
        .bind(router.id)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT action FROM audit_events WHERE network_id=$1")
            .bind(net)
            .fetch_all(&t.pool)
            .await
            .unwrap();
    for action in ["route.declare", "route.approve", "route.reject", "dns.set"] {
        assert!(actions.iter().any(|a| a == action));
    }
    t.finish().await;
}
