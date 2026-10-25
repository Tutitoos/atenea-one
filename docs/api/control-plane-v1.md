# Control plane v1 (fase 3B)

- **Estado:** Base de fase 3B implementada y verificada (pruebas de integración contra PostgreSQL y E2E de laboratorio: [README](../../apps/control-plane/README.md) y [resultados E2E](../operations/e2e-fase3b.md)); extensiones de fase 6 descritas en §10. Pendientes: HTTPS y alta disponibilidad (fase 9).
- **Fecha:** 2026-10-24
- **Implementa:** `apps/control-plane` (monolito modular Axum + PostgreSQL). El panel (fase 6) y el cliente generado consumen este contrato; los agentes usan `GET /api/v1/device/config` y `POST /api/v1/enroll`.

## 1. Principios

- Monolito modular: un único binario. Módulos: `db` (migraciones SQLx), `auth` (OIDC, sesiones, CSRF), `orgs` (organizaciones y RBAC), `networks` (redes, IPAM), `devices` (enrolamiento, inventario, aprobación), `policies` (ACL), `issue` (emisión de configuración firmada), `audit`, `api` (rutas y errores), `config` (Figment, variables `ATENEA_CP_*`).
- **PostgreSQL es la fuente de verdad.** Estado efímero (nonces de peticiones de dispositivo, flujos OIDC) en memoria o en tablas con TTL; se puede reconstruir.
- **El control plane nunca recibe claves privadas de dispositivos.** Recibe el `DeviceBundle` público.
- **La clave raíz NO vive en el control plane (ADR-005).** El control plane guarda solo la clave del emisor (fichero 0600) y, por red, el certificado de emisor firmado offline por la raíz.
- Autorización en el backend (RBAC por organización); la interfaz no es una barrera.
- Errores JSON uniformes `{"error": {"code": "snake_case", "message": "..."}}` con código HTTP coherente; nunca se devuelven trazas internas ni detalles de SQL.
- Tiempos en UTC; identificadores en la API como UUID en texto (los de red y dispositivo son los 16 bytes del contrato en forma UUID).

## 2. Modelo de datos

Migraciones versionadas en `apps/control-plane/migrations/`. Restricciones obligatorias (las pruebas las ejercitan):

| Tabla | Columnas clave y restricciones |
| --- | --- |
| `organizations` | `id uuid pk`, `name text not null`, `created_at` |
| `users` | `id uuid pk`, `oidc_issuer text`, `oidc_subject text`, `email text null`, `display_name text null`, `created_at`; `unique(oidc_issuer, oidc_subject)` (no hay contraseñas locales) |
| `memberships` | `org_id`, `user_id`, `role in ('owner','admin','member')`; `pk(org_id, user_id)` |
| `sessions` | `id` = hash SHA-256 del token de sesión (el token solo existe en la cookie), `user_id`, `csrf_hash`, `created_at`, `expires_at`, `last_seen_at` |
| `oidc_flows` | `state_hash pk`, `pkce_verifier`, `nonce`, `created_at`, `expires_at` (10 min); se consume una sola vez |
| `networks` | `id uuid pk` (= `network_id` del contrato), `org_id`, `name`, `ipv4_cidr cidr`, `ipv6_cidr cidr`, `root_anchor bytea(32)` (clave pública de la raíz, la fija el administrador), `issuer_cert bytea null` (178 bytes), `created_at`; `unique(org_id, name)`; los CIDR de redes de una misma organización no se solapan |
| `devices` | `id uuid pk` (= `device_id`), `network_id`, `name`, `status in ('pending','approved','revoked')`, `noise_pubkey bytea(32)`, `identity_pubkey bytea(32)`, `bundle bytea(146)`, `endpoints jsonb` (lista de `"ip:puerto"` indicada por el administrador), `enrolled_at`, `approved_at`, `revoked_at`, `last_config_at`; `unique(network_id, noise_pubkey)`; `unique(network_id, identity_pubkey)` |
| `ip_allocations` | `network_id`, `device_id`, `family smallint in (4,6)`, `addr inet`; `unique(network_id, addr)`, `unique(network_id, device_id, family)` |
| `enrollment_tokens` | `id uuid pk`, `network_id`, `token_hash bytea unique` (SHA-256; el token en claro solo se muestra al crearlo), `created_by`, `created_at`, `expires_at`, `auto_approve bool`, `consumed_at null`, `consumed_by_device null` |
| `access_policies` | `id uuid pk`, `network_id`, `src`, `dst` (cada uno `kind in ('any','device','prefix')` con `device_id`/`prefix` según el tipo), `proto smallint` (0, 1, 6, 17, 58), `port_min int`, `port_max int`, `created_by`, `created_at` |
| `configuration_versions` | `id bigserial`, `device_id`, `version bigint`, `config_hash bytea`, `not_before`, `not_after`, `issued_at`, `issuer_serial`; `unique(device_id, version)` |
| `audit_events` | `id bigserial`, `at`, `actor_kind in ('user','device','system')`, `actor_id text`, `org_id null`, `network_id null`, `action text`, `target text`, `detail jsonb`; solo inserciones (el rol de aplicación no tiene `UPDATE` ni `DELETE` sobre esta tabla) |

