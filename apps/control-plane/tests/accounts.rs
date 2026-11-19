// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
//! ADR-028 fase 2: cuentas propias (registro, inicio de sesión, recuperación) y organización personal.
mod common;
use atenea_control_plane::{accounts, config::Config};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Reply, TestApp};
use serde_json::{Value, json};
use uuid::Uuid;

async fn app(registration_open: bool) -> Option<TestApp> {
    TestApp::configured(Config {
        registration_open,
        ..Config::default()
    })
    .await
}

fn cookie(reply: &Reply) -> String {
    let value = reply
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("atenea_session="))
        .expect("Set-Cookie de sesión");
    value.split(';').next().unwrap().to_owned()
}

async fn get(app: &TestApp, path: &str, cookie: &str) -> Reply {
    let request = Request::builder()
        .uri(path)
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();
    app.send(request).await
}

async fn register(app: &TestApp, login: &str) -> Reply {
    app.json(
        "POST",
        "/auth/local/register",
        None,
        json!({"login": login, "password": "una contraseña larga", "display_name": "Ana"}),
    )
    .await
}

#[tokio::test]
async fn registro_crea_cuenta_sesion_codigos_y_organizacion_personal() {
    let Some(app) = app(true).await else { return };
    let r = register(&app, "Ana@Example.test").await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["recovery_codes"].as_array().unwrap().len(), 10);
    let user: Uuid = r.json["user_id"].as_str().unwrap().parse().unwrap();

    // La sesión de la respuesta ya sirve y la cuenta es dueña de su organización personal.
    let cookie = cookie(&r);
    let orgs = get(&app, "/api/v1/orgs", &cookie).await;
    assert_eq!(orgs.status, StatusCode::OK, "{}", orgs.json);
    assert_eq!(orgs.json.as_array().unwrap().len(), 1);
    assert_eq!(orgs.json[0]["role"], "owner");
    let personal: bool = sqlx::query_scalar("SELECT personal FROM organizations WHERE id = $1")
        .bind(Uuid::parse_str(orgs.json[0]["id"].as_str().unwrap()).unwrap())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(personal);

    // El correo no está verificado: no se copia a users.email (no sirve para «añadir por correo»).
    let (email, issuer, subject): (Option<String>, String, String) =
        sqlx::query_as("SELECT email, oidc_issuer, oidc_subject FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(email, None);
    assert_eq!(
        (issuer.as_str(), subject.as_str()),
        ("local", "ana@example.test")
    );

    // Ni la contraseña ni los códigos se guardan en claro.
    let stored: String =
        sqlx::query_scalar("SELECT password_hash FROM local_credentials WHERE user_id = $1")
            .bind(user)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(stored.starts_with("$argon2id$"));
    let code = r.json["recovery_codes"][0].as_str().unwrap();
    let plain: i64 = sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE code_hash = $1")
        .bind(code.as_bytes())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(plain, 0);
    app.finish().await;
}

