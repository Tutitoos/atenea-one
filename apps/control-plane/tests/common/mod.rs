#![allow(dead_code)]

use atenea_control_plane::{AppState, config::Config, router};
use atenea_identity::{DeviceBundle, SigningKey, generate_seed, signing_key};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgConnectOptions};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};
use tower::ServiceExt;
use uuid::Uuid;

pub struct TestApp {
    pub app: Router,
    pub pool: PgPool,
    pub issuer: SigningKey,
    admin: PgPool,
    database: String,
}

impl TestApp {
    pub async fn new() -> Option<Self> {
        Self::configured(Config {
            enroll_per_minute: 10000,
            ..Config::default()
        })
        .await
    }

    pub async fn configured(config: Config) -> Option<Self> {
        let Ok(url) = std::env::var("ATENEA_TEST_DATABASE_URL") else {
            eprintln!(
                "OMITIDA integración PostgreSQL: falta ATENEA_TEST_DATABASE_URL; ejecute apps/control-plane/tests/run.sh para exigir una base real."
            );
            return None;
        };
        let options = PgConnectOptions::from_str(&url).expect("URL PostgreSQL de pruebas válida");
        let admin = PgPool::connect_with(options.clone())
            .await
            .expect("PostgreSQL de pruebas accesible");
        let database = format!("atenea_test_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE \"{database}\"").as_str())
            .await
            .expect("rol de pruebas con CREATEDB");
        let pool = PgPool::connect_with(options.database(&database))
            .await
            .unwrap();
        atenea_control_plane::db::migrate(&pool, None)
            .await
            .unwrap();
        let issuer = signing_key(generate_seed());
        let app = router(AppState::new(pool.clone(), config, issuer.clone()));
        Some(Self {
            app,
            pool,
            issuer,
            admin,
            database,
        })
    }

    pub async fn finish(self) {
        drop(self.app);
        self.pool.close().await;
        self.admin
            .execute(format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database).as_str())
            .await
            .unwrap();
        self.admin.close().await;
    }

    pub async fn user(&self) -> Session {
        let user = Uuid::new_v4();
        let token = Uuid::new_v4().to_string();
        let csrf = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users (id,oidc_issuer,oidc_subject,email,display_name) VALUES ($1,'https://oidc.example.test',$2,'test@example.test','Prueba')")
            .bind(user).bind(user.to_string()).execute(&self.pool).await.unwrap();
        sqlx::query("INSERT INTO sessions (id,user_id,csrf_hash,csrf_token,created_at,expires_at,last_seen_at) VALUES ($1,$2,$3,$4,now(),now()+interval '8 hours',now())")
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(user)
            .bind(Sha256::digest(csrf.as_bytes()).to_vec()).bind(&csrf).execute(&self.pool).await.unwrap();
        Session { user, token, csrf }
    }

    pub async fn org(&self, session: &Session) -> Uuid {
        let r = self
            .json(
                "POST",
                "/api/v1/orgs",
                Some(session),
                json!({"name":Uuid::new_v4().to_string()}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        id(&r.json)
    }

    pub async fn network(
        &self,
        session: &Session,
        org: Uuid,
        root: &SigningKey,
        v4: &str,
        v6: &str,
    ) -> Uuid {
        let r = self.json("POST", &format!("/api/v1/orgs/{org}/networks"), Some(session), json!({"name":Uuid::new_v4().to_string(),"ipv4_cidr":v4,"ipv6_cidr":v6,"root_anchor":hex::encode(root.verifying_key().to_bytes())})).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        id(&r.json)
    }

    pub async fn token(&self, session: &Session, net: Uuid, auto: bool) -> String {
        let r = self
            .json(
                "POST",
                &format!("/api/v1/networks/{net}/enrollment-tokens"),
                Some(session),
                json!({"ttl_secs":600,"auto_approve":auto}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        assert_eq!(r.json["join"]["network_id"], net.to_string());
        assert!(r.json["join"]["root_anchor"].as_str().is_some());
        assert!(r.json["expires_at"].is_string() || r.json["expires_at"].is_number());
        r.json["token"].as_str().unwrap().to_owned()
    }

    pub async fn enroll(&self, token: &str, device: &Device) -> Reply {
        self.json("POST", "/api/v1/enroll", None, json!({"token":token,"bundle":hex::encode(device.bundle.encode()),"name":"dispositivo sintético"})).await
    }

    pub async fn action(&self, session: &Session, net: Uuid, dev: Uuid, action: &str) -> Reply {
        self.json(
            "POST",
            &format!("/api/v1/networks/{net}/devices/{dev}/{action}"),
            Some(session),
            json!({}),
        )
        .await
    }

    pub async fn json(
        &self,
        method: &str,
        path: &str,
        session: Option<&Session>,
        body: Value,
    ) -> Reply {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(s) = session {
            builder = builder
                .header("cookie", format!("atenea_session={}", s.token))
                .header("x-csrf-token", &s.csrf);
        }
        let mut request = builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                12345,
            )));
        self.send(request).await
    }

    pub async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap()
            .to_vec();
        let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
        Reply {
            status,
            headers,
            body,
            json,
        }
    }

    pub async fn config(&self, device: &Device, ts: u64, nonce: [u8; 16]) -> Reply {
        let header = atenea_identity::api_auth::sign_request(
            &device.key,
            *device.id.as_bytes(),
            "GET",
            "/api/v1/device/config",
            b"",
            ts,
            nonce,
        );
        self.send(
            Request::builder()
                .uri("/api/v1/device/config")
                .header("authorization", header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn certificate(&self, session: &Session, net: Uuid, root: &SigningKey) {
        let cert = atenea_identity::IssuerCert::issue(
            root,
            *net.as_bytes(),
            1,
            now() - 60,
            now() + 3600,
            self.issuer.verifying_key().to_bytes(),
            None,
        )
        .unwrap();
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/networks/{net}/issuer-cert"))
            .header("cookie", format!("atenea_session={}", session.token))
            .header("x-csrf-token", &session.csrf)
            .header("content-type", "application/octet-stream")
            .body(Body::from(cert.encode().to_vec()))
            .unwrap();
        let r = self.send(request).await;
        assert!(r.status.is_success(), "{} {}", r.status, r.json);
    }
}

pub struct Session {
    pub user: Uuid,
    pub token: String,
    pub csrf: String,
}
pub struct Reply {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Vec<u8>,
    pub json: Value,
}
impl Reply {
    pub fn error(&self, status: StatusCode, code: &str) {
        assert_eq!(self.status, status, "{}", self.json);
        assert_eq!(self.json["error"]["code"], code);
    }
}
pub struct Device {
    pub id: Uuid,
    pub key: SigningKey,
    pub bundle: DeviceBundle,
}
impl Device {
    pub fn new() -> Self {
        let id = Uuid::new_v4();
        let key = signing_key(generate_seed());
        let bundle = DeviceBundle::create(
            *id.as_bytes(),
            signing_key(generate_seed()).verifying_key().to_bytes(),
            &key,
        );
        Self { id, key, bundle }
    }
}
pub fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// Se abre una conexión real: no se sustituyen permisos ni SQL por dobles de prueba.
pub async fn connection(pool: &PgPool) -> PgConnection {
    PgConnection::connect_with(pool.connect_options().as_ref())
        .await
        .unwrap()
}