**IPAM.** Cada red tiene un CIDR IPv4 y uno IPv6. La asignación ocurre al **aprobar** un dispositivo, dentro de una transacción que bloquea la fila de la red (`SELECT … FOR UPDATE`): se toma la dirección libre más baja de cada familia (IPv4 sin dirección de red ni de difusión; IPv6 sin la dirección de red) y se insertan ambas asignaciones. Las restricciones únicas son la red de seguridad: ante una violación se reintenta una vez y después se devuelve `409`. Un dispositivo revocado libera sus direcciones. Tamaño máximo de red: 256 dispositivos aprobados (límite de pares del contrato); superarlo devuelve `409 network_full`.

## 3. Autenticación y autorización

### 3.1 Administradores: OIDC + sesión

- OIDC Authorization Code con **PKCE (S256)**, `state` y `nonce`. Validar firma del ID token contra las claves del proveedor (JWKS por discovery), `iss`, `aud` (= `client_id`), `exp`, `nonce`. Configuración: `ATENEA_CP_OIDC_ISSUER`, `..._CLIENT_ID`, `..._CLIENT_SECRET`, `..._REDIRECT_URL`. Las contraseñas no se almacenan; el MFA lo aplica el proveedor.
- `GET /auth/login` crea el flujo (guarda hash del `state`, verificador PKCE y `nonce`) y redirige. `GET /auth/callback` consume el flujo, intercambia el código, valida el ID token y crea o actualiza el usuario por (`iss`, `sub`). Crea la sesión y fija la cookie `atenea_session` (`HttpOnly`, `Secure` salvo en modo desarrollo explícito, `SameSite=Lax`, `Path=/`, duración 8 h absolutas, 1 h de inactividad).
- **CSRF:** toda petición con método distinto de `GET`/`HEAD`/`OPTIONS` que use sesión exige la cabecera `X-CSRF-Token` igual al valor devuelto por `GET /api/v1/session` (se compara su hash). Sin sesión válida: `401`. Sin CSRF válido: `403 csrf`.
- `POST /auth/logout` borra la sesión.

### 3.2 Roles (por organización)

| Acción | owner | admin | member |
| --- | --- | --- | --- |
| Ver organización, redes, dispositivos, políticas y auditoría | sí | sí | sí |
| Crear/eliminar redes, subir certificado de emisor, crear tokens, aprobar/revocar dispositivos, gestionar políticas | sí | sí | no |
| Gestionar miembros y roles; eliminar la organización | sí | no | no |

Toda ruta de una red o dispositivo comprueba que el usuario pertenece a la **organización propietaria**; una red ajena responde `404` (no `403`) para no revelar su existencia. Una organización debe conservar siempre al menos un `owner`. Cualquier usuario autenticado puede crear organizaciones (queda como `owner`).

### 3.3 Dispositivos: petición firmada

Los agentes se autentican con la clave de identidad Ed25519 del dispositivo (ya registrada en el enrolamiento). Cabecera:

```
Authorization: AtDevice v1 device=<hex 16 bytes>, ts=<unix s>, nonce=<hex 16 bytes>, sig=<hex 64 bytes>
```

