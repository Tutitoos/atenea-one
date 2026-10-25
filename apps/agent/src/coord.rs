//! Cliente HTTP del coordinator (docs/protocol/coordinator-v1.md §3): anuncio de presencia,
//! rendezvous y avisos. Todo lo que devuelve son PISTAS no autenticadas; la ruta solo existe
//! cuando un paquete Noise autenticado llega desde un candidato.

use crate::config::CoordinatorConfig;
use crate::relay::RelayEndpoint;
use atenea_identity::{DeviceId, Prefix, SigningKey, api_auth::sign_request};
use atenea_nat_traversal::candidate_ok;
use atenea_policies::prefix_contains;
use base64::Engine as _;
use serde::Deserialize;
use std::io::Read;
use std::net::SocketAddr;
use std::time::Duration;

/// Respuestas del coordinator: pequeñas; más de esto se rechaza.
const RESPONSE_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Un par quiere conectar: candidatos (pistas) para el sondeo.
    Connect {
        peer: DeviceId,
        candidates: Vec<SocketAddr>,
    },
    /// Un par pide relay: token para abrir la asignación inversa.
    Relay { peer: DeviceId, grant: RelayGrant },
}

#[derive(Deserialize)]
struct RawRendezvous {
    #[serde(default)]
    candidates: Vec<String>,
}

#[derive(Deserialize)]
struct RawEvents {
    #[serde(default)]
    events: Vec<RawEvent>,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(default)]
    kind: String,
    peer: String,
    #[serde(default)]
    candidates: Vec<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    relays: Vec<RelayEndpoint>,
}

#[derive(Deserialize)]
struct RawRelay {
    token: String,
    #[serde(default)]
    relays: Vec<RelayEndpoint>,
}

/// Token de asignación del coordinator y relays a los que sirve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayGrant {
    pub token: Vec<u8>,
    pub relays: Vec<RelayEndpoint>,
}

fn grant(token_b64: &str, relays: Vec<RelayEndpoint>) -> Option<RelayGrant> {
    let token = base64::engine::general_purpose::STANDARD
        .decode(token_b64)
        .ok()?;
    (token.len() == atenea_relay_core::TOKEN_LEN).then_some(RelayGrant { token, relays })
}

fn parse_candidates(list: &[String]) -> Vec<SocketAddr> {
    list.iter().filter_map(|s| s.parse().ok()).collect()
}

/// Relays de una concesión que se aceptan como mucho (el coordinator no es de confianza).
const MAX_RELAYS: usize = 4;
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(3);

/// Resuelve `host:puerto` con plazo y devuelve una IP literal aceptable: unicast enrutable
/// (sin loopback, enlace local, multicast ni no especificada) y fuera de la red del túnel y de
/// las subredes servidas, que solo son alcanzables a través del propio túnel.
async fn vet_addr(addr: &str, tunnel_only: &[Prefix]) -> Option<String> {
    if addr.len() > 255 {
        return None;
    }
    let resolved = tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host(addr))
        .await
        .ok()?
        .ok()?;
    resolved
        .take(8)
        .find(|a| candidate_ok(a) && !tunnel_only.iter().any(|p| prefix_contains(p, &a.ip())))
        .map(|a| a.to_string())
}

/// Acota y sanea los destinos de una concesión de relay. Los que se devuelven llevan la IP ya
/// verificada: el cliente de relay conecta a ellas y no vuelve a resolver el nombre.
pub async fn vet_relays(relays: Vec<RelayEndpoint>, tunnel_only: &[Prefix]) -> Vec<RelayEndpoint> {
    let mut out = Vec::new();
    for mut relay in relays.into_iter().take(MAX_RELAYS) {
        if let Some(addr) = relay.udp.take() {
            relay.udp = vet_addr(&addr, tunnel_only).await;
        }
        if let Some(addr) = relay.tls.take() {
            relay.tls = vet_addr(&addr, tunnel_only).await;
        }
        if relay.udp.is_some() || relay.tls.is_some() {
            out.push(relay);
        }
    }
    out
}

