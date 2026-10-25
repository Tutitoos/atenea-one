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

### Fase 5 — Coordinator relay

`apps/coordinator` añade únicamente `atenea-relay-core` (crate interna, Apache-2.0)
para emitir tokens; reutiliza Ed25519 y el RNG del sistema a través de
`atenea-identity`, y las dependencias Figment/serde/hex/base64 existentes.
Sin nuevas dependencias externas, TLS ni certificados en este componente.

### Fase 5 — Cliente relay del agente

| Crate | Requisito | Licencia | Uso |
| --- | --- | --- | --- |
| `rustls` | 0.23 | Apache-2.0 OR ISC OR MIT | TLS 1.3, proveedor `ring`, sin características por defecto |
| `tokio-rustls` | 0.26 | MIT OR Apache-2.0 | Transporte TLS asíncrono, solo proveedor `ring` |
| `webpki-roots` | 1 | CDLA-Permissive-2.0 | Raíces públicas, ampliadas por PEM local opcional |
| `socket2` | 0.6 | MIT OR Apache-2.0 | Un socket UDP dual-stack por cliente, separado del socket de datos |
| `rcgen` | 0.14 | MIT OR Apache-2.0 | Solo pruebas: certificados y claves efímeros generados en ejecución |

Reutiliza `atenea-relay-core`, Tokio y serde. Colas de entrada, salida y eventos
acotadas a 256 mensajes; los descartes se contabilizan sin registrar secretos.

### Fase 5 — Servidor relay

| Crate | Requisito | Licencia | Uso |
| --- | --- | --- | --- |
| `rustls` | 0.23 | Apache-2.0 OR ISC OR MIT | TLS 1.3 exclusivamente, proveedor `ring`, sin características por defecto; `tls12` solo en pruebas de rechazo |
| `tokio-rustls` | 0.26 | MIT OR Apache-2.0 | TLS asíncrono, proveedor `ring`, sin características por defecto |
| `rustls-pemfile` | 2 | Apache-2.0 OR ISC OR MIT | Certificado y clave PEM |
| `rand` | 0.8 | MIT OR Apache-2.0 | `OsRng`: secreto de cookies e identificadores impredecibles |
| `figment`, `hex` | 0.10, 0.4 | MIT OR Apache-2.0 | Configuración TOML/entorno y claves públicas hex |
| `serde`, `serde_json` | 1, 1 | MIT OR Apache-2.0 | Estado y configuración |
| `tokio` | 1 | MIT | Sockets, tareas acotadas, señales y plazos |
| `tracing`, `tracing-subscriber` | 0.1, 0.3 | MIT (tracing también Apache-2.0) | Registro sin secretos |
| `rcgen` | 0.14 | MIT OR Apache-2.0 | Solo pruebas: certificados y claves efímeros |

Reutiliza `atenea-identity` y `atenea-relay-core` (Apache-2.0). El proveedor
`ring` conserva la aceptación indicada arriba; no se añade `aws-lc-rs`.

### Fase 6 — Control plane y estado del agente

Versiones fijadas en manifiesto y `Cargo.lock`; sin nuevos servicios externos.
El agente reutiliza su cliente `ureq`, Tokio, serde y `atenea-identity`.
Los estáticos usan Tokio y Axum existentes, sin otro servidor de ficheros.

| Crate | Versión fijada | Licencia | Motivo |
| --- | --- | --- | --- |
| `axum` (característica `ws`) | 0.8.9 | MIT | Upgrade y tramas WebSocket del panel |
| `tokio-tungstenite` | 0.29.0 | MIT | Transporte WebSocket de Axum y cliente TCP real de integración |
| `tungstenite` | 0.29.0 | MIT OR Apache-2.0 | Protocolo WebSocket transitivo |
| `rand`, `rand_core`, `rand_chacha` | 0.9.5, 0.9.5, 0.9.0 | MIT OR Apache-2.0 | Aleatoriedad del protocolo WebSocket transitivo |
| `futures-util`, `futures-macro` | 0.3.34, 0.3.34 | MIT OR Apache-2.0 | Lectura asíncrona del cliente de pruebas; macro transitiva |
| `url` | 2.5.8 | MIT OR Apache-2.0 | Dependencia de pruebas ya existente, ahora fijada; producción reutiliza `reqwest::Url` para validar Origin |

### Fase 6 — Panel web

Dependencias directas fijadas exactamente en los manifiestos y `pnpm-lock.yaml`.
Licencias consultadas en los metadatos oficiales npm. Los componentes locales
de diálogo siguen la composición shadcn/ui (MIT) sobre Radix; no hay CDN.