`sig = Ed25519(identity_sk, "atenea-one/api-auth/v1\0" ‖ METHOD ‖ "\n" ‖ PATH_Y_QUERY ‖ "\n" ‖ ts ‖ "\n" ‖ nonce ‖ "\n" ‖ BLAKE2s-256(cuerpo))` (`METHOD` en mayúsculas; `ts` y `nonce` en el mismo texto que la cabecera). El servidor exige `|now − ts| ≤ 60 s`, rechaza un `nonce` repetido dentro de 120 s (caché en memoria; con varias instancias, tabla con TTL), busca el dispositivo y verifica con `verify_strict`. Un dispositivo `revoked` o `pending` autenticado recibe `403 device_not_approved` (no se emite configuración). Fallos de firma, desfase de reloj o repetición: `401 invalid_signature` (misma respuesta, sin distinguir la causa). La construcción y verificación viven en `atenea-identity` (`api_auth`), compartida por servidor y agente.

## 4. API REST

Prefijo `/api/v1`. Cuerpos JSON salvo indicación. Las rutas con `{org}`, `{net}`, `{dev}` usan UUID.

| Método y ruta | Acceso | Descripción |
| --- | --- | --- |
| `GET /healthz` | público | `200` si la base responde |
| `GET /auth/login`, `GET /auth/callback`, `POST /auth/logout` | público / sesión | Ver §3.1 |
| `GET /api/v1/session` | sesión | Usuario, token CSRF y membresías |
| `POST /api/v1/orgs` `{name}` | sesión | Crea la organización; el creador es `owner` |
| `GET /api/v1/orgs` | sesión | Organizaciones del usuario |
| `GET /api/v1/orgs/{org}/members`; `PUT …/members/{user}` `{role}`; `DELETE …/members/{user}` | member / owner / owner | Ver `PUT` también añade un usuario ya registrado (por `user` id) |
| `POST /api/v1/orgs/{org}/networks` `{name, ipv4_cidr, ipv6_cidr, root_anchor}` | admin | `root_anchor` en hex (32 bytes). Genera `network_id` |
| `GET /api/v1/orgs/{org}/networks`; `GET /api/v1/networks/{net}` | member | Lista y detalle (incluye si hay certificado de emisor) |
| `PUT /api/v1/networks/{net}/issuer-cert` (cuerpo binario 178 bytes) | admin | Valida que el certificado parsea, que su `network_id` coincide, que está firmado por `root_anchor` y que sigue vigente |
| `DELETE /api/v1/networks/{net}` | admin | Elimina la red y sus dispositivos (cascada) |
| `POST /api/v1/networks/{net}/enrollment-tokens` `{ttl_secs, auto_approve}` | admin | Devuelve `{token, expires_at, join}`; `join` agrupa `url`, `token`, `network_id` y `root_anchor`. `ttl_secs` entre 60 y 86400. El token (256 bits aleatorios, base64url) se devuelve solo una vez |
| `GET /api/v1/networks/{net}/devices` | member | Inventario |
| `POST /api/v1/networks/{net}/devices/{dev}/approve` | admin | Asigna IPv4+IPv6 en transacción; `409` si ya está aprobado o si la red está llena |
| `POST /api/v1/networks/{net}/devices/{dev}/revoke` | admin | Marca `revoked`, libera direcciones; el dispositivo deja de recibir configuración y desaparece de la de sus peers en la siguiente emisión |
| `PATCH /api/v1/networks/{net}/devices/{dev}` `{name?, endpoints?}` | admin | `endpoints`: lista de `ip:puerto` |
| `GET/POST /api/v1/networks/{net}/policies`; `DELETE …/policies/{id}` | member / admin / admin | Cada política se valida: selectores existentes en la red, prefijos dentro de los CIDR de la red, puertos coherentes (solo TCP/UDP, `port_min ≤ port_max`), como máximo 256 políticas |
| `GET /api/v1/networks/{net}/audit?limit=&before=` | member | Eventos de auditoría de la red, paginados |
| `POST /api/v1/enroll` `{token, bundle (hex, 146 bytes), name}` | token | Ver §5 |
| `GET /api/v1/device/config` | dispositivo (§3.3) | Ver §6 |

Las mutaciones generan `audit_events`: `org.create`, `member.set`, `network.create`, `network.issuer_cert`, `token.create`, `device.enroll`, `device.approve`, `device.revoke`, `device.update`, `policy.create` y `policy.delete`. Cada emisión firmada se registra en `configuration_versions`; `config.issue` conserva además versión y hash de la primera emisión por dispositivo en cada ventana de una hora. Repetir una revocación ya aplicada no genera otro evento.

## 5. Enrolamiento

