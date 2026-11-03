<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-020: Token de servicio de solo lectura para asistentes de IA (`ai-atenea-one`)

- **Estado:** **Aceptado** por el responsable el 2026-11-03; **implementado** en el repositorio (servidor, panel y plugin `packages/ai-atenea-one`), sin desplegar. Plan: `docs/superpowers/plans/2026-11-03-ai-atenea-one.md`.
- **Relacionados:** `docs/api/control-plane-v1.md` §3 (autenticación y roles), ADR-009 (secretos), ADR-012 y ADR-014 (nombres y roles sin firmar), ADR-015 (privacidad de miembros).

## Contexto

Un MCP local para asistentes de IA necesita leer redes, dispositivos, servidores, rutas, DNS, políticas y estado. Esos `GET` exigen hoy sesión OIDC con cookie (`auth::session`); un proceso sin navegador no puede obtenerla. La firma de dispositivo (§3.3) solo da `config` y `peers` de la propia red.

Todas las rutas de lectura de administración pasan por `auth::session(state, headers, write=false)` y después comprueban pertenencia con `orgs::role` o `networks::access`. Ese es el único punto donde cambia la identidad.

## Decisión propuesta

1. **Un token de servicio es una credencial de lectura ligada a una organización.** Se materializa como un `users` interno (sin `oidc_issuer`/`oidc_subject` reales; `kind = 'service'`) con una membresía `member` en esa única organización. `member` ya es solo lectura en el RBAC de §3.2: **no se añade ningún rol nuevo** y el aislamiento entre organizaciones (404) se hereda sin código nuevo.
2. **Solo `GET`.** `authenticated()` acepta `Authorization: Bearer atsvc_<256 bits base64url>` únicamente cuando `write=false`. Con `write=true` el token se rechaza con `403 service_token_read_only`, antes de tocar la base. Los extractores de escritura no cambian.
3. **Almacenamiento.** Tabla `service_tokens(id uuid pk, user_id, org_id, name, token_hash bytea unique, created_by, created_at, expires_at not null, last_used_at, revoked_at)`. Solo el SHA-256 del token; el valor en claro se muestra una vez al crearlo. Caducidad obligatoria, máximo 90 días.
4. **Gestión solo por sesión de owner** (con CSRF): `POST /api/v1/orgs/{org}/service-tokens {name, ttl_secs}`, `GET …` (lista sin secretos), `DELETE …/{id}`. Un token de servicio **no puede crear ni listar tokens**. Se audita `service_token.create`, `service_token.revoke` y, como máximo una vez por token y hora, `service_token.use` (sin cuerpo ni cabeceras).
5. **Alcance de lectura = el de `member`.** Ve redes, dispositivos, rutas, DNS, políticas, auditoría, métricas e información del servidor de su organización. **No** ve el código de unión (admin), tokens de enrolamiento ni claves privadas (el control plane no las tiene). Los eventos WebSocket y `GET /api/v1/session` quedan fuera: se rechazan con el token.
6. **Límites.** Tasa por token (`service_per_minute`, por defecto 120, `429`); el límite se aplica tras autenticar. Revocar es inmediato (consulta por petición, sin caché de validez).
7. **Fallos indistinguibles.** Token desconocido, caducado o revocado: `401 invalid_session`, igual que una sesión inválida.
8. **Datos no autoritativos.** `name`, `hostname`, `role` y `report` los declara el agente o el administrador y no están firmados (ADR-012, ADR-014). El cliente de IA debe presentarlos como tales y nunca como decisión de autorización.

## Fuera del cliente (MCP), no del servidor

- El token vive en el ordenador del usuario, en un fichero `0600` fuera del repositorio (`~/.config/ai-atenea-one/config.toml`) o en una variable de entorno. Nunca en el plugin, el repositorio ni los logs.
- El MCP no tiene herramientas de escritura. Las sondas de red (`ping`, TCP, banner SSH, DNS) solo se lanzan contra destinos que salen del inventario consultado, con comandos fijos y tiempo máximo; no hay ejecución de comandos arbitrarios ni credenciales SSH.

## Alternativas descartadas

- **Rol `viewer` nuevo:** duplica `member` y obliga a revisar cada `orgs::role`.
- **Reutilizar la cookie de sesión del navegador:** frágil, de 8 h, con permisos del usuario completo y CSRF.
- **Leer el estado local del CLI:** solo la red del propio equipo; no sirve para «qué redes y servidores hay».
- **Firma de dispositivo para lectura de administración:** un dispositivo comprometido leería el inventario completo.

## Consecuencias

- Cambia `apps/control-plane` (migración `0009`, `auth.rs`, módulo `service_tokens`, OpenAPI, cliente generado) y el panel (sección para crear/revocar). Es un cambio de servidor; **no despliega nada** ni autoriza un despliegue.
- Riesgo residual: quien tenga el token lee el inventario de la organización hasta que caduque o se revoque (nombres, IPs de túnel, topología). Mitigación: caducidad corta por defecto (30 días), revocación inmediata, auditoría de uso.
- Pendiente de comprobar al implementar: que `member` ya oculta correos de otros miembros (ADR-015) también para un usuario `kind='service'`, y que ninguna ruta `GET` de administración llame a `authenticated` con `write=false` para devolver secretos.

## Resoluciones de implementación

- El rechazo de escrituras es un middleware del router (`read_only_token`): las rutas firmadas `/device/*` y `/enroll` no pasan por `authenticated()`.
- `PUT …/members/{user}` rechaza a un usuario `kind='service'`: sin esa guarda un owner podría ascenderlo a `admin` y leer `join-code`.
- `events` y `session` exigen la cookie de sesión (`auth::browser_only`).
- El plugin no expone `GET /orgs/{org}/members` a `atenea_api_get`: no aporta al inventario de red.
- Verificado con un control plane real y datos sintéticos: `docs/operations/ai-atenea-one.md`.