#[tokio::test]
async fn registro_cerrado_por_defecto_y_validaciones() {
    let Some(closed) = app(false).await else {
        return;
    };
    register(&closed, "ana@example.test")
        .await
        .error(StatusCode::FORBIDDEN, "registration_closed");
    closed.finish().await;
    // La pantalla de acceso lo lee sin sesión para ocultar «Crear una cuenta».
    let closed = app(false).await.unwrap();
    let options = closed
        .json("GET", "/auth/local/options", None, serde_json::Value::Null)
        .await;
    assert_eq!(options.status, StatusCode::OK);
    assert_eq!(options.json["registration_open"], false);
    closed.finish().await;
    let open = app(true).await.unwrap();
    let options = open
        .json("GET", "/auth/local/options", None, serde_json::Value::Null)
        .await;
    assert_eq!(options.json["registration_open"], true);
    open.finish().await;

    let Some(app) = TestApp::configured(Config {
        registration_open: true,
        register_per_minute: 100,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    for login in ["sin-arroba", "a@b", "a b@example.test", "@example.test"] {
        register(&app, login)
            .await
            .error(StatusCode::BAD_REQUEST, "invalid_login");
    }
    app.json(
        "POST",
        "/auth/local/register",
        None,
        json!({"login": "ana@example.test", "password": "corta"}),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_password");
    assert_eq!(
        register(&app, "ana@example.test").await.status,
        StatusCode::CREATED
    );
    // El correo es único sin distinguir mayúsculas.
    register(&app, "ANA@example.test")
        .await
        .error(StatusCode::CONFLICT, "conflict");
    app.finish().await;
}

#[tokio::test]
async fn inicio_de_sesion_no_distingue_cuenta_inexistente_y_bloquea_tras_cinco_fallos() {
    let Some(app) = app(true).await else { return };
    assert_eq!(
        register(&app, "ana@example.test").await.status,
        StatusCode::CREATED
    );
    let login = |login: &'static str, password: &'static str| {
        app.json(
            "POST",
            "/auth/local/login",
            None,
            json!({"login": login, "password": password}),
        )
    };
    login("nadie@example.test", "una contraseña larga")
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    for _ in 0..5 {
        login("ana@example.test", "otra contraseña mala")
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    // Bloqueada: ni con la contraseña correcta.
    login("ana@example.test", "una contraseña larga")
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    sqlx::query("UPDATE local_credentials SET locked_until = now() - interval '1 second'")
        .execute(&app.pool)
        .await
        .unwrap();
    let ok = login("Ana@Example.test", "una contraseña larga").await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    let session = get(&app, "/api/v1/session", &cookie(&ok)).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.json);
    // Entrar repone el contador.
    let failed: i32 = sqlx::query_scalar("SELECT failed_attempts FROM local_credentials")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(failed, 0);
    app.finish().await;
}

#[tokio::test]
async fn recuperacion_consume_el_codigo_cambia_la_contrasena_y_cierra_sesiones() {
    let Some(app) = app(true).await else { return };
    let r = register(&app, "ana@example.test").await;
    let old_session = cookie(&r);
    let code = r.json["recovery_codes"][3].as_str().unwrap().to_owned();
    let recover = |code: &str, password: &str| {
        app.json(
            "POST",
            "/auth/local/recover",
            None,
            json!({"login": "ana@example.test", "recovery_code": code, "new_password": password}),
        )
    };
    recover("no es un código", "contraseña nueva larga")
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_eq!(
        recover(&code, "contraseña nueva larga").await.status,
        StatusCode::NO_CONTENT
    );
    // Un solo uso, y la sesión anterior deja de valer.
    recover(&code, "otra contraseña larga")
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_eq!(
        get(&app, "/api/v1/session", &old_session).await.status,
        StatusCode::UNAUTHORIZED
    );
    app.json(
        "POST",
        "/auth/local/login",
        None,
        json!({"login": "ana@example.test", "password": "una contraseña larga"}),
    )
    .await
    .error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    let ok = app
        .json(
            "POST",
            "/auth/local/login",
            None,
            json!({"login": "ana@example.test", "password": "contraseña nueva larga"}),
        )
        .await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    app.finish().await;
}

#[tokio::test]
async fn el_registro_tiene_cupo_por_ip() {
    let Some(app) = TestApp::configured(Config {
        registration_open: true,
        register_per_minute: 2,
        ..Config::default()
    })
    .await
    else {
        return;
    };
    for i in 0..2 {
        assert_eq!(
            register(&app, &format!("u{i}@example.test")).await.status,
            StatusCode::CREATED
        );
    }
    register(&app, "u2@example.test")
        .await
        .error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    app.finish().await;
}

#[tokio::test]
async fn la_organizacion_personal_no_admite_miembros_y_se_crea_una_vez() {
    let Some(app) = app(true).await else { return };
    let r = register(&app, "ana@example.test").await;
    let user: Uuid = r.json["user_id"].as_str().unwrap().parse().unwrap();
    let cookie = cookie(&r);
    let org = Uuid::parse_str(
        get(&app, "/api/v1/orgs", &cookie).await.json[0]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    // Otra cuenta con correo verificado de un proveedor: aun así no se puede añadir a la personal.
    let other = app.user().await;
    let csrf = get(&app, "/api/v1/session", &cookie).await.json["csrf_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let request = Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/orgs/{org}/members/{}", other.user))
        .header("cookie", &cookie)
        .header("x-csrf-token", csrf)
        .header("content-type", "application/json")
        .body(Body::from(json!({"role": "member"}).to_string()))
        .unwrap();
    app.send(request)
        .await
        .error(StatusCode::CONFLICT, "personal_org");

    // Repetir la creación no duplica la organización ni la recrea tras borrarla.
    let mut conn = common::connection(&app.pool).await;
    accounts::ensure_personal(&mut conn, user, Some("Ana"))
        .await
        .unwrap();
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(org)
        .execute(&app.pool)
        .await
        .unwrap();
    accounts::ensure_personal(&mut conn, user, Some("Ana"))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM memberships WHERE user_id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    app.finish().await;
}

#[tokio::test]
async fn un_usuario_oidc_existente_recibe_su_organizacion_personal_al_entrar() {
    let Some(app) = app(false).await else { return };
    let existing = app.user().await;
    let mut conn = common::connection(&app.pool).await;
    accounts::ensure_personal(&mut conn, existing.user, Some("Guille"))
        .await
        .unwrap();
    let name: String = sqlx::query_scalar(
        "SELECT o.name FROM organizations o JOIN memberships m ON m.org_id = o.id \
         WHERE m.user_id = $1 AND o.personal",
    )
    .bind(existing.user)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(name, "Organización de Guille");
    app.finish().await;
}

#[tokio::test]
async fn el_emisor_se_expone_solo_con_sesion() {
    let Some(app) = app(true).await else { return };
    let anonymous = app
        .send(
            Request::builder()
                .uri("/api/v1/issuer")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    anonymous.error(StatusCode::UNAUTHORIZED, "invalid_session");
    let session = app.user().await;
    let r = get(
        &app,
        "/api/v1/issuer",
        &format!("atenea_session={}", session.token),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(
        r.json["issuer_pubkey"],
        Value::String(hex::encode(app.issuer.verifying_key().to_bytes()))
    );
    app.finish().await;
}

#[tokio::test]
async fn el_minimo_de_contrasena_es_configurable_pero_nunca_baja_de_ocho() {
    for (configured, accepted, rejected) in
        [(8usize, "Admin123", "Admin12"), (3, "Admin123", "Admin12")]
    {
        let Some(app) = TestApp::configured(Config {
            registration_open: true,
            password_min_chars: configured,
            ..Config::default()
        })
        .await
        else {
            return;
        };
        let body = |password: &str, login: &str| json!({"login": login, "password": password});
        app.json(
            "POST",
            "/auth/local/register",
            None,
            body(rejected, "a@example.test"),
        )
        .await
        .error(StatusCode::BAD_REQUEST, "invalid_password");
        let ok = app
            .json(
                "POST",
                "/auth/local/register",
                None,
                body(accepted, "b@example.test"),
            )
            .await;
        assert_eq!(ok.status, StatusCode::CREATED, "{}", ok.json);
        app.finish().await;
    }
    // Por defecto sigue pidiendo 12.
    let Some(app) = app(true).await else { return };
    app.json(
        "POST",
        "/auth/local/register",
        None,
        json!({"login": "c@example.test", "password": "Admin123456"}),
    )
    .await
    .error(StatusCode::BAD_REQUEST, "invalid_password");
    app.finish().await;
}