1. Un administrador crea un token (`auto_approve` por defecto `false`) y entrega `join` al operador del dispositivo por un canal fuera de banda; **el `root_anchor` va en `join`** para que el agente lo fije como ancla de confianza (el servicio de enrolamiento no es la fuente de confianza de esa clave).
2. El dispositivo ejecuta `atenea device-init` (genera claves y `bundle.bin`) y `atenea enroll`, que hace `POST /api/v1/enroll`.
3. El servidor, **en una sola transacción**:
   - Consume el token con `UPDATE enrollment_tokens SET consumed_at = now() WHERE token_hash = $1 AND consumed_at IS NULL AND expires_at > now() RETURNING …` (dos peticiones simultáneas con el mismo token: una gana, la otra recibe `401 invalid_token`; token caducado, usado o inexistente: la misma respuesta).
   - Parsea el `DeviceBundle` y comprueba `verify()` (prueba de posesión de la clave de identidad). `device_id` lo trae el bundle; si ya existe, `409`.
   - Inserta el dispositivo en `pending` (o `approved` y asigna IPs, si el token tenía `auto_approve`). Una clave Noise o de identidad repetida en la red: `409` y el token **no** se consume (la transacción revierte).
   - Registra `device.enroll`.
4. Respuesta `201 {device_id, network_id, status}`.
5. Mientras esté `pending`, `GET /device/config` devuelve `403 device_not_approved`. Un administrador compara la **huella del bundle** (`atenea device-init` la imprime; el inventario la muestra) por otro canal antes de aprobar.

Límite de tasa de `/api/v1/enroll` por IP de origen (token bucket en memoria, configurable; por defecto 10 por minuto) para frenar la adivinación de tokens (256 bits: no es el control principal).
Los límites por IP usan la IP cliente: `X-Real-IP` solo si el peer TCP está en `trusted_proxies`; si no, el peer. `GET /auth/login` tiene límite análogo (`login_per_minute`, `429 rate_limited`) y tope de 10 000 flujos OIDC vigentes; `oidc_flows` y `sessions` caducados se purgan cada minuto. Las peticiones firmadas (`config`, `status`, `routes`) se limitan por dispositivo (`device_per_minute`, `429`) tras verificar la firma y antes de abrir transacción; la caché de nonces está acotada (65 536; llena → `429`). Un dispositivo no aprobado recibe `403` sin bloquear la fila de la red. `config.issue` se audita como máximo una vez por dispositivo y hora.

`POST …/networks` exige CIDR privados (RFC 1918, 100.64/10, ULA), IPv4 ≥ /8 e IPv6 ≥ /32 (`400 invalid_cidr`), reforzado por CHECK `NOT VALID` (migración 0004). Una red heredada fuera de rango no se firma (`409 invalid_network_prefix`). `PATCH …/devices/{dev}` valida `endpoints` con `sanitize_candidates` (sin loopback, multicast, no especificadas, enlace local ni puerto 0; `400 invalid_endpoints`); al cargar los pares para emitir también se sanean los datos heredados, con máximo cuatro endpoints. `POST …/enrollment-tokens` responde con `Cache-Control: no-store` y `X-Content-Type-Options: nosniff`. `revoke` es idempotente: repetirlo no reescribe `revoked_at` ni duplica auditoría.

## 6. Emisión de configuración

`GET /api/v1/device/config`, solo para dispositivos `approved`. El servidor construye y firma una `DeviceConfig` del contrato `signed-config-v1` para ese dispositivo en cada petición:

- `network_id`, `device_id`, claves y `tunnel_addrs` (las dos asignaciones), `network_prefixes` (los dos CIDR), `mtu` 1400.
- `issuer_serial` y la firma: de la clave del emisor del control plane y del certificado guardado en la red. Si la red no tiene certificado o este caduca antes del final de la validez pedida: `503 issuer_unavailable`; el `not_after` nunca supera el del certificado.
- `not_before = now`; `not_after = now + config_validity_secs` (por defecto **300**, ADR-002 piloto). `config_version`: milisegundos Unix, estrictamente mayor que la última emitida para ese dispositivo (`max(now_ms, última + 1)` dentro de la misma transacción que registra `configuration_versions`).
- **Pares:** dispositivos `approved` de la red que cumplen alguna política contra este dispositivo: para cada política en la que este dispositivo coincide como origen o destino, los dispositivos que coinciden con el otro selector (`any` = todos; prefijo = los que tienen una dirección dentro; dispositivo = ese). Sin política no hay pares. Máximo 256. Cada par lleva `endpoints` desde la columna del dispositivo.
- **Reglas:** por cada política relevante, la regla del contrato con selectores traducidos (`device` → `Device(id)`, `prefix` → `Prefix`, `any` → `Any`), más su **regla inversa** (misma `proto`, origen y destino intercambiados, sin puertos), porque las ACL del agente no tienen estado (contrato §8). Se eliminan duplicados; máximo 256 reglas (si se supera: `409 policy_overflow` al crear la política que lo provoca, comprobado con una emisión de prueba).
- Respuesta `200 application/octet-stream` con el fichero `SignedConfig` completo y cabeceras `X-Config-Version` y `Cache-Control: no-store`. Actualiza `last_config_at`.

