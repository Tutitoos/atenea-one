# Inventario de dependencias y licencias (fase 0)

- **Fecha de consulta:** 2026-10-24 (crates.io y npm). Son las últimas versiones estables **observadas**; no se ha compilado ni resuelto ninguna. Las versiones se fijan con el primer `Cargo.lock` / `pnpm-lock.yaml` de cada componente.
- **Licencia del proyecto:** Apache-2.0 (ADR-006, pendiente de ratificar). Todas las licencias de abajo son permisivas y compatibles con su redistribución, salvo lo marcado.

## Rust

| Crate | Versión observada | Licencia | Uso | Notas |
| --- | --- | --- | --- | --- |
| `snow` | 0.10.0 | Apache-2.0 OR MIT | Noise IK, `StatelessTransportState` (ADR-001/003) | Confirmar compilación del patrón y de la conversión `TryFrom<HandshakeState>` |
| `ed25519-dalek` | 3.0.0 | BSD-3-Clause | Firmas (contrato, ADR-005) | Confirmar API de verificación estricta |
| `x25519-dalek` | 3.0.0 | BSD-3-Clause | Claves estáticas de dispositivo | Comprobar compatibilidad con el resolver de `snow` |
| `blake2` | 0.11.0 | MIT OR Apache-2.0 | `key_id` y huellas | Misma familia que el hash del patrón Noise |
| `tokio` | 1.54.1 | MIT | Runtime asíncrono | |
| `axum` | 0.8.9 | MIT | Control plane (fase 3B) | |
| `sqlx` | 0.9.0 | MIT OR Apache-2.0 | PostgreSQL (fase 3B) | |
| `utoipa` | 6.0.0 | MIT OR Apache-2.0 (a confirmar) | OpenAPI (fase 3B) | Licencia no consultada con certeza |
| `rustls` | 0.23.46 (estable) | Apache-2.0 OR ISC OR MIT | TLS del relay (fase 5) | crates.io muestra además una `0.24.0-dev`; usar la estable |
| `clap` | 4.6.7 | MIT OR Apache-2.0 | CLI | |
| `tracing`, `serde` | 0.1.44, 1.0.229 | MIT OR Apache-2.0 | Registro y serialización | |
| `tun` | 0.8.14 | **WTFPL** | Interfaz TUN Linux (fase 2) | Licencia permisiva no estándar: evaluar frente a `tun-rs` u otra, o la llamada directa a `/dev/net/tun`, antes de depender de ella |
| `ureq` | 3.5.0 (requisito `3.1`) | MIT OR Apache-2.0 | Sincronización del agente y enrolamiento CLI | `default-features = false`, solo `rustls`; sin OpenSSL, compresión ni redirecciones |
| `ureq-proto`, `http`, `httparse` | 0.6.5, 1.5.0, 1.10.1 | MIT OR Apache-2.0 | Transporte HTTP de ureq | Licencias leídas en los manifiestos descargados por Cargo |
| `utf8-zero`, `base64`, `percent-encoding` | 0.8.1, 0.23.1, 2.3.2 | MIT OR Apache-2.0 | Codificación de ureq | |
| `rustls-pki-types`, `rustls-webpki` | 1.x, 0.103.x | MIT OR Apache-2.0; ISC, respectivamente | Tipos y validación TLS | `rustls` 0.23.46: Apache-2.0 OR ISC OR MIT; reutiliza `ring` ya aceptado abajo |
| `webpki-roots` | 1.0.11 | CDLA-Permissive-2.0 | Anclas públicas TLS | Licencia permisiva de datos; no sustituye el pinning de la raíz de Atenea |

La CLI añade `serde_json` 1.0 (MIT OR Apache-2.0), ya presente en el workspace.
El límite HTTP no sustituye `verify_config`: las configuraciones descargadas solo
se autorizan al pasar la verificación criptográfica y el estado antirrollback existentes.

### Control plane (fase 3B)

Requisitos directos de `apps/control-plane/Cargo.toml` (versiones exactas resueltas en `Cargo.lock`):

