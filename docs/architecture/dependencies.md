# Inventario de dependencias y licencias (fase 0)

- **Fecha de consulta:** 2026-10-24 (crates.io y npm). Son las últimas versiones estables **observadas**; no se ha compilado ni resuelto ninguna. Las versiones se fijan con el primer `Cargo.lock` / `pnpm-lock.yaml` de cada componente.
- **Licencia del proyecto:** Apache-2.0 ([ADR-006](ADR-006-licencia.md), aceptado el 2026-10-26). Todas las licencias de abajo son permisivas y compatibles con su redistribución, salvo lo marcado.

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

### Herramienta de carga (fase 11, solo pruebas)

`crates/testpeer` (`publish = false`) añade como dependencia directa
`ed25519-dalek = "3.0.0"` para el binario auxiliar `atenea-load-sign`: genera
fixtures sintéticas verificadas y firmas de carga. Reutiliza la versión y
checksum ya fijados; `Cargo.lock` solo añade esa arista a `atenea-testpeer`,
sin paquetes nuevos. Reutiliza `atenea-identity`, `atenea-crypto` y
`atenea-protocol`; no introduce primitivas alternativas ni librerías Python.
`scripts/release/build-local.sh` construye y copia una lista explícita de los
cinco binarios de producto; Debian, macOS y Windows empaquetan exclusivamente
`atenea` y `atenea-agent`. El helper no se distribuye en esos artefactos.


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

- Avisos: el [`NOTICE`](../../NOTICE) del repositorio declara `Copyright 2026 Tutitoos` (2026-10-26) y remite a este documento para los avisos de terceros; el instalador de Windows conserva `WINTUN-LICENSE.txt` de WireGuard LLC.
- Wintun: excepción de controlador aceptada por el responsable el 2026-10-25; licencia de binarios oficiales revisada, véase fase 8 Windows abajo. `utoipa` 5.5.0 declara MIT OR Apache-2.0 en su manifiesto descargado.
- `tun` (WTFPL) **no se adoptó**: `atenea-tunnel` usa `/dev/net/tun` directamente con `libc` (código `unsafe` acotado y comentado).
- Escaneo de dependencias en CI (job `supply-chain`, también semanal): `cargo deny check advisories bans licenses sources` con `deny.toml` (solo grafo de producción; licencias permitidas: Apache-2.0, MIT, ISC, BSD-2/3-Clause, Unicode-3.0, Zlib, CDLA-Permissive-2.0, BSL-1.0, Unlicense; fuentes solo crates.io) y `pnpm audit --prod`. Dependabot propone actualizaciones de cargo, npm, GitHub Actions y Docker. Las excepciones requieren razón, responsable y fecha de revisión (véase abajo). Wintun no es un crate: su licencia y hash se revisan aparte y figuran en el SBOM de Windows. La auditoría usa el toolchain de `rust-toolchain.toml` tanto para instalarlo como para seleccionarlo dentro del contenedor de la acción. Todos los paquetes internos declaran `publish = false`: admiten dependencias por ruta sin versión sin permitir comodines externos.
- pnpm se instala con `scripts/install-pnpm.sh`, que verifica el SHA-512 del tarball de npm de la versión fijada (CI y Dockerfile del control plane); al subir de versión hay que actualizar el script y `packageManager`.
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
| `rustls::pki_types` | 1.15.1 (reexportado por rustls) | MIT OR Apache-2.0 | `pem::PemObject` para certificado y clave PEM; sin dependencia adicional |
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

### Fase 8 — Windows

| Dependencia | Versión fijada | Licencia | Uso |
| --- | --- | --- | --- |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 | DACL, canalizaciones, eventos Wintun y API Win32, solo Windows |
| `windows-service` | 0.8.0 | MIT OR Apache-2.0 | Integración SCM y notificaciones de parada/apagado |
| `libloading` | 0.8.9 | ISC | Carga de la API de Wintun en ejecución |
| `atenea-windows` | workspace | Apache-2.0 del proyecto | Frontera Win32 con `unsafe` acotado para permisos/IPC |
| Wintun, DLL oficial | 0.14.1 | **Prebuilt Binaries License**, propia de WireGuard LLC | Única excepción de controlador aceptada por el responsable el 2026-10-25 |