**Revocación (ADR-002).** Revocar un dispositivo (o quitar una política) tiene efecto inmediato en el servidor: ya no recibe configuración y deja de figurar entre los pares de los demás en su siguiente emisión. Los peers dejan de aceptarlo cuando recargan (≤ 60 s de sondeo más la recarga); el propio agente revocado se corta solo al caducar su última configuración (≤ 300 + 30 s) aunque pierda el canal de control.

## 7. Configuración y operación

- Variables `ATENEA_CP_*` (Figment; también fichero TOML opcional): `DATABASE_URL`, `LISTEN`, `PUBLIC_URL`, `OIDC_*`, `ISSUER_KEY_FILE` (semilla hex 0600, mismo formato que `atenea issuer-init`), `CONFIG_VALIDITY_SECS` (300), `DEV_INSECURE_COOKIES` (desactiva `Secure` solo en desarrollo; por defecto falso y se registra un aviso al activarlo).
- Registro con `tracing`; no se registran tokens, cookies, claves ni cuerpos de configuración.
- `/healthz` y migraciones automáticas al arrancar con `--migrate` (no por defecto).
- El rol de base de datos de la aplicación no es propietario del esquema ni puede modificar `audit_events`.

## 8. Pruebas exigidas

Contra un PostgreSQL real (contenedor), sin simular la base:

- Dos organizaciones con redes y aislamiento: un usuario de A no ve ni modifica nada de B (`404`); un `member` no puede crear tokens, aprobar ni tocar políticas (`403`); no se puede dejar una organización sin `owner`.
- Token: caducado, ya usado, inexistente y **consumido en paralelo por N peticiones concurrentes** (exactamente una gana).
- IPAM: N aprobaciones concurrentes no duplican direcciones (restricción única comprobada y resultado sin repetidos); liberar al revocar; red llena.
- Enrolamiento con bundle de prueba de posesión inválida, clave repetida (token no consumido) y `device_id` repetido.
- Emisión: la configuración devuelta **la acepta `verify_config`** de `atenea-identity` con el estado y las claves del dispositivo; `config_version` estrictamente creciente; no incluye a dispositivos revocados ni pendientes; respeta las políticas y sus reglas inversas; red sin certificado de emisor: `503`; dispositivo revocado/pendiente: `403`; firma de petición inválida, `ts` fuera de ventana y `nonce` repetido: `401`.
- CSRF y sesión: sin cabecera CSRF `403`; sesión caducada `401`.
- OIDC: validación de `iss`, `aud`, `nonce` y `state`, y reutilización del flujo, contra el proveedor de desarrollo.
- Auditoría: cada escritura deja su evento; la tabla no admite `UPDATE`/`DELETE` desde la aplicación.
- Contrato OpenAPI generado (`docs/api/openapi.json`) y comprobado en CI contra el código.
- E2E (laboratorio): un agente se enrola, se aprueba, obtiene su configuración del control plane y se comunica con otro; al revocarlo, sus peers lo dejan de aceptar y él se corta al caducar el lease, aun si se corta la conexión con el control plane.

## 9. Fuera de alcance de la fase 3B

Panel web (fase 6), coordinator/STUN/relays y descubrimiento de endpoints (fases 4 y 5), alta disponibilidad del control plane (fase 9), rotación automatizada del certificado de emisor, invitaciones por correo y SSO por organización.

## Resoluciones de implementación

