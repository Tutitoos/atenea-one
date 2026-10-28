// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-013 fase 1: política de unión, código de red y solicitudes de unión sin token.
mod common;

use atenea_control_plane::config::Config;
use atenea_identity::{
    SigningKey, anchor_fingerprint, generate_seed, join_code::JoinCode, signing_key,
};
use axum::http::StatusCode;
use common::{Device, Reply, Session, TestApp, now};
use serde_json::{Value, json};
use uuid::Uuid;

const URL: &str = "https://control.example.test";

fn config(max: u32) -> Config {
    Config {
        enroll_per_minute: 10000,
        public_url: URL.into(),
        max_pending_join_requests: max,
        ..Config::default()
    }
}

struct Lab {
    t: TestApp,
    owner: Session,
    org: Uuid,
    root: SigningKey,
    net: Uuid,
}

async fn lab(config: Config) -> Option<Lab> {
    let t = TestApp::configured(config).await?;
    let owner = t.user().await;
    let org = t.org(&owner).await;
    let root = signing_key(generate_seed());
    let net = t
        .network(&owner, org, &root, "10.77.0.0/24", "fd77::/64")
        .await;
    Some(Lab {
        t,
        owner,
        org,
        root,
        net,
    })
}

impl Lab {
    async fn policy(&self, net: Uuid, value: Value) -> Reply {
        self.t
            .json(
                "PUT",
                &format!("/api/v1/networks/{net}/join-policy"),
                Some(&self.owner),
                json!({"policy":value}),
            )
            .await
    }
    async fn open(&self, net: Uuid) {
        let r = self.policy(net, json!("con_solicitud")).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }
    async fn second_network(&self) -> Uuid {
        let net = self
            .t
            .network(
                &self.owner,
                self.org,
                &self.root,
                "10.78.0.0/24",
                "fd78::/64",
            )
            .await;
        self.open(net).await;
        net
    }
    async fn request(&self, net: Uuid, device: &Device) -> Reply {
        self.t
            .json(
                "POST",
                &format!("/api/v1/networks/{net}/join-requests"),
                None,
                json!({"bundle":hex::encode(device.bundle.encode()),"name":"equipo sintético"}),
            )
            .await
    }
    async fn devices(&self, net: Uuid) -> Value {
        let r = self
            .t
            .json(
                "GET",
                &format!("/api/v1/networks/{net}/devices"),
                Some(&self.owner),
                Value::Null,
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
        r.json
    }
    async fn audit_actions(&self, net: Uuid) -> Vec<String> {
        let r = self
            .t
            .json(
                "GET",
                &format!("/api/v1/networks/{net}/audit"),
                Some(&self.owner),
                Value::Null,
            )
            .await;
        r.json
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["action"].as_str().unwrap().to_owned())
            .collect()
    }
    async fn count(&self, net: Uuid, origin: &str, status: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM devices WHERE network_id=$1 AND join_origin=$2 AND status=$3",
        )
        .bind(net)
        .bind(origin)
        .bind(status)
        .fetch_one(&self.t.pool)
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn politica_por_defecto_cambio_permisos_y_auditoria() {
    let Some(l) = lab(config(20)).await else {
        return;
    };
    let get =
        l.t.json(
            "GET",
            &format!("/api/v1/networks/{}", l.net),
            Some(&l.owner),
            Value::Null,
        )
        .await;
    assert_eq!(get.json["join_policy"], "cerrada");
    let member = l.t.user().await;
    let stranger = l.t.user().await;
    let r =
        l.t.json(
            "PUT",
            &format!("/api/v1/orgs/{}/members/{}", l.org, member.user),
            Some(&l.owner),
            json!({"role":"member"}),
        )
        .await;
    assert!(r.status.is_success());
    let path = format!("/api/v1/networks/{}/join-policy", l.net);
    let body = json!({"policy":"con_solicitud"});
    l.t.json("PUT", &path, Some(&member), body.clone())
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    l.t.json("PUT", &path, Some(&stranger), body.clone())
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(
        l.policy(l.net, json!("cerrada")).await.status,
        StatusCode::OK
    );
    for bad in [
        json!("abierta"),
        json!(""),
        json!(1),
        json!(null),
        json!("Cerrada"),
    ] {
        l.policy(l.net, bad)
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let r = l.t.json("PUT", &path, Some(&l.owner), json!({})).await;
    r.error(StatusCode::BAD_REQUEST, "invalid_request");
    // Sin sesión no hay cambio.
    assert_eq!(
        l.t.json("PUT", &path, None, body).await.status,
        StatusCode::UNAUTHORIZED
    );
    let ok = l.policy(l.net, json!("con_solicitud")).await;
    assert_eq!(ok.json, json!({"policy":"con_solicitud"}));
    let get =
        l.t.json(
            "GET",
            &format!("/api/v1/networks/{}", l.net),
            Some(&l.owner),
            Value::Null,
        )
        .await;
    assert_eq!(get.json["join_policy"], "con_solicitud");
    assert!(
        l.audit_actions(l.net)
            .await
            .contains(&"network.join_policy".to_owned())
    );
    l.t.finish().await;
}

#[tokio::test]
async fn red_cerrada_e_inexistente_responden_igual() {
    let Some(l) = lab(config(20)).await else {
        return;
    };
    let device = Device::new();
    let closed = l.request(l.net, &device).await;
    let missing = l.request(Uuid::new_v4(), &device).await;
    closed.error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(closed.status, missing.status);
    assert_eq!(closed.body, missing.body);
    assert_eq!(l.count(l.net, "request", "pending").await, 0);
    l.t.finish().await;
}

#[tokio::test]
async fn solicitud_pendiente_aprobable_y_recibe_configuracion() {
    let Some(l) = lab(config(20)).await else {
        return;
    };
    l.open(l.net).await;
    l.t.certificate(&l.owner, l.net, &l.root).await;
    let device = Device::new();
    let r = l.request(l.net, &device).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(
        r.json,
        json!({"device_id":device.id,"network_id":l.net,"status":"pending"})
    );
    let items = l.devices(l.net).await;
    let item = &items.as_array().unwrap()[0];
    assert_eq!(item["status"], "pending");
    assert_eq!(item["join_origin"], "request");
    assert_eq!(item["fingerprint"], device.bundle.fingerprint());
    // Pendiente: aún sin configuración.
    assert_eq!(
        l.t.config(&device, now(), [1; 16]).await.status,
        StatusCode::FORBIDDEN
    );
    let a = l.t.action(&l.owner, l.net, device.id, "approve").await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.json);
    assert_eq!(
        l.t.config(&device, now(), [2; 16]).await.status,
        StatusCode::OK
    );
    let actions = l.audit_actions(l.net).await;
    assert!(actions.contains(&"device.join_requested".to_owned()));
    // La auditoría no guarda el bundle completo.
    let detail: String = sqlx::query_scalar(
        "SELECT detail::text FROM audit_events WHERE action='device.join_requested'",
    )
    .fetch_one(&l.t.pool)
    .await
    .unwrap();
    assert!(!detail.contains(&hex::encode(device.bundle.encode())));
    // Misma clave otra vez: conflicto como en enroll.
    l.request(l.net, &device)
        .await
        .error(StatusCode::CONFLICT, "conflict");
    l.t.finish().await;
}

#[tokio::test]
async fn tope_caducidad_y_aislamiento_entre_redes() {
    let Some(l) = lab(config(3)).await else {
        return;
    };
    l.open(l.net).await;
    let other = l.second_network().await;
    let devices: Vec<Device> = (0..6).map(|_| Device::new()).collect();
    for d in &devices[..3] {
        assert_eq!(l.request(l.net, d).await.status, StatusCode::CREATED);
    }
    // La N+1 se rechaza y no crea nada; la otra red no se ve afectada.
    l.request(l.net, &devices[3])
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "too_many_pending");
    assert_eq!(l.count(l.net, "request", "pending").await, 3);
    assert_eq!(l.count(l.net, "request", "pending").await, 3);
    assert_eq!(
        l.request(other, &devices[3]).await.status,
        StatusCode::CREATED
    );
    assert_eq!(l.count(other, "request", "pending").await, 1);
    assert_eq!(l.count(l.net, "request", "pending").await, 3);
    // Aprobar una libera hueco.
    let a = l.t.action(&l.owner, l.net, devices[0].id, "approve").await;
    assert_eq!(a.status, StatusCode::OK, "{}", a.json);
    assert_eq!(
        l.request(l.net, &devices[4]).await.status,
        StatusCode::CREATED
    );
    l.request(l.net, &devices[5])
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "too_many_pending");
    // Un pendiente de origen token también envejece, y no debe tocarse.
    let token = l.t.token(&l.owner, l.net, false).await;
    let by_token = Device::new();
    assert_eq!(
        l.t.enroll(&token, &by_token).await.status,
        StatusCode::CREATED
    );
    sqlx::query("UPDATE devices SET enrolled_at=now()-interval '8 days' WHERE network_id=$1")
        .bind(l.net)
        .execute(&l.t.pool)
        .await
        .unwrap();
    assert_eq!(
        l.request(l.net, &devices[5]).await.status,
        StatusCode::CREATED
    );
    // Solo se eliminaron las solicitudes pendientes viejas (devices 1, 2 y 4).
    let left: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id,join_origin,status FROM devices WHERE network_id=$1 ORDER BY id")
            .bind(l.net)
            .fetch_all(&l.t.pool)
            .await
            .unwrap();
    let mut expected = vec![
        (devices[0].id, "request".to_owned(), "approved".to_owned()),
        (by_token.id, "token".to_owned(), "pending".to_owned()),
        (devices[5].id, "request".to_owned(), "pending".to_owned()),
    ];
    expected.sort();
    assert_eq!(left, expected);
    // La otra red no se tocó.
    assert_eq!(l.count(other, "request", "pending").await, 1);
    l.t.finish().await;
}

