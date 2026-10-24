mod common;
use atenea_control_plane::config::Config;
use axum::{
    Form, Json, Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::*;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use url::Url;

const N: &str = "twtgjAiNW2HW0YMSgUC8svgiqkOaeuhp4i1NAa8akF4kovTsmETGDPcnV8C_1hWxEIQFjA8XX7i-0I-2r-mLYV_g7lQ5oG_ZgpWPE1I9v4St2NL9O-SSufnnUTJFAVUroJiixTL5tNCIcPXJm0CUMPnxvTTL54Lj9YgtsXeHuEji5O5m5_VbiK-c_-nbeW7ZrS5gpBq2qspCHziILuEQKhsyk1w2tWvlgh8eTniynZmtL4HrtU7WSwStc5RIZWABgXOueuBuwySeD_V_T33iaFWQpeCcYE4UXlpOaJh6HcCqe6pe5qYDSvU8QXBnXYDjVcjwzNm7uiLFPyUjdwOFYw";
// Clave sintética pública del proveedor de pruebas; nunca sirve como credencial operativa.
const KEY: &[u8] = include_bytes!("fixtures/oidc-test-key.pem");
#[derive(Default)]
struct Flow {
    nonce: String,
    challenge: String,
    mode: &'static str,
    exchanges: usize,
}
#[derive(Clone)]
struct Provider {
    issuer: String,
    flow: Arc<Mutex<Flow>>,
}
async fn discovery(State(p): State<Provider>) -> Json<Value> {
    Json(
        json!({"issuer":p.issuer,"authorization_endpoint":format!("{}/authorize",p.issuer),"token_endpoint":format!("{}/token",p.issuer),"jwks_uri":format!("{}/jwks",p.issuer),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post"],"code_challenge_methods_supported":["S256"]}),
    )
}
async fn jwks() -> Json<Value> {
    Json(
        json!({"keys":[{"kty":"RSA","kid":"test-key","use":"sig","alg":"RS256","n":N,"e":"AQAB"}]}),
    )
}
async fn token(
    State(p): State<Provider>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let mut flow = p.flow.lock().unwrap();
    flow.exchanges += 1;
    assert_eq!(form.get("grant_type").unwrap(), "authorization_code");
    assert_eq!(form.get("code").unwrap(), "test-code");
    let verifier = form.get("code_verifier").expect("PKCE obligatorio");
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        flow.challenge
    );
    let mut claims = json!({"iss":p.issuer,"sub":"subject-test","aud":"test-client","iat":now(),"exp":now()+300,"nonce":flow.nonce,"email":"oidc@example.test","name":"OIDC sintético"});
    match flow.mode {
        "issuer" => claims["iss"] = json!("https://otro.example.test"),
        "audience" => claims["aud"] = json!("otro-cliente"),
        "nonce" => claims["nonce"] = json!("otro-nonce"),
        "expired" => claims["exp"] = json!(now() - 3600),
        _ => {}
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    let mut jwt =
        jsonwebtoken::encode(&header, &claims, &EncodingKey::from_rsa_pem(KEY).unwrap()).unwrap();
    if flow.mode == "signature" {
        let pos = jwt.rfind('.').unwrap() + 1;
        let mut bytes = jwt.into_bytes();
        bytes[pos] = if bytes[pos] == b'A' { b'B' } else { b'A' };
        jwt = String::from_utf8(bytes).unwrap();
    }
    Json(
        json!({"access_token":"synthetic-access-token","token_type":"Bearer","expires_in":300,"id_token":jwt}),
    )
}
struct Server {
    provider: Provider,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = Provider {
            issuer: format!("http://{}", listener.local_addr().unwrap()),
            flow: Arc::new(Mutex::new(Flow::default())),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .with_state(provider.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { provider, task }
    }
    fn config(&self) -> Config {
        Config {
            oidc_issuer: self.provider.issuer.clone(),
            oidc_client_id: "test-client".into(),
            oidc_client_secret: "test-secret".into(),
            oidc_redirect_url: "http://control.example.test/auth/callback".into(),
            dev_insecure_cookies: false,
            ..Config::default()
        }
    }
    async fn login(&self, t: &TestApp, mode: &'static str) -> (String, String) {
        let r = t
            .send(
                Request::builder()
                    .uri("/auth/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert!(r.status.is_redirection(), "{} {}", r.status, r.json);
        let url = Url::parse(r.headers["location"].to_str().unwrap()).unwrap();
        assert_eq!(url.origin().ascii_serialization(), self.provider.issuer);
        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["client_id"], "test-client");
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(params["scope"].split_whitespace().any(|s| s == "openid"));
        assert_ne!(params["state"], params["nonce"]);
        let cookie = r
            .headers
            .get_all("set-cookie")
            .iter()
            .map(|s| s.to_str().unwrap())
            .find(|s| s.starts_with("atenea_oidc="))
            .unwrap();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Lax"));
        let cookie = cookie.split(';').next().unwrap().to_owned();
        let mut flow = self.provider.flow.lock().unwrap();
        flow.nonce = params["nonce"].clone();
        flow.challenge = params["code_challenge"].clone();
        flow.mode = mode;
        (params["state"].clone(), cookie)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn callback(t: &TestApp, state: &str, cookie: Option<&str>) -> Reply {
    let mut req = Request::builder().uri(format!("/auth/callback?code=test-code&state={state}"));
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    t.send(req.body(Body::empty()).unwrap()).await
}

#[tokio::test]
async fn oidc_discovery_jwks_pkce_sesion_y_reutilizacion() {
    let provider = Server::start().await;
    let Some(t) = TestApp::configured(provider.config()).await else {
        return;
    };
    let (state, flow_cookie) = provider.login(&t, "").await;
    let r = callback(&t, &state, Some(&flow_cookie)).await;
    assert!(r.status.is_redirection(), "{} {}", r.status, r.json);
    let cookie = r
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|s| s.to_str().unwrap())
        .find(|s| s.starts_with("atenea_session="))
        .unwrap();
    for flag in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(flag), "{cookie}");
    }
    let session_cookie = cookie.split(';').next().unwrap().to_owned();
    let session = t
        .send(
            Request::builder()
                .uri("/api/v1/session")
                .header("cookie", &session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.json["user"]["email"], "oidc@example.test");
    assert!(session.json["csrf_token"].as_str().is_some());
    callback(&t, &state, Some(&flow_cookie))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
    assert_eq!(provider.provider.flow.lock().unwrap().exchanges, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    // El mismo sujeto se actualiza, no crea un segundo usuario.
    let (state, cookie) = provider.login(&t, "").await;
    assert!(
        callback(&t, &state, Some(&cookie))
            .await
            .status
            .is_redirection()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    t.finish().await;
}

#[tokio::test]
async fn oidc_rechaza_issuer_audience_nonce_caducidad_firma_state_y_cookie() {
    let provider = Server::start().await;
    let Some(t) = TestApp::configured(provider.config()).await else {
        return;
    };
    for mode in ["issuer", "audience", "nonce", "expired", "signature"] {
        let (state, cookie) = provider.login(&t, mode).await;
        callback(&t, &state, Some(&cookie))
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
        callback(&t, &state, Some(&cookie))
            .await
            .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
    }
    let (state, cookie) = provider.login(&t, "").await;
    callback(&t, "estado-falso", Some(&cookie))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
    callback(&t, &state, None)
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
    callback(&t, &state, Some("atenea_oidc=otro-navegador"))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
    let (state, cookie) = provider.login(&t, "").await;
    sqlx::query("UPDATE oidc_flows SET expires_at=now()-interval '1 second'")
        .execute(&t.pool)
        .await
        .unwrap();
    callback(&t, &state, Some(&cookie))
        .await
        .error(StatusCode::UNAUTHORIZED, "invalid_oidc");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    t.finish().await;
}