- La eliminación de organizaciones figura en RBAC pero faltaba en la tabla REST: se expone `DELETE /api/v1/orgs/{org}`, solo owner, con cascada y auditoría `org.delete`.
- El callback vincula además `state` al navegador mediante cookie temporal HttpOnly `atenea_oidc`, SameSite=Lax, Path=/auth/callback y duración 600 s; evita login CSRF.
- Para devolver el mismo CSRF en `/session`, se conserva también `sessions.csrf_token`. El token de sesión continúa almacenado exclusivamente como hash; CSRF no autentica sin esa sesión.
- `--migrate` usa la conexión administrativa, concede permisos al rol configurado y termina. El servicio normal usa otra conexión restringida y rechaza permisos UPDATE/DELETE/TRUNCATE sobre auditoría.
- Cada nueva versión firmada se registra en `configuration_versions`; el evento de auditoría `config.issue` se escribe en esa misma transacción como máximo una vez por dispositivo y hora (versión y hash de la primera de la ventana).
- Las pruebas del router no sustituyen el E2E con TUN Linux; este se ejecuta con `tests/network-lab/run-e2e.sh` (ver [resultados](../operations/e2e-fase3b.md)). No forma parte del CI porque necesita contenedores privilegiados.

## 10. Extensiones para el panel (fase 6)

Contrato compartido por backend (`apps/control-plane`, agente) y panel (`apps/web`). Todo cambio de forma se refleja en `docs/api/openapi.json` (generado) y el cliente TypeScript se regenera desde él; el panel no define modelos de API a mano.

### 10.1 Inventario ampliado

`GET /api/v1/networks/{net}/devices` añade a cada elemento: `ipv4` y `ipv6` (texto o `null` si no está aprobado), `enrolled_at`, `approved_at`, `revoked_at`, `last_config_at` (RFC 3339 o `null`) y `report` (`null` si el agente nunca informó):

```json
{"report": {"reported_at": "…", "age_secs": 12, "nat_mapping": "independent|dependent|unknown|null",
  "paths": [{"peer": "<uuid>", "path": "direct|relay-udp|relay-tls|probing|none", "last_rx_secs": 3, "rtt_ms": 20, "relay": "lab"}]}}
```

`age_secs` lo calcula el servidor con su reloj (el panel no compara relojes). `rtt_ms` y `relay` pueden ser `null`. Un informe es **declarado por el agente** (autenticado como ese dispositivo, no verificado por terceros): el panel lo presenta como tal, con su antigüedad. Conexión mostrada: `online` (< 90 s), `stale` (< 10 min), `offline` (más), `unknown` (sin informe).

### 10.2 Informe de estado del dispositivo

`POST /api/v1/device/status` (autenticación de dispositivo §3.3; solo `approved`). Cuerpo ≤ 64 KiB: `{"nat_mapping"?: …, "paths": [{"peer","path","last_rx_secs"?,"rtt_ms"?,"relay"?}]}`, como máximo 256 caminos, `peer` UUID de un dispositivo de la misma red, `path` en el conjunto de §10.1; se rechaza con `400` cualquier otra cosa. Reemplaza el último informe del dispositivo (tabla `device_reports`, uno por dispositivo) y responde `204`. Máximo un informe cada 5 s por dispositivo (`429`). No genera evento de auditoría (alta frecuencia, sin cambio de autorización). Notifica `changed/devices` por WebSocket.

El agente informa cada 15 s mientras tenga `[sync]` configurado y configuración vigente; no incluye IPs externas ni claves.

### 10.3 Eventos en vivo (WebSocket)

`GET /api/v1/networks/{net}/events` con `Upgrade: websocket`, autenticado por la cookie de sesión (member o superior; red ajena `404`). Se exige que `Origin` coincida con el origen de `PUBLIC_URL` (`403` si no), por ser una petición con cookies sin CSRF. Mensajes JSON del servidor, uno por trama de texto:

- `{"type":"hello","seq":N}` al conectar.
- `{"type":"changed","scope":"devices|policies|audit|network|metrics","seq":N}` cuando cambia ese recurso. `seq` crece por proceso; el panel **no reconstruye estado a partir de eventos**: ante cualquier evento, o al reconectar, vuelve a pedir el recurso por REST (resincronización). Ping del servidor cada 25 s; el cliente reconecta con retardo exponencial y jitter. Máximo 8 conexiones por usuario y 64 por proceso (`429`). Un solo proceso: la difusión es en memoria (con varias instancias hará falta LISTEN/NOTIFY o equivalente, fase 9).

### 10.4 Métricas