#[tokio::test]
async fn concurrencia_nunca_supera_el_tope() {
    let Some(l) = lab(config(3)).await else {
        return;
    };
    l.open(l.net).await;
    let devices: Vec<Device> = (0..16).map(|_| Device::new()).collect();
    let replies = futures_util::future::join_all(devices.iter().map(|d| l.request(l.net, d))).await;
    let created = replies
        .iter()
        .filter(|r| r.status == StatusCode::CREATED)
        .count();
    assert_eq!(created, 3);
    for r in replies.iter().filter(|r| r.status != StatusCode::CREATED) {
        r.error(StatusCode::TOO_MANY_REQUESTS, "too_many_pending");
    }
    assert_eq!(l.count(l.net, "request", "pending").await, 3);
    l.t.finish().await;
}

#[tokio::test]
async fn limite_por_ip_igual_que_enroll() {
    let Some(l) = lab(Config {
        enroll_per_minute: 2,
        ..config(20)
    })
    .await
    else {
        return;
    };
    l.open(l.net).await;
    for _ in 0..2 {
        assert_eq!(
            l.request(l.net, &Device::new()).await.status,
            StatusCode::CREATED
        );
    }
    l.request(l.net, &Device::new())
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    l.t.finish().await;
}

#[tokio::test]
async fn validacion_de_nombre_y_bundle_igual_que_enroll() {
    let Some(l) = lab(config(20)).await else {
        return;
    };
    l.open(l.net).await;
    let device = Device::new();
    let good = hex::encode(device.bundle.encode());
    let mut tampered = device.bundle.encode();
    tampered[100] ^= 1;
    let long = "a".repeat(65);
    for (bundle, name, code) in [
        (good.clone(), "", "invalid_name"),
        (good.clone(), "  ", "invalid_name"),
        (good.clone(), "a\nb", "invalid_name"),
        (good.clone(), "a\u{202e}b", "invalid_name"),
        (good.clone(), long.as_str(), "invalid_name"),
        ("zz".to_owned(), "equipo", "invalid_bundle"),
        (good[..100].to_owned(), "equipo", "invalid_bundle"),
        (hex::encode(tampered), "equipo", "invalid_bundle"),
    ] {
        let join =
            l.t.json(
                "POST",
                &format!("/api/v1/networks/{}/join-requests", l.net),
                None,
                json!({"bundle":bundle,"name":name}),
            )
            .await;
        join.error(StatusCode::BAD_REQUEST, code);
        let token = l.t.token(&l.owner, l.net, false).await;
        let enroll =
            l.t.json(
                "POST",
                "/api/v1/enroll",
                None,
                json!({"token":token,"bundle":bundle,"name":name}),
            )
            .await;
        assert_eq!((join.status, &join.body), (enroll.status, &enroll.body));
    }
    assert_eq!(l.count(l.net, "request", "pending").await, 0);
    // name_auto se comporta como en enroll.
    let r =
        l.t.json(
            "POST",
            &format!("/api/v1/networks/{}/join-requests", l.net),
            None,
            json!({"bundle":good,"name":"equipo-sistema","name_auto":true}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let items = l.devices(l.net).await;
    assert_eq!(items[0]["name_source"], "device");
    assert_eq!(items[0]["hostname"], "equipo-sistema");
    l.t.finish().await;
}

#[tokio::test]
async fn codigo_de_red_solo_admin_sin_token_y_decodificable() {
    let Some(l) = lab(config(20)).await else {
        return;
    };
    let path = format!("/api/v1/networks/{}/join-code", l.net);
    let member = l.t.user().await;
    let stranger = l.t.user().await;
    let r =
        l.t.json(
            "PUT",
            &format!("/api/v1/orgs/{}/members/{}", l.org, member.user),
            Some(&l.owner),
            json!({"role":"member"}),
        )
        .await;
    assert!(r.status.is_success());
    l.t.json("GET", &path, Some(&member), Value::Null)
        .await
        .error(StatusCode::FORBIDDEN, "forbidden");
    l.t.json("GET", &path, Some(&stranger), Value::Null)
        .await
        .error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(
        l.t.json("GET", &path, None, Value::Null).await.status,
        StatusCode::UNAUTHORIZED
    );
    let r = l.t.json("GET", &path, Some(&l.owner), Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.headers["cache-control"], "no-store");
    let anchor = l.root.verifying_key().to_bytes();
    assert_eq!(
        r.json["root_anchor_fingerprint"],
        anchor_fingerprint(&anchor)
    );
    assert_eq!(r.json["join_policy"], "cerrada");
    assert_eq!(r.json["url"], URL);
    let code = JoinCode::decode(r.json["code"].as_str().unwrap()).unwrap();
    assert_eq!(code.url, URL);
    assert_eq!(code.network_id, *l.net.as_bytes());
    assert_eq!(code.root_anchor, anchor);
    assert_eq!(code.name, r.json["name"].as_str().unwrap());
    assert_eq!(code.token, None);

    // Un nombre que no cabe en el formato no se recorta: 409.
    sqlx::query("UPDATE networks SET name=$2 WHERE id=$1")
        .bind(l.net)
        .bind("n".repeat(49))
        .execute(&l.t.pool)
        .await
        .unwrap();
    l.t.json("GET", &path, Some(&l.owner), Value::Null)
        .await
        .error(StatusCode::CONFLICT, "invalid_network_name_for_code");
    l.t.finish().await;

    // public_url por defecto (http) tampoco es válida para el código.
    let Some(l) = lab(Config {
        enroll_per_minute: 10000,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    l.t.json(
        "GET",
        &format!("/api/v1/networks/{}/join-code", l.net),
        Some(&l.owner),
        Value::Null,
    )
    .await
    .error(StatusCode::CONFLICT, "invalid_public_url_for_code");
    l.t.finish().await;
}

#[tokio::test]
async fn token_de_enrolamiento_devuelve_codigo_con_token() {
    let Some(l) = lab(config(20)).await else {
        return;
    };
    let path = format!("/api/v1/networks/{}/enrollment-tokens", l.net);
    let r =
        l.t.json("POST", &path, Some(&l.owner), json!({"ttl_secs":600}))
            .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let token = r.json["token"].as_str().unwrap();
    let anchor = l.root.verifying_key().to_bytes();
    assert_eq!(
        r.json["join"],
        json!({"url":URL,"token":token,"network_id":l.net,"root_anchor":hex::encode(anchor)})
    );
    assert_eq!(
        r.json["root_anchor_fingerprint"],
        anchor_fingerprint(&anchor)
    );
    let code = JoinCode::decode(r.json["code"].as_str().unwrap()).unwrap();
    assert_eq!(code.token.as_deref(), Some(token));
    assert_eq!(code.url, URL);
    assert_eq!(code.network_id, *l.net.as_bytes());
    assert_eq!(code.root_anchor, anchor);
    // Un nombre fuera del formato no impide crear el token: el código es null.
    sqlx::query("UPDATE networks SET name=$2 WHERE id=$1")
        .bind(l.net)
        .bind("n".repeat(49))
        .execute(&l.t.pool)
        .await
        .unwrap();
    let r =
        l.t.json("POST", &path, Some(&l.owner), json!({"ttl_secs":600}))
            .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert!(r.json["code"].is_null() && r.json["token"].is_string());
    l.t.finish().await;
}

#[test]
fn openapi_describe_lo_nuevo() {
    let doc = serde_json::to_value(atenea_control_plane::openapi()).unwrap();
    let paths = &doc["paths"];
    assert!(
        paths["/api/v1/networks/{net}/join-policy"]["put"]["security"][0]
            .get("csrf")
            .is_some()
    );
    let code = &paths["/api/v1/networks/{net}/join-code"]["get"];
    assert!(code["security"][0].get("session").is_some());
    let join = &paths["/api/v1/networks/{net}/join-requests"]["post"];
    assert!(
        join.get("security")
            .is_none_or(|s| s.as_array().is_none_or(|a| a.is_empty()))
    );
    assert!(join["responses"].get("429").is_some() && join["responses"].get("404").is_some());
    let schemas = &doc["components"]["schemas"];
    for (schema, field) in [
        ("Network", "join_policy"),
        ("Device", "join_origin"),
        ("EnrollmentToken", "code"),
        ("EnrollmentToken", "root_anchor_fingerprint"),
        ("JoinCode", "root_anchor_fingerprint"),
        ("JoinRequest", "bundle"),
    ] {
        assert!(
            schemas[schema]["properties"].get(field).is_some(),
            "{schema}.{field}"
        );
    }
}
