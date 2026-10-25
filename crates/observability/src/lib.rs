//! Exportador privado; las rutas proceden exclusivamente de MatchedPath, nunca de URI.
use axum::{
    Router,
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use std::{
    collections::BTreeMap,
    fmt::Write,
    sync::{Arc, Mutex},
    time::Instant,
};
const BUCKETS: [f64; 6] = [0.005, 0.025, 0.1, 0.5, 2.0, 30.0];
type HttpSamples = BTreeMap<(String, &'static str, u16), Sample>;
#[derive(Clone, Copy)]
pub enum Event {
    DatabaseError,
    LeaseIssued,
    LeaseRenewed,
    LeaseRejected,
    LeaseExpired,
    Approved,
    Revoked,
    RelayTokensIssued,
}
static EVENTS: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];
pub fn event(event: Event, count: u64) {
    EVENTS[event as usize].fetch_add(count, std::sync::atomic::Ordering::Relaxed);
}
#[derive(Clone)]
pub struct ErrorCode(pub &'static str);
const REASONS: [&str; 8] = [
    "invalid_signature",
    "invalid_config",
    "peer_unavailable",
    "not_announced",
    "rate_limited",
    "relay_unavailable",
    "invalid_request",
    "other",
];
static REJECTIONS: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];
#[derive(Default)]
struct Sample {
    count: u64,
    sum: f64,
    buckets: [u64; 6],
}
#[derive(Clone, Default)]
pub struct Metrics(Arc<Mutex<HttpSamples>>);
impl Metrics {
    fn observe(&self, route: String, method: &'static str, code: u16, seconds: f64) {
        let mut data = self.0.lock().expect("métricas");
        let sample = data.entry((route, method, code)).or_default();
        sample.count += 1;
        sample.sum += seconds;
        for (i, bound) in BUCKETS.iter().enumerate() {
            if seconds <= *bound {
                sample.buckets[i] += 1;
            }
        }
    }
    pub fn encode(&self) -> String {
        let mut out = String::from(
            "# HELP atenea_http_requests_total Peticiones HTTP por plantilla de ruta.\n# TYPE atenea_http_requests_total counter\n# HELP atenea_http_duration_seconds Duración HTTP.\n# TYPE atenea_http_duration_seconds histogram\n",
        );
        for ((route, method, code), sample) in self.0.lock().expect("métricas").iter() {
            let labels = format!("route=\"{route}\",method=\"{method}\",code=\"{code}\"");
            let _ = writeln!(
                out,
                "atenea_http_requests_total{{{labels}}} {}",
                sample.count
            );
            for (i, bound) in BUCKETS.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "atenea_http_duration_seconds_bucket{{{labels},le=\"{bound}\"}} {}",
                    sample.buckets[i]
                );
            }
            let _ = writeln!(
                out,
                "atenea_http_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {}\natenea_http_duration_seconds_count{{{labels}}} {}\natenea_http_duration_seconds_sum{{{labels}}} {}",
                sample.count, sample.count, sample.sum
            );
        }
        for (index, name) in [
            "database_errors",
            "leases_issued",
            "leases_renewed",
            "leases_rejected",
            "leases_expired",
            "approvals",
            "revocations",
            "relay_tokens_issued",
        ]
        .iter()
        .enumerate()
        {
            let _ = writeln!(
                out,
                "# TYPE atenea_{name}_total counter\natenea_{name}_total {}",
                EVENTS[index].load(std::sync::atomic::Ordering::Relaxed)
            );
        }
        for (index, reason) in REASONS.iter().enumerate() {
            let _ = writeln!(
                out,
                "atenea_relay_token_rejections_total{{reason=\"{reason}\"}} {}",
                REJECTIONS[index].load(std::sync::atomic::Ordering::Relaxed)
            );
        }
        out
    }
}
pub async fn record(State(metrics): State<Metrics>, request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();
    let method = match request.method().as_str() {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "PATCH" => "PATCH",
        "HEAD" => "HEAD",
        "OPTIONS" => "OPTIONS",
        _ => "OTHER",
    };
    let start = Instant::now();
    let response = next.run(request).await;
    let ok = response.status().is_success();
    if route == "/api/v1/device/config" && !ok {
        event(Event::LeaseRejected, 1);
    }
    if route.ends_with("/approve") && ok {
        event(Event::Approved, 1);
    }
    if route.ends_with("/revoke") && ok {
        event(Event::Revoked, 1);
    }
    if route == "/v1/relay" && !ok {
        let code = response
            .extensions()
            .get::<ErrorCode>()
            .map_or("other", |v| v.0);
        let index = REASONS
            .iter()
            .position(|reason| *reason == code)
            .unwrap_or(7);
        REJECTIONS[index].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    metrics.observe(
        route,
        method,
        response.status().as_u16(),
        start.elapsed().as_secs_f64(),
    );
    response
}
pub fn router(render: Arc<dyn Fn() -> String + Send + Sync>) -> Router {
    Router::new().route(
        "/metrics",
        get(move || {
            let render = render.clone();
            async move {
                (
                    [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                    render(),
                )
                    .into_response()
            }
        }),
    )
}
pub async fn serve(
    address: &str,
    render: Arc<dyn Fn() -> String + Send + Sync>,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    Ok(tokio::spawn(async move {
        let _ = axum::serve(listener, router(render)).await;
    }))
}
pub async fn serve_listener(
    listener: tokio::net::TcpListener,
    render: Arc<dyn Fn() -> String + Send + Sync>,
) -> std::io::Result<()> {
    axum::serve(listener, router(render)).await
}
#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn many_devices_networks_and_private_inputs_do_not_create_series() {
        let metrics = Metrics::default();
        let app = Router::new()
            .route(
                "/networks/{network}/devices/{device}",
                get(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                metrics.clone(),
                record,
            ));
        let mut series = 0;
        for n in 0..10000 {
            let request = Request::builder()
                .uri(format!(
                    "/networks/{n}/devices/{n}?token=SECRET_TOKEN&email=private@example.test"
                ))
                .header("authorization", "Bearer SECRET_TOKEN")
                .header("cookie", "SECRET_COOKIE")
                .body(axum::body::Body::empty())
                .unwrap();
            assert_eq!(app.clone().oneshot(request).await.unwrap().status(), 200);
            if n == 0 {
                series = metrics.encode().lines().count();
            }
        }
        let text = metrics.encode();
        assert_eq!(series, text.lines().count());
        for private in [
            "SECRET_TOKEN",
            "SECRET_COOKIE",
            "private@example.test",
            "/networks/9999/devices/9999",
        ] {
            assert!(!text.contains(private));
        }
        assert!(text.contains("/networks/{network}/devices/{device}"));
    }
}