/// URL completa y `path_and_query` exacto (es lo que firma la petición).
fn target(cfg: &CoordinatorConfig, path: &str) -> Result<(String, String), &'static str> {
    let base = cfg.url.trim_end_matches('/');
    let url = format!("{base}{path}");
    let uri: ureq::http::Uri = url.parse().map_err(|_| "URL inválida")?;
    let pq = uri
        .path_and_query()
        .ok_or("URL inválida")?
        .as_str()
        .to_string();
    Ok((url, pq))
}

fn nonce() -> [u8; 16] {
    let mut n = [0u8; 16];
    n.copy_from_slice(&atenea_identity::generate_seed()[..16]);
    n
}

fn agent(total: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(total))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

fn read_limited(response: &mut ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "respuesta ilegible")?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err("respuesta demasiado grande");
    }
    Ok(bytes)
}

fn status_error(code: u16) -> &'static str {
    match code {
        401 | 403 => "acceso denegado",
        404 | 409 => "par no disponible",
        429 => "límite de tasa",
        _ => "error HTTP del coordinator",
    }
}

/// Presencia: la configuración firmada vigente (el coordinator la verifica) y los candidatos propios.
pub fn announce(
    cfg: &CoordinatorConfig,
    key: &SigningKey,
    device: DeviceId,
    signed_config: &[u8],
    candidates: &[SocketAddr],
    ts: u64,
) -> Result<(), &'static str> {
    let (url, pq) = target(cfg, "/v1/announce")?;
    let body = serde_json::json!({
        "config": base64::engine::general_purpose::STANDARD.encode(signed_config),
        "candidates": candidates.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
    })
    .to_string()
    .into_bytes();
    let auth = sign_request(key, device, "POST", &pq, &body, ts, nonce());
    let response = agent(Duration::from_secs(15))
        .post(&url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .send(&body[..])
        .map_err(|_| "error de red")?;
    match response.status().as_u16() {
        200 => Ok(()),
        c => Err(status_error(c)),
    }
}

/// Pide los candidatos de un par (autorización mutua en el coordinator).
pub fn rendezvous(
    cfg: &CoordinatorConfig,
    key: &SigningKey,
    device: DeviceId,
    peer: DeviceId,
    ts: u64,
) -> Result<Vec<SocketAddr>, &'static str> {
    let (url, pq) = target(cfg, "/v1/rendezvous")?;
    let body = serde_json::json!({ "peer": atenea_crypto::hex::encode(&peer) })
        .to_string()
        .into_bytes();
    let auth = sign_request(key, device, "POST", &pq, &body, ts, nonce());
    let mut response = agent(Duration::from_secs(15))
        .post(&url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .send(&body[..])
        .map_err(|_| "error de red")?;
    match response.status().as_u16() {
        200 => {
            let bytes = read_limited(&mut response)?;
            let raw: RawRendezvous =
                serde_json::from_slice(&bytes).map_err(|_| "respuesta inválida")?;
            Ok(parse_candidates(&raw.candidates))
        }
        c => Err(status_error(c)),
    }
}

/// Pide un token de relay para `peer` (autorización mutua en el coordinator). El coordinator avisa
/// además al par con el token inverso (evento `relay`).
pub fn relay(
    cfg: &CoordinatorConfig,
    key: &SigningKey,
    device: DeviceId,
    peer: DeviceId,
    ts: u64,
) -> Result<RelayGrant, &'static str> {
    let (url, pq) = target(cfg, "/v1/relay")?;
    let body = serde_json::json!({ "peer": atenea_crypto::hex::encode(&peer) })
        .to_string()
        .into_bytes();
    let auth = sign_request(key, device, "POST", &pq, &body, ts, nonce());
    let mut response = agent(Duration::from_secs(15))
        .post(&url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .send(&body[..])
        .map_err(|_| "error de red")?;
    match response.status().as_u16() {
        200 => {
            let bytes = read_limited(&mut response)?;
            let raw: RawRelay = serde_json::from_slice(&bytes).map_err(|_| "respuesta inválida")?;
            grant(&raw.token, raw.relays).ok_or("respuesta inválida")
        }
        503 => Err("relay no disponible"),
        c => Err(status_error(c)),
    }
}