`GET /api/v1/networks/{net}/metrics?since=<RFC 3339>&limit=` (member): serie real muestreada por el servidor cada 60 s a partir de los últimos informes: `[{"at","total","direct","relay","probing","unknown","offline"}]` (dispositivos aprobados clasificados por su camino predominante y conexión). Tabla `network_samples`, retención 7 días, `limit` ≤ 1000 (por defecto 360). Sin datos no se inventan puntos: lista vacía.

### 10.5 Contenido estático

Con `ATENEA_CP_WEB_DIR` (directorio con el `build/client` del panel) el control plane sirve el panel desde el mismo origen (cookies `SameSite=Lax` y CSRF sin CORS). Rutas que no empiezan por `/api`, `/auth` ni `/healthz` y no son un fichero existente devuelven `index.html` (SPA). Cabeceras en todas las respuestas de contenido: `Content-Security-Policy: default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`, `Cache-Control: no-cache` para `index.html` y `immutable` para ficheros con hash.

### 10.6 Fuera de alcance de la fase 6

Editor Monaco, creación de claves o certificados desde el panel (la raíz y el emisor siguen offline, ADR-005), invitaciones por correo, varias instancias del control plane.

### Resoluciones

- `connection` es un campo explícito del inventario, calculado en el servidor; `age_secs` es el número entero de segundos transcurridos, truncado y nunca negativo. Las fechas opcionales y las asignaciones ausentes son `null`.
- Los informes rechazan campos desconocidos, tipos inválidos, números negativos y cuerpos mayores de 64 KiB con `400 invalid_report`; la limitación de 5 s usa el reloj PostgreSQL y bloqueo del dispositivo para que dos peticiones simultáneas no la eludan. `relay` admite hasta 255 bytes sin caracteres de control.
- El camino predominante de una muestra es la categoría más frecuente entre los caminos del informe (`relay-udp` y `relay-tls` se agrupan en `relay`; `none` en `unknown`). Los empates se resuelven por orden alfabético de categoría. Sin informe es `unknown`; desde 600 s es `offline`; los informes `stale` conservan su categoría hasta ese umbral. Solo cuentan dispositivos aprobados. Se muestrea también al arrancar y se eliminan muestras anteriores a siete días en la misma transacción. `limit` exige 1..1000, y se devuelven las últimas muestras solicitadas en orden temporal ascendente.
- No hay catálogo ni endpoint nuevo de relays: el panel obtiene los relays observados de `report.paths[].relay`. El agente incluye el nombre declarado por su cliente relay cuando conoce la asignación y coincide con el transporte activo; si no lo conoce, devuelve `null`. No envía direcciones de transporte ni claves.
- Los eventos se publican tras confirmar las escrituras. La secuencia es global por proceso; cada conexión solo recibe eventos de su red. Se revalida sesión y pertenencia antes de enviar eventos y cada ping. Un consumidor que pierde mensajes por saturación se desconecta para forzar resincronización REST. Los límites de conexiones se liberan también al fallar el upgrade o desconectarse.
- El servicio estático conserva el `404` JSON para los prefijos reservados, no sigue enlaces simbólicos fuera del directorio configurado y reconoce los assets Vite con sufijo `-hash` (al menos ocho caracteres alfanuméricos o `_`) para su caché inmutable. El resto, incluido `index.html`, usa `no-cache`. Sin `WEB_DIR`, no se sirve contenido.

## 11. Rutas de subred y DNS (fase 7)

Las decisiones de seguridad son las de [ADR-007](../architecture/ADR-007-subredes-dns-privilegios.md); el cuerpo firmado se rige por [signed-config-v2](../protocol/signed-config-v2.md). Sin rutas ni DNS se conserva la codificación v1 byte a byte.

| Método y ruta | Acceso | Contrato |
| --- | --- | --- |
| `POST /api/v1/device/routes` | dispositivo aprobado, firma §3.3 | `{"routes":["192.168.50.0/24"]}`; reemplaza la declaración completa, máximo 8 prefijos; `204` |
| `GET /api/v1/networks/{net}/routes` | member | Lista `{id,device_id,prefix,status,snat,declared_at,decided_at}` |
| `POST /api/v1/networks/{net}/routes/{id}/approve` | admin | `{"snat":true}`; booleano obligatorio sin valor por defecto; `204` |
| `POST /api/v1/networks/{net}/routes/{id}/reject` | admin | Rechaza una pendiente o revoca una aprobada; `204` |
| `GET /api/v1/networks/{net}/dns` | member | `{"servers":["10.77.0.1"],"domains":["lab.internal"]}` |
| `PUT /api/v1/networks/{net}/dns` | admin | Reemplaza ambos vectores con el mismo formato; `204` |