| Crate | Requisito | Licencia | Uso |
| --- | --- | --- | --- |
| `axum` | 0.8 | MIT | Router HTTP |
| `sqlx` | 0.8 | MIT OR Apache-2.0 | PostgreSQL, consultas en ejecución y migraciones |
| `openidconnect` | 4 | MIT | Discovery, PKCE y validación de ID tokens |
| `reqwest` | 0.12 | MIT OR Apache-2.0 | Transporte OIDC con rustls, sin redirecciones |
| `figment` | 0.10 | MIT OR Apache-2.0 | TOML opcional y variables de entorno |
| `utoipa` | 5 | MIT OR Apache-2.0 | Contrato OpenAPI del router |
| `uuid` | 1 | MIT OR Apache-2.0 | Identificadores UUID |
| `chrono` | 0.4 | MIT OR Apache-2.0 | Tiempos UTC |
| `ipnet` | 2 | MIT OR Apache-2.0 | Validación de prefijos e IPAM |
| `sha2` | 0.10 | MIT OR Apache-2.0 | Hash de tokens de sesión y enrolamiento |
| `hex` | 0.4 | MIT OR Apache-2.0 | Codificación pública y fichero de emisor |
| `base64` | 0.22 | MIT OR Apache-2.0 | Tokens base64url |
| `tower` | 0.5 | MIT | Router en memoria (pruebas) |
| `http-body-util` | 0.1 | MIT | Lectura de respuestas (pruebas) |
| `jsonwebtoken` | 10 | MIT | Proveedor OIDC falso (solo pruebas) |
| `rsa` | 0.9 | MIT OR Apache-2.0 | Clave RSA efímera de 2048 bits y JWKS del proveedor OIDC de pruebas; PKCS#8/PEM solo en memoria |
| `url` | 2 | MIT OR Apache-2.0 | URLs del proveedor de pruebas |

Reutiliza `serde`, `serde_json`, `tokio`, `tracing`, `tracing-subscriber`, `zeroize`
y `atenea-identity`. Sin dependencias operativas del panel, coordinator ni relays.

### Coordinator (fase 4)

`apps/coordinator` reutiliza `atenea-identity` (inspección sin estado y firmas de
petición) y `atenea-nat-traversal` (STUN y saneamiento de candidatos), sin nuevas
dependencias criptográficas ni servicios externos. Requisitos directos:

| Crate | Requisito | Licencia | Uso |
| --- | --- | --- | --- |
| `axum` | 0.8 | MIT | HTTP, router y dirección TCP observada |
| `base64` | 0.22 | MIT OR Apache-2.0 | Configuración firmada en JSON |
| `figment` | 0.10 | MIT OR Apache-2.0 | TOML opcional y variables de entorno |
| `hex` | 0.4 | MIT OR Apache-2.0 | Identificadores y anclas públicas |
| `serde`, `serde_json` | 1, 1 | MIT OR Apache-2.0 | JSON y configuración |
| `tokio` | 1 | MIT | UDP, long-poll, señales y barrido |
| `tracing`, `tracing-subscriber` | 0.1, 0.3 | MIT | Registro sin secretos (tracing también Apache-2.0) |
| `tower`, `http-body-util` | 0.5, 0.1 | MIT | Router en memoria y cuerpos, solo pruebas |

Todos estos crates ya se usan en el workspace; versiones resueltas en
`Cargo.lock`. No añade TLS, base de datos ni cliente STUN público.

## TypeScript / panel (fase 6)

| Paquete | Versión observada | Licencia |
| --- | --- | --- |
| `react-router` | 8.5.0 | MIT |
| `react` | 19.3.1 | MIT |
| `@tanstack/react-query` | 5.104.5 | MIT |
| `zod` | 4.6.7 | MIT |
| `tailwindcss` | 4.3.3 | MIT |
| `recharts` | 3.10.1 | MIT |
| `motion` | 14.6.0 | MIT |

Compatibilidad entre `react-router` 8.5.0, React 19.3.1 y el resto: **no verificada**; se comprueba al crear `apps/web`.

## Pendiente

- Wintun (licencia propia, redistribución; fase 8): revisar el texto de la licencia antes de adoptarlo. `utoipa` 5.5.0 declara MIT OR Apache-2.0 en su manifiesto descargado.
- `tun` (WTFPL) **no se adoptó**: `atenea-tunnel` usa `/dev/net/tun` directamente con `libc` (código `unsafe` acotado y comentado).
- Añadir `cargo deny` / auditoría de dependencias (RustSec) al CI. Ya hay un `Cargo.lock` real: queda pendiente.
- **`ring` 0.17.14 llega a través de `snow` 0.10.0** con las características por defecto (visible con `cargo tree -p atenea-crypto -e features -i ring`). Su licencia es propia (mezcla ISC/OpenSSL). **Aceptado por el responsable "por ahora" (2026-10-24)**; se revisará el texto de la licencia antes del primer release.
- Resueltas y en uso (versiones del `Cargo.lock`): `snow` 0.10.0, `x25519-dalek` 3.0.0, `ed25519-dalek` 3.0.0, `blake2` 0.11.0 (identity) y 0.10 (dentro de snow), `getrandom` 0.3, `zeroize`, `tokio` 1.54, `clap` 4.6, `serde`/`serde_json`/`toml` 0.9, `tracing`, `proptest` 1, `libc` 0.2. No se han leído las licencias de las dependencias transitivas.