/// Espera avisos de pares que quieren conectar (long-poll de hasta `wait` segundos).
pub fn events(
    cfg: &CoordinatorConfig,
    key: &SigningKey,
    device: DeviceId,
    ts: u64,
    wait: u64,
) -> Result<Vec<Event>, &'static str> {
    let (url, pq) = target(cfg, &format!("/v1/events?wait={wait}"))?;
    let auth = sign_request(key, device, "GET", &pq, &[], ts, nonce());
    let mut response = agent(Duration::from_secs(wait + 10))
        .get(&url)
        .header("Authorization", &auth)
        .call()
        .map_err(|_| "error de red")?;
    match response.status().as_u16() {
        200 => {
            let bytes = read_limited(&mut response)?;
            let raw: RawEvents =
                serde_json::from_slice(&bytes).map_err(|_| "respuesta inválida")?;
            Ok(raw
                .events
                .iter()
                .filter_map(|e| {
                    let peer = atenea_crypto::hex::decode::<16>(&e.peer)?;
                    match e.kind.as_str() {
                        "connect" => Some(Event::Connect {
                            peer,
                            candidates: parse_candidates(&e.candidates),
                        }),
                        "relay" => Some(Event::Relay {
                            peer,
                            grant: grant(e.token.as_deref()?, e.relays.clone())?,
                        }),
                        _ => None,
                    }
                })
                .collect())
        }
        c => Err(status_error(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;

    fn cfg(url: &str) -> CoordinatorConfig {
        CoordinatorConfig {
            url: url.into(),
            stun: vec!["h:1".into(), "h:2".into()],
            relay_ca_file: None,
            relay: "auto".into(),
            insecure_http: false,
        }
    }

    /// Servidor HTTP local que responde una sola vez con el estado y cuerpo dados y devuelve la petición.
    fn serve(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let (status, body) = (status.to_string(), body.to_string());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            // Cabeceras y cuerpo pueden llegar en segmentos distintos: se lee hasta cubrir Content-Length.
            let mut raw = Vec::new();
            let mut buf = vec![0u8; 65536];
            loop {
                let n = s.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let want = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if raw.len() >= end + 4 + want {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&raw).to_string();
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(reply.as_bytes()).unwrap();
            req
        });
        (url, h)
    }

    #[tokio::test]
    async fn relay_destinations_are_vetted_and_bounded() {
        let tunnel = [Prefix {
            addr: "10.1.0.0".parse().unwrap(),
            len: 16,
        }];
        let endpoint = |udp: &str, tls: Option<&str>| RelayEndpoint {
            name: "r".into(),
            udp: Some(udp.into()),
            tls: tls.map(Into::into),
            tls_name: None,
        };
        let vetted = vet_relays(
            vec![
                endpoint("127.0.0.1:4000", None),
                endpoint("169.254.169.254:80", None),
                endpoint("10.1.2.3:4000", Some("[::1]:443")),
                endpoint("198.51.100.7:4000", Some("localhost:443")),
                endpoint("198.51.100.8:4000", None),
                endpoint("198.51.100.9:4000", None),
                endpoint("198.51.100.10:4000", None),
                endpoint("198.51.100.11:4000", None),
            ],
            &tunnel,
        )
        .await;
        // Solo se consideran 4 relays; de ellos, uno conserva su UDP público y pierde el TLS a loopback.
        assert_eq!(vetted.len(), 1);
        assert_eq!(vetted[0].udp.as_deref(), Some("198.51.100.7:4000"));
        assert!(vetted[0].tls.is_none());
    }

    #[test]
    fn target_keeps_the_exact_signed_path() {
        let (url, pq) = target(&cfg("https://h.test/prefix/"), "/v1/events?wait=25").unwrap();
        assert_eq!(url, "https://h.test/prefix/v1/events?wait=25");
        assert_eq!(pq, "/prefix/v1/events?wait=25");
    }

    #[test]
    fn rendezvous_parses_candidates_and_ignores_garbage() {
        let key = atenea_identity::signing_key([1; 32]);
        let (url, h) = serve(
            "200 OK",
            r#"{"peer":"00","candidates":["192.0.2.1:5","basura","[2001:db8::1]:9"]}"#,
        );
        let c = rendezvous(&cfg(&url), &key, [2; 16], [3; 16], 100).unwrap();
        assert_eq!(
            c,
            vec![
                "192.0.2.1:5".parse().unwrap(),
                "[2001:db8::1]:9".parse().unwrap()
            ]
        );
        let req = h.join().unwrap();
        assert!(req.starts_with("POST /v1/rendezvous"));
        assert!(
            req.to_lowercase()
                .contains("authorization: atdevice v1 device=")
        );
    }

    #[test]
    fn events_only_connect_kind_with_valid_peers() {
        let key = atenea_identity::signing_key([1; 32]);
        let peer = "03".repeat(16);
        let body = format!(
            r#"{{"events":[{{"kind":"connect","peer":"{peer}","candidates":["192.0.2.7:1"],"seq":1}},{{"kind":"otra","peer":"{peer}","candidates":[]}},{{"kind":"connect","peer":"zz","candidates":[]}}]}}"#
        );
        let (url, h) = serve("200 OK", &body);
        let ev = events(&cfg(&url), &key, [2; 16], 100, 1).unwrap();
        assert_eq!(
            ev,
            vec![Event::Connect {
                peer: [3; 16],
                candidates: vec!["192.0.2.7:1".parse().unwrap()]
            }]
        );
        assert!(h.join().unwrap().starts_with("GET /v1/events?wait=1"));
    }

    #[test]
    fn relay_grant_and_relay_events() {
        let key = atenea_identity::signing_key([1; 32]);
        let token = base64::engine::general_purpose::STANDARD.encode([7u8; 173]);
        let relays = r#"[{"name":"r1","udp":"198.51.100.10:443","tls":"198.51.100.10:443","tls_name":"relay.atenea.one"}]"#;
        let (url, h) = serve(
            "200 OK",
            &format!(r#"{{"token":"{token}","relays":{relays},"ttl_secs":300}}"#),
        );
        let g = relay(&cfg(&url), &key, [2; 16], [3; 16], 100).unwrap();
        assert_eq!(g.token, vec![7u8; 173]);
        assert_eq!(g.relays.len(), 1);
        assert_eq!(g.relays[0].tls_name.as_deref(), Some("relay.atenea.one"));
        assert!(h.join().unwrap().starts_with("POST /v1/relay"));
        // token de longitud incorrecta: se rechaza
        let short = base64::engine::general_purpose::STANDARD.encode([7u8; 10]);
        let (url, h) = serve("200 OK", &format!(r#"{{"token":"{short}","relays":[]}}"#));
        assert_eq!(
            relay(&cfg(&url), &key, [2; 16], [3; 16], 100),
            Err("respuesta inválida")
        );
        h.join().unwrap();
        // evento relay válido y evento relay con token corrupto (ignorado)
        let peer = "03".repeat(16);
        let body = format!(
            r#"{{"events":[{{"kind":"relay","peer":"{peer}","token":"{token}","relays":{relays}}},{{"kind":"relay","peer":"{peer}","token":"!!","relays":[]}}]}}"#
        );
        let (url, h) = serve("200 OK", &body);
        let ev = events(&cfg(&url), &key, [2; 16], 100, 1).unwrap();
        assert_eq!(ev.len(), 1);
        match &ev[0] {
            Event::Relay { peer, grant } => {
                assert_eq!(*peer, [3; 16]);
                assert_eq!(grant.token.len(), 173);
            }
            other => panic!("{other:?}"),
        }
        h.join().unwrap();
    }
    #[test]
    fn announce_sends_base64_config_and_maps_errors() {
        let key = atenea_identity::signing_key([1; 32]);
        let (url, h) = serve("200 OK", "{}");
        announce(
            &cfg(&url),
            &key,
            [2; 16],
            b"AOCFxx",
            &["192.0.2.1:9".parse().unwrap()],
            100,
        )
        .unwrap();
        let req = h.join().unwrap();
        assert!(req.contains("QU9DRnh4")); // base64("AOCFxx")
        assert!(req.contains("192.0.2.1:9"));
        for (st, msg) in [
            ("403 Forbidden", "acceso denegado"),
            ("404 Not Found", "par no disponible"),
            ("429 Too Many Requests", "límite de tasa"),
            ("500 Internal Server Error", "error HTTP del coordinator"),
        ] {
            let (url, h) = serve(st, "{}");
            assert_eq!(
                announce(&cfg(&url), &key, [2; 16], b"AOCF", &[], 100),
                Err(msg)
            );
            h.join().unwrap();
        }
        assert_eq!(
            announce(&cfg("http://127.0.0.1:1"), &key, [2; 16], b"AOCF", &[], 100),
            Err("error de red")
        );
    }
}