### 11.1 Declaración y aprobación

Solo se aceptan CIDR canónicos completamente contenidos en RFC 1918, `100.64.0.0/10` o ULA `fc00::/7`, con longitud mínima /8 en IPv4 y /32 en IPv6. No se admiten prefijos públicos, rutas por defecto, bits de host, duplicados ni solapes dentro de una declaración (`400 invalid_route` o `route_overlap`). Las rutas nuevas entran en `pending`; las que permanecen conservan su decisión y las retiradas se borran. Una ruta rechazada no se vuelve pendiente por repetir la declaración: hay que retirarla y declararla de nuevo, o aprobarla explícitamente.

Máximo una declaración aceptada cada 5 s por dispositivo (`429 rate_limited`), medido por PostgreSQL bajo bloqueo; las declaraciones inválidas no consumen el intervalo. Al aprobar se bloquea la fila de red, igual que en IPAM, y se comprueba el solape con ambos CIDR del túnel y con todas las otras rutas aprobadas de la red: `409 route_overlap`. La comprobación y la decisión se confirman en la misma transacción; dos aprobaciones simultáneas no pueden eludirla. Los solapes entre declaraciones de dispositivos distintos son posibles mientras no estén aprobadas.

La tabla `device_routes` conserva `network_id`, `device_id`, `prefix cidr`, estado (`pending|approved|rejected`), `snat bool|null`, `declared_at`, `decided_at` y `decided_by`; `(device_id,prefix)` es único. `snat` solo tiene valor en una aprobada. Revocar un dispositivo rechaza también sus rutas y libera su autorización para futuras aprobaciones. RBAC y ocultación de organizaciones ajenas son los de §3 (`403` member en escrituras; `404` para redes o rutas ajenas).

### 11.2 DNS y emisión

`servers` admite hasta 3 direcciones únicas dentro de los CIDR del túnel o de rutas aprobadas de dispositivos aprobados. `domains` admite hasta 8 nombres únicos en ASCII minúsculo, etiquetas LDH de 1–63 caracteres sin guiones en los extremos, longitud total ≤253, sin punto final, etiquetas vacías ni dominio raíz. Datos inválidos: `400 invalid_dns`. Vectores vacíos eliminan la configuración DNS; no hay dominios ni servidores implícitos.

Las políticas pueden seleccionar un prefijo contenido en una ruta aprobada: seleccionan al dispositivo que la sirve. La emisión solo añade `peers[].routes` de pares ya seleccionados por políticas y `advertised` del propio dispositivo con el bit SNAT aprobado. Si se rechaza o retira una ruta, una política cuyo prefijo queda huérfano se conserva en la base, pero no selecciona pares ni emite reglas: **la emisión no falla**. También se omiten las reglas que dependan de dispositivos revocados.

Cada configuración incluye los dominios de la red y únicamente los servidores alcanzables dentro de sus propios `network_prefixes`, `peers[].routes` o `advertised`. Esto omite servidores que dejan de estar autorizados tras una revocación y los de routers que la política no selecciona; nunca se añaden pares solo para resolver DNS. Si todos los campos nuevos quedan vacíos el cuerpo vuelve a v1; en otro caso es v2.

### 11.3 Estado, auditoría y eventos

`POST /api/v1/device/status` añade campos opcionales `forwarding_ok: bool|null`, `dns: "applied"|"unsupported"|"none"|null` y `routes: [{"prefix":"192.168.50.0/24","state":"installed|conflict|pending","reason":"motivo opcional"}]|null`. El inventario los devuelve dentro de `report`, sin reinterpretar el estado declarado por el agente. Se mantienen límites y semántica de reemplazo de §10.2; los campos ausentes quedan en `null`. Hasta 256 rutas de estado, CIDR privados canónicos válidos y motivos de hasta 255 bytes sin controles; campos o estados desconocidos dan `400 invalid_report`.

Se registran `route.declare`, `route.approve`, `route.reject` y `dns.set` en la transacción correspondiente. Tras confirmar, el WebSocket emite `changed` con scope `routes` o `dns`, además de `audit`; el consumidor vuelve a consultar REST. Los informes siguen emitiendo `changed/devices` sin generar auditoría de alta frecuencia.