Fuentes de las licencias Rust: metadatos oficiales de
[`windows-sys`](https://crates.io/crates/windows-sys/0.61.2),
[`windows-service`](https://crates.io/crates/windows-service/0.8.0) y
[`libloading`](https://crates.io/crates/libloading/0.8.9).
La licencia del código fuente Wintun (GPL-2.0) no sustituye la licencia propia de
las DLL precompiladas. Se descargó y leyó `wintun/LICENSE.txt` del
[ZIP oficial 0.14.1](https://www.wintun.net/builds/wintun-0.14.1.zip)
el 2026-10-25: concede uso mediante la API, prohíbe modificar la DLL, retirar avisos
o usar la marca como aval; la redistribución solo se permite junto al software que
la utiliza mediante la API (u otro consentimiento escrito). No se usa el protocolo WireGuard.

El repositorio y el ZIP de Atenea **no incluyen la DLL**. El instalador la descarga
desde HTTPS oficial, valida SHA256
`07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51`
antes de extraer, valida Authenticode y conserva `WINTUN-LICENSE.txt`.
El hash publicado en [wintun.net](https://www.wintun.net/) se contrastó mediante
descarga y cálculo independiente el 2026-10-25. Sin alternativas de controlador,
hash configurable ni descarga desde terceros. Certificado de firma de Atenea y
validación en máquinas reales pendientes; la firma oficial de Wintun no firma el cliente.

### Fase 9 — Copias y recuperación

| Dependencia | Versión observada / fijación | Licencia | Uso |
| --- | --- | --- | --- |
| PostgreSQL, imagen oficial | `postgres:17@sha256:b8faeff7712663c2773a190181e1246e37e364b887611c04658c28b44e2408fe` (17.11, índice multiarch consultado con `docker buildx imagetools inspect`) | PostgreSQL | Base de `deploy/backup/Dockerfile` |
| pgBackRest | 2.59.3 (`2.59.3-1.pgdg13+1`, paquete PGDG) | MIT | Completas, incrementales, cifrado AES-256-CBC y WAL/PITR |
| age | 1.2.1 (`1.2.1-1+b5`, Debian) | BSD-3-Clause | Copia de secretos a varios destinatarios |
| Python | 3.13, Debian en imagen; biblioteca estándar en el orquestador | PSF | Extracción segura de claves y simulacro Docker |

Fuentes: [pgBackRest](https://pgbackrest.org/), [age](https://github.com/FiloSottile/age), [imagen PostgreSQL](https://hub.docker.com/_/postgres). Los paquetes apt se resuelven al construir: el digest de la base no congela el repositorio apt; el release debe fijar el digest de la imagen resultante y registrar su SBOM.

### Fase 9 — Infraestructura como código

| Dependencia | Versión fijada / observada | Licencia | Uso |
| --- | --- | --- | --- |
| Debian, imagen oficial | `debian:13@sha256:4c25275446ca371766b8b093c696782eabce06263fccacb8d5d813b3dc44b308` | Licencias de paquetes Debian | Systemd, herramientas y ensayo limpio |
| OpenTofu | `ghcr.io/opentofu/opentofu:1.10.6@sha256:879a73a0ee45132abf1b26583e3ab3a1e5443157a13b460114578d13dbe40b6e` | MPL-2.0 | `fmt`, `validate`, `plan` local; binario copiado sin heredar ONBUILD |
| SOPS | `ghcr.io/getsops/sops:v3.11.0@sha256:6ce00ac946e1a52b2d1e5b7087441f7b6ef7bad907753104013f8bdb8f5fd891` | MPL-2.0 | Cifrado y rotación de secretos |
| Ansible core | Debian `2.19.11-0+deb13u1` | GPL-3.0-or-later | Solo `ansible.builtin`; sin Galaxy ni colecciones externas |
| ansible-lint | Debian `25.6.1+really25.2.1-1` | GPL-3.0-or-later | Perfil production, ejecución offline |
| yamllint | Debian `1.37.1-1` | GPL-3.0-or-later | YAML de infraestructura |
| age | Debian `1.2.1-1+b5` | BSD-3-Clause | Identidades efímeras y destinatarios SOPS |
| Provider incorporado Terraform | `terraform.io/builtin/terraform`, incluido en Tofu | Incluido en OpenTofu | `terraform_data`, ningún servicio remoto |
| Provider DNS | `hashicorp/dns` `3.4.3` | MPL-2.0 | Ejemplo RFC2136/TSIG; no aplicado contra DNS real |

Digests de índices consultados con `docker buildx imagetools inspect`; versiones
Debian observadas mediante `dpkg-query` en el contenedor. Los paquetes apt de roles
y herramientas no quedan congelados por el digest base: registrar SBOM y digest
de la imagen construida para un release. Las herramientas GPL se ejecutan como
procesos de operación/lint separados; no se enlazan con los binarios Apache-2.0.
Fuentes: [OpenTofu](https://github.com/opentofu/opentofu),
[SOPS](https://github.com/getsops/sops), [Ansible](https://docs.ansible.com/),
[DNS 3.4.3](https://registry.terraform.io/providers/hashicorp/dns/3.4.3/docs).

### Fase 9 — Stack e imágenes

| Dependencia | Versión / índice fijado | Licencia | Uso |
| --- | --- | --- | --- |
| Rust oficial | `rust:1.99@sha256:931c2c0531a8b143e044f0f3914dfe802f15bb3d1955f0b657e9359327ac2ffc` | MIT/Apache-2.0 y paquetes Debian | Compilación `--locked` multi-etapa |
| Distroless CC Debian 13 | `gcr.io/distroless/cc-debian13:nonroot@sha256:3ee00291cf386ec52a552e6a858d555debd74776eb4bfa53fa3df2cd6677515e` | Apache-2.0 y paquetes Debian | Runtime no root y sin shell |
| Node oficial | `node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205` | MIT y paquetes Debian | Build estático del panel con pnpm 11.23.0 |
| HAProxy oficial | `haproxy:3.2-alpine@sha256:5893840576071e6217cd52ebdcc14f454c6f7a631056fa6f4e0f40d3cac4215b` | GPL-2.0/LGPL-2.1 | Entrada TCP L4, SNI/ALPN, PROXYv2 |
| Caddy oficial | `caddy:2.10-alpine@sha256:4c6e91c6ed0e2fa03efd5b44747b625fec79bc9cd06ac5235a779726618e530d` | Apache-2.0 | HTTPS panel, HTTP-01 y CA local |
| Dex | `ghcr.io/dexidp/dex:v2.41.1@sha256:bc7cfce7c17f52864e2bb2a4dc1d2f86a41e3019f6d42e81d92a301fad0c8a1d` | Apache-2.0 | OIDC del piloto |

Digests reales consultados con `docker buildx imagetools inspect`; PostgreSQL 17 usa el índice documentado en copias. Fuentes: [Distroless](https://github.com/GoogleContainerTools/distroless), [Rust](https://hub.docker.com/_/rust), [HAProxy](https://hub.docker.com/_/haproxy), [Caddy](https://hub.docker.com/_/caddy), [Dex](https://github.com/dexidp/dex). No hay nuevas dependencias Rust externas para bootstrap ni healthchecks.

### Fase 9 — observabilidad

El crate local `atenea-observability` reutiliza Axum 0.8.9 (MIT) y Tokio 1
(MIT), ya presentes en el workspace. No se incorpora SDK OpenTelemetry ni otro
registro Prometheus: son contadores enteros, un histograma de seis límites
constantes y texto Prometheus 0.0.4, contrastado por un scrape de Prometheus real.

| Dependencia separada | Versión | Licencia | Uso |
| --- | --- | --- | --- |
| Prometheus | 3.2.1 | Apache-2.0 | Scrape, PromQL, reglas y promtool |
| node_exporter | 1.9.1 | Apache-2.0 | Solo colector de sistemas de ficheros |
| Grafana | 11.6.0 | AGPL-3.0 | Paneles provisionados |
| Loki | 3.4.2 | AGPL-3.0 | Registros de siete días |
| Alloy | 1.8.3 | Apache-2.0 | Lectura de JSON Docker sin acceso al socket Docker |

Los cinco índices multiarch se obtuvieron con `docker buildx imagetools inspect`
y están fijados por digest en `deploy/compose/prod/observability.yaml`.
Fuentes: [Prometheus](https://github.com/prometheus/prometheus),
[node_exporter](https://github.com/prometheus/node_exporter),
[Grafana](https://github.com/grafana/grafana),
[Loki](https://github.com/grafana/loki) y [Alloy](https://github.com/grafana/alloy).
Se ejecutan como procesos independientes: no se enlazan con los binarios Atenea.

### Fase 9 — release

Sin nuevos crates, paquetes npm ni herramientas descargadas. Python 3.11+
(PSF-2.0, biblioteca estándar `tomllib`, `tarfile`, `hashlib`, `json`) genera
CycloneDX 1.5 y el manifiesto. OpenSSL 3 (Apache-2.0) firma/verifica Ed25519 con
`pkeyutl -rawin`; Docker exporta imágenes locales y dpkg construye el `.deb`.
El parser propio admite exclusivamente el formato pnpm lock v9; no inventa
licencias npm ausentes del lock ni versiones de ejecutables estáticos.
Las acciones upload/download-artifact v4 solo intervienen en runners de GitHub.


## Excepciones temporales de auditoría (2026-10-25)

Responsable: responsable de seguridad de Atenea One. Revisar **antes de 2027-01-23**
(máximo tres meses); las entradas concretas están en `deny.toml`. No se deshabilitan
las comprobaciones de advisories ni de fuentes.

- **RUSTSEC-2026-0387 — `crypto-bigint` 0.5.5.**
  [Advisory](https://rustsec.org/advisories/RUSTSEC-2026-0387). El problema es
  variabilidad temporal con enteros secretos. El grafo de producción
  (`cargo tree --offline --locked -e normal,build -i crypto-bigint`) solo llega
  por `openidconnect` → `p256`/`p384` → `elliptic-curve`. En
  `apps/control-plane/src/auth.rs::callback` se verifica el ID token; operaciones
  afectadas solo sobre enteros públicos (verificación); sin secretos ECDSA en el
  proceso. `openidconnect::core::crypto::verify_ec_signature` usa
  `p256::ecdsa::VerifyingKey` y `p384::ecdsa::VerifyingKey` con la clave pública del
  IdP, la firma y el mensaje. No hay firma, generación de claves ni DH con estos
  crates en producción. La versión corregida es ≥ 0.7.6, incompatible con
  `elliptic-curve` 0.13.8 (`crypto-bigint ^0.5`). El índice consultado el
  2026-10-25 solo publica `openidconnect` estable hasta 4.0.1, que exige `p256
  ^0.13.2` y `p384 ^0.13.0`; los últimos compatibles siguen en 0.13.
  **Salida:** openidconnect estable con p256/p384 0.14, siempre que el grafo
  resultante resuelva crypto-bigint ≥ 0.7.6; retirar la excepción tras verificarlo.
- **RUSTSEC-2023-0071 — `rsa` 0.9.10.**
  [Advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071). El flujo de
  producción es el mismo callback OIDC y solo verifica firmas con la clave pública
  del IdP (`openidconnect::core::crypto::verify_rsa_signature` construye
  `rsa::RsaPublicKey` y llama a `verify`); no hay operaciones con clave privada RSA.
  La clave privada de `apps/control-plane/tests/oidc.rs` es efímera y solo del
  proveedor de pruebas. Upstream no publica una versión corregida.
  **Salida:** versión corregida de rsa compatible con openidconnect, o retirada
  upstream de esa dependencia; retirar la excepción después de la actualización.

Ambas excepciones dejan de ser válidas si se introduce uso de claves privadas
RSA/ECDSA o DH con estas dependencias: revisar antes de cambiar el flujo.


### Fase 11 — Wire v2

`crates/crypto` hace directas dos dependencias ya presentes transitivamente en
`Cargo.lock`, sin paquetes ni versiones nuevos:

| Crate | Versión fijada | Licencia | Uso |
| --- | --- | --- | --- |
| `blake2` | 0.11.0 | MIT OR Apache-2.0 | `Blake2sMac<U16>` keyed, salida de 16 bytes parametrizada, MAC1/MAC2 y cookies |
| `chacha20poly1305` | 0.10.1 | Apache-2.0 OR MIT | XChaCha20Poly1305 exclusivamente para COOKIE_REPLY; Noise conserva ChaChaPoly |


### Fase 11 — Relay v2

`crates/relay-core` hace directas `zeroize` (workspace, ya fijada) para borrar el
secreto de cookies al destruirlo, y `subtle` 2.6.1 (BSD-3-Clause, ya transitiva de
dalek) para comparar cookies en tiempo constante. No se añade ningún paquete,
versión ni checksum al lock; solo listas de dependencias. Conserva
`atenea-protocol` para reutilizar las constantes normativas de wire-v2 sin deriva.


### Aplicación de escritorio (ADR-011)

`apps/desktop` tiene `Cargo.lock` propio (417 paquetes en el lock, que incluye los de las tres plataformas; árbol de producción medido el 2026-10-27 con `cargo tree --locked -e normal`: 213 en macOS, 254 en Linux, 222 en Windows). Directas:

| Crate | Versión fijada | Licencia | Uso |
| --- | --- | --- | --- |
| `tauri` | 2.12.5 | Apache-2.0 OR MIT | Ventana, webview del sistema, bandeja (`tray-icon`) e imagen PNG (`image-png`) |
| `tauri-build` | 2.x (lock) | Apache-2.0 OR MIT | Script de compilación y esquemas de permisos |
| `serde_json` | 1.x (lock) | MIT OR Apache-2.0 | Validar y leer la respuesta de estado |
| `atenea-windows` | ruta local | Apache-2.0 | Canalización con nombre (solo Windows) |

Licencias: todo el árbol está en la lista permitida **salvo cinco crates con MPL-2.0** (`cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`), admitidos como excepciones por crate en `deny.toml` (ADR-011 §5). La comprobación inicial se hizo con un script sobre `cargo tree`; `cargo-deny` (advisories incluidos) lo ejecuta la CI en el job `supply-chain`, con un resultado que no se conocía al escribir esto.