| Paquete | Versión fijada | Licencia | Motivo |
| --- | --- | --- | --- |
| `react`, `react-dom` | 19.3.1 | MIT | Interfaz y renderizado |
| `react-router`, `@react-router/dev`, `@react-router/node` | 8.5.0 | MIT | Framework SPA estático y compilación |
| `vite` | 8.3.5 | MIT | Servidor de desarrollo y bundle |
| `typescript` | 5.9.3 | Apache-2.0 | Modo estricto; el generador requiere la API compiler JS, ausente en TS 7 |
| `tailwindcss`, `@tailwindcss/vite` | 4.3.3 | MIT | CSS local y compilación |
| `@tanstack/react-query` | 5.104.5 | MIT | Caché REST, invalidación y respaldo por sondeo |
| `@tanstack/react-table` | 9.3.1 | MIT | Inventario tipado |
| `@tanstack/react-form` | 1.33.5 | MIT | Estado y envío de formularios |
| `zod` | 4.6.7 | MIT | Validación de límites de entrada |
| `recharts` | 3.10.1 | MIT | Puntos reales de métricas |
| `motion` | 14.6.0 | MIT | Aparición de diálogo respetando movimiento reducido |
| `geist` | 1.8.2 | OFL-1.1 | Fuente variable servida localmente |
| `radix-ui` | 1.8.1 | MIT | Diálogos accesibles y gestión del foco |
| `openapi-typescript` | 7.13.0 | MIT | Generación de tipos desde el contrato Rust |
| `openapi-fetch` | 0.17.0 | MIT | Cliente HTTP tipado con cookies y CSRF en memoria |
| `vitest` | 5.0.5 | MIT | Pruebas unitarias y de componentes |
| `@testing-library/react` | 16.3.3 | MIT | Pruebas de interacción accesibles |
| `@testing-library/jest-dom` | 7.0.1 | MIT | Aserciones DOM |
| `@testing-library/user-event` | 14.6.8 | MIT | Teclado y acciones de usuario |
| `vitest-axe` | 0.1.0 | MIT | Comprobación básica de accesibilidad |
| `jsdom` | 30.1.3 | MIT | DOM para pruebas |
| `eslint` | 10.13.0 | MIT | Lint |
| `@eslint/js` | 10.0.1 | MIT | Reglas recomendadas |
| `typescript-eslint` | 8.72.0 | MIT | Lint TypeScript |
| `eslint-plugin-jsx-a11y` | 6.10.2 | MIT | Accesibilidad estática |
| `@types/react`, `@types/react-dom` | 19.3.0 | MIT | Tipos de React |
| `@types/node` | 26.6.5 | MIT | Tipos de herramientas de compilación |
| `@playwright/test` | 1.64.1 | Apache-2.0 | E2E del panel en Chromium (`tests/e2e-panel`, solo laboratorio; la imagen instala el mismo Chromium) |
| `@axe-core/playwright` | 4.14.0 | MPL-2.0 | Auditoría de accesibilidad (incluye contraste) en el E2E; solo desarrollo, no se distribuye |

Referencias oficiales de implementación: [React](https://react.dev/reference/react),
[React Router SPA](https://reactrouter.com/how-to/spa),
[Vite](https://vite.dev/guide/), [Tailwind](https://tailwindcss.com/docs/installation/using-vite),
[Query](https://tanstack.com/query/latest/docs/framework/react/quick-start),
[Table](https://tanstack.com/table/latest/docs/framework/react/guide/table-state),
[Form](https://tanstack.com/form/latest/docs/framework/react/quick-start),
[Zod](https://zod.dev/api), [Recharts](https://recharts.github.io/en-US/api/Line/),
[Motion](https://motion.dev/docs/react-use-reduced-motion),
[Geist](https://github.com/vercel/geist-font),
[shadcn/ui](https://ui.shadcn.com/docs/components/radix/dialog),
[Radix](https://www.radix-ui.com/primitives/docs/components/dialog),
[OpenAPI](https://openapi-ts.dev/openapi-fetch/middleware-auth),
[Vitest](https://vitest.dev/guide/),
[Testing Library](https://testing-library.com/docs/react-testing-library/intro/),
[vitest-axe](https://github.com/chaance/vitest-axe),
[ESLint](https://eslint.org/docs/latest/use/configure/configuration-files),
[typescript-eslint](https://typescript-eslint.io/getting-started/),
[jsx-a11y](https://github.com/jsx-eslint/eslint-plugin-jsx-a11y).

### Fase 8 — Cliente macOS

Sin dependencias externas nuevas. `atenea-tunnel` reutiliza `libc` 0.2 y Tokio ya resueltos en `Cargo.lock` (MIT OR Apache-2.0 y MIT, respectivamente), ahora también en macOS. El único código unsafe permanece en ese crate: socket de control utun, ioctl, connect, getsockopt, fcntl y readv/writev; sin copia ni asignación por paquete. El agente reutiliza permisos, credenciales Unix y persistencia existentes.

Integración mediante herramientas incluidas en macOS (`ifconfig`, `route`, `netstat`, launchd, `pkgbuild`, `productbuild`, `pkgutil`); no se redistribuyen. Las pruebas privilegiadas usan Python 3 del runner y el par Rust del workspace, no bibliotecas Python externas. Firma/notarización Apple Developer ID pendiente de decisión del responsable.
