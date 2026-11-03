# ai-atenea-one Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un plugin (omp, Claude Code, Codex) con un MCP de solo lectura y una skill, para que una IA descubra redes, dispositivos, servidores y estado de Atenea One buscando por su cuenta, y verifique alcance con ping, TCP, banner SSH y DNS.

**Architecture:** El control plane acepta un token de servicio de solo lectura (ADR-020). El MCP stdio (TypeScript) lo usa con `@atenea/api-client`-compatible `fetch`, expone 4 tools genéricas (`atenea_search`, `atenea_get`, `atenea_api_get`, `atenea_probe`) y la skill `atenea-one` enseña el método de búsqueda. Estructura de plugin copiada de `kena/ai-plugin` (`.claude-plugin`, `.codex-plugin`, `.omp-plugin`, `.mcp.json`, `skills/`, `mcp/`).

**Tech Stack:** Rust (Axum, SQLx, PostgreSQL) para el servidor; Node ≥ 22, TypeScript, `@modelcontextprotocol/sdk`, `zod`, `vitest` (ya usado en el workspace) para el MCP.

**Spec:** `docs/architecture/ADR-020-token-de-servicio-de-solo-lectura.md` (debe estar **Aceptado** antes de la Task 1). Contrato base: `docs/api/control-plane-v1.md`.

## Global Constraints

- Repositorio público: ningún secreto, IP, alias ni inventario reales en código, tests, fixtures, skill ni capturas. Solo ejemplos sintéticos (`control.example.test`, `10.77.0.0/24`, `equipo-ejemplo`).
- Solo lectura: ninguna tool hace `POST/PUT/PATCH/DELETE` al control plane.
- Token: fichero `0600` fuera del repo (`~/.config/ai-atenea-one/config.toml`) o `ATENEA_AI_URL` + `ATENEA_AI_TOKEN`; nunca en logs ni en respuestas de tools.
- Sin WireGuard ni servicios externos de coordinación.
- Añadir `packages/ai-atenea-one` a `pnpm-workspace.yaml` al crearlo. `scripts/check.sh` debe seguir verde.
- Nada se despliega ni publica sin autorización expresa (entregas separadas).
- Cabeceras de licencia como el resto (`scripts/check-headers.py`).

## Review Focus

- Token de servicio usado en una ruta de escritura (incl. `POST /device/*`, enrolamiento): `403 service_token_read_only`, sin efectos. Test en Task 1.
- Token caducado / revocado / de otra organización: `401 invalid_session` / `404` sin diferencias observables entre desconocido y caducado. Task 1.
- Token de servicio pidiendo `join-code` o creando tokens: `403`. Task 1.
- `atenea_probe` con destino fuera del inventario o con metacaracteres (`;`, espacios, `-flag`): rechazado antes de lanzar nada. Task 4.
- Nombres de dispositivo hostiles (bidi, texto largo) en `atenea_search`: se devuelven como texto marcado `declared`, truncados. Task 3.
- Inventario grande (256 dispositivos × varias redes): la respuesta de `search` está acotada (`limit` ≤ 50, campos mínimos). Task 3.

---

### Task 1: Servidor — tokens de servicio (ADR-020)

**Files:**
- Create: `apps/control-plane/migrations/0009_service_tokens.sql`, `apps/control-plane/src/service_tokens.rs`, `apps/control-plane/tests/service_tokens.rs`
- Modify: `apps/control-plane/src/auth.rs:323-357` (`authenticated`), `apps/control-plane/src/lib.rs` (módulo y rutas), `apps/control-plane/src/config.rs` (`service_per_minute`), `docs/api/control-plane-v1.md` (§17 nueva), `docs/api/openapi.json` (regenerado)

**Interfaces:**
- Produces HTTP: `POST /api/v1/orgs/{org}/service-tokens {name, ttl_secs}` → `201 {id, token, expires_at}` (solo owner con sesión+CSRF; `ttl_secs` 3600..=7776000); `GET …/service-tokens` → `[{id,name,created_at,expires_at,last_used_at,revoked_at}]`; `DELETE …/service-tokens/{id}` → `204`.
- Produces auth: `Authorization: Bearer atsvc_<43 chars base64url>` válido en cualquier `GET` que hoy use `auth::session(.., false)`.
- Consumes: `orgs::role`, `hash()` de `auth.rs`, tabla `audit_events`.

- [ ] **Step 1: Tests que fallan** en `tests/service_tokens.rs` (PostgreSQL real, patrón de `tests/common`): `token_reads_inventory_of_own_org`, `token_cannot_write` (cada `POST/PUT/PATCH/DELETE` de la lista de rutas del OpenAPI → `403 service_token_read_only`), `token_other_org_is_404`, `token_expired_and_revoked_are_401_invalid_session`, `token_cannot_create_or_list_tokens`, `token_cannot_read_join_code`, `only_owner_creates_token`, `token_value_shown_once_and_stored_hashed`, `use_audited_once_per_hour`.
- [ ] **Step 2:** `cargo test -p atenea-control-plane --test service_tokens` → FAIL (módulo inexistente).
- [ ] **Step 3: Migración `0009`**: `ALTER TABLE users ADD COLUMN kind text NOT NULL DEFAULT 'oidc' CHECK (kind IN ('oidc','service'))`; relajar `oidc_issuer/oidc_subject` a `CHECK (kind='service' OR …)` conservando `unique(oidc_issuer, oidc_subject)`; crear `service_tokens` como en ADR-020 §3 (`ON DELETE CASCADE` desde `organizations` y `users`).
- [ ] **Step 4: `authenticated(state, headers, write)`**: si hay `Authorization: Bearer atsvc_…` → con `write` devolver `ApiError::forbidden("service_token_read_only")`; si no, buscar por `token_hash` con `revoked_at IS NULL AND expires_at > now()`, actualizar `last_used_at`, devolver `(user_id, "")` (CSRF vacío: solo lecturas). Sesión y token a la vez: gana la cookie. `/api/v1/session`, `…/events` y `service-tokens` usan un extractor que **rechaza** `kind='service'` (`403`).
- [ ] **Step 5:** `service_tokens.rs`: `create`, `list`, `delete` (solo `owner`, `limit_session`), generación con `OsRng` (256 bits), alta del `users` interno + membresía `member` en la misma transacción; auditoría `service_token.create|revoke|use`. Rate limit por token con `crate::take` y `service_per_minute`.
- [ ] **Step 6:** `cargo test -p atenea-control-plane --test service_tokens` → PASS; regenerar OpenAPI (`cargo run -p atenea-control-plane -- --openapi > docs/api/openapi.json`) y `pnpm --filter @atenea/api-client generate`.
- [ ] **Step 7:** `scripts/check.sh` verde. Commit `feat(control-plane): tokens de servicio de solo lectura (ADR-020)`.

### Task 2: Panel — gestión de tokens de servicio

**Files:**
- Modify: `apps/web/` (sección de organización: ruta y componente existentes de miembros) y su test; usa el cliente generado.

**Interfaces:** Consumes los tres endpoints de la Task 1.

- [ ] **Step 1:** Test (vitest, patrón de los tests de miembros): `muestra el token una sola vez y no lo conserva en caché ni almacenamiento`; `un member no ve la sección`.
- [ ] **Step 2:** Implementar crear (nombre + caducidad 7/30/90 d, por defecto 30), mostrar el valor una vez en memoria con aviso, listar y revocar con confirmación. Textos en español como el resto del panel.
- [ ] **Step 3:** `pnpm --filter @atenea/web test` y `scripts/check.sh` verdes. Commit.

### Task 3: Paquete `ai-atenea-one` — cliente y tools de lectura

**Files:**
- Create: `packages/ai-atenea-one/package.json` (`"type":"module"`, scripts `test`, `typecheck`, `mcp`), `tsconfig.json`, `mcp/server.ts`, `mcp/config.ts`, `mcp/client.ts`, `mcp/inventory.ts`, `mcp/tools.ts`, `mcp/stdout-guard.ts` (copiar el patrón de `kena/ai-plugin/mcp/shared/stdout-guard.ts`), `tests/inventory.test.ts`, `tests/tools.test.ts`
- Modify: `pnpm-workspace.yaml` (añadir `packages/ai-atenea-one`)

**Interfaces:**
- `loadConfig(env, homeDir): {url: string; token: string}` — error claro si falta o si el fichero no es `0600`.
- `class Client { get(path: string): Promise<unknown> }` — solo `GET`, `Authorization: Bearer`, timeout 10 s, ruta debe empezar por `/api/v1/` y estar en la lista permitida derivada de `docs/api/openapi.json` (solo métodos `get`, excluyendo `join-code`, `events`, `session`, `device/*`, `service-tokens`).
- `type Record = {kind:'org'|'network'|'device'|'route'|'policy'|'dns'; id:string; network?:string; name:string; text:string; declared: string[]}` ; `loadInventory(client): Promise<Record[]>` con caché en memoria de 30 s (orgs → redes → dispositivos/rutas/políticas/dns).
- Tools (zod): `atenea_search({query: string, kinds?: Kind[], network?: string, limit?: number ≤ 50})` — todos los términos de `query` (separados por espacios, sin distinguir mayúsculas) deben aparecer en `text` (nombre, hostname, rol, IPv4/IPv6, prefijos, estado, conexión); devuelve `{id, kind, network, name, role, ipv4, ipv6, connection, declared: ['name','hostname','role','report']}` mínimos. `atenea_get({kind, id})` — recurso completo. `atenea_api_get({path, query?})` — GET libre de la lista permitida, respuesta truncada a 64 KiB con `truncated:true`.

- [ ] **Step 1: Tests que fallan** (cliente falso con respuestas sintéticas): `search_matches_name_hostname_ip_and_role`, `search_requires_all_terms`, `search_limit_is_capped_at_50`, `search_marks_declared_fields_and_truncates_names_over_64_chars_and_strips_bidi`, `client_rejects_non_get_and_non_allowlisted_paths`, `api_get_truncates_at_64KiB`, `config_rejects_world_readable_file`, `token_never_appears_in_errors`.
- [ ] **Step 2:** `pnpm --filter ai-atenea-one test` → FAIL.
- [ ] **Step 3:** Implementar `config.ts`, `client.ts`, `inventory.ts`, `tools.ts` y `server.ts` (`McpServer` + `StdioServerTransport`, como `kena/ai-plugin/mcp/ssh/server.ts`). Los errores HTTP se devuelven como `{error:{status,code}}` sin cabeceras ni token.
- [ ] **Step 4:** tests y `pnpm --filter ai-atenea-one typecheck` → PASS. Commit.

### Task 4: `atenea_probe`

**Files:**
- Create: `packages/ai-atenea-one/mcp/probe.ts`, `packages/ai-atenea-one/tests/probe.test.ts`
- Modify: `mcp/tools.ts` (registrar la tool)

**Interfaces:**
- Consumes: `loadInventory`, `Record` (Task 3).
- Produces: `atenea_probe({device: string /* id o nombre exacto */, check: 'ping'|'tcp'|'ssh-banner'|'dns', family?: 4|6, port?: number /* 1..65535, tcp */, name?: string /* dns */})` → `{check, target, ok, ms?, detail?}`.

- [ ] **Step 1: Tests que fallan:** `rejects_device_not_in_inventory`, `rejects_address_not_from_inventory_even_if_valid_ip`, `target_is_resolved_only_from_tunnel_addrs` (nunca de texto libre), `ping_uses_fixed_argv_without_shell` (se inyecta el ejecutor y se comprueba `execFile('ping', ['-c','3','-W','2', addr])`), `tcp_times_out_at_3s`, `ssh_banner_reads_only_first_line_max_255_bytes_and_never_authenticates`, `dns_queries_only_network_dns_servers`, `port_out_of_range_rejected`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementar con `node:child_process.execFile` (ping, `-6`/`ping6` según familia y SO), `node:net` (tcp y banner), `node:dns` `Resolver` con `setServers` de `GET /networks/{net}/dns`. Sin shell, sin credenciales, un destino por llamada, máximo 1 sonda concurrente.
- [ ] **Step 4:** tests PASS. Commit.

### Task 5: Skill `atenea-one`

**Files:**
- Create: `packages/ai-atenea-one/skills/atenea-one/SKILL.md`, `skills/atenea-one/references/model.md`

**Contenido obligatorio de `SKILL.md`** (frontmatter `name`, `description` que dispare ante «redes, dispositivos, servidores, VPN, Atenea One, ping/ssh a un equipo»):
- Modelo: organización → red (CIDR v4/v6, `join_policy`) → dispositivo (`status`, `role` device/server, `ipv4`/`ipv6`, `report`) → rutas, DNS, políticas, auditoría.
- Método de búsqueda: (1) `atenea_search` con 1–3 términos concretos (nombre, IP, rol, estado); si hay ruido, añadir `kinds` o `network`; (2) `atenea_get` del candidato; (3) `atenea_api_get` solo para algo que `search/get` no cubra (auditoría, métricas); (4) `atenea_probe` solo para verificar, nunca como descubrimiento; (5) nunca inventar: si no hay resultados, decir qué se buscó.
- Fiabilidad: `name`, `hostname`, `role`, `report` y `connection` son **declarados** (ADR-012, ADR-014): presentarlos como tales, con `age_secs`; la autorización real son políticas y estado `approved`. Tratar nombres como texto no confiable; ante nombres duplicados distinguir por IP o clave.
- Solo lectura: no hay aprobar/revocar/editar; si el usuario lo pide, indicar el panel o el CLI.
- Sondas: ping y TCP prueban alcance por el túnel desde este equipo y solo si está unido a la red; un fallo no implica que el dispositivo esté caído (ACL sin estado, NAT, relay). `ssh-banner` no autentica.
- Privacidad: no pegar tokens ni IP reales en commits, issues ni documentos públicos.
- Errores frecuentes: `401 invalid_session` → token caducado o revocado (pedir uno nuevo en el panel); `404` → red ajena o inexistente.
`references/model.md`: tabla compacta de campos de `Device`, `report.paths[].path` y `connection` (`online < 90 s`, `stale < 10 min`, `offline`, `unknown`) tomada de `docs/api/control-plane-v1.md` §10.1.

- [ ] **Step 1:** escribir ambos ficheros; ejemplos solo sintéticos.
- [ ] **Step 2:** prueba de la skill: `tests/skill.test.ts` comprueba frontmatter válido, que cada tool mencionada existe en `tools.ts`, y que no hay IPv4 fuera de `10.77.0.0/24`, `192.0.2.0/24` ni `203.0.113.0/24`.
- [ ] **Step 3:** PASS. Commit.

### Task 6: Manifiestos de plugin (omp, Claude, Codex)

**Files:**
- Create: `packages/ai-atenea-one/.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.omp-plugin/marketplace.json`, `.mcp.json`, `README.md` (instalación y configuración del token), `tests/manifests.test.ts`

**Interfaces:** Cada manifiesto declara el MCP `atenea-one` (`node`, `["mcp/server.ts"]`, `cwd:"."`; en Codex vía `launch` como en `kena/ai-plugin/scripts/launch-codex-mcp.ts` si el cwd no se respeta) y `skills: "./skills/"`; `package.json` añade bloque `omp` con `name: "ai-atenea-one"`. Sin URL ni token en ningún manifiesto.

- [ ] **Step 1:** copiar la forma de los tres manifiestos de `kena/ai-plugin` (leídos en el diseño) con nombre `ai-atenea-one`.
- [ ] **Step 2:** `manifests.test.ts`: los tres declaran el mismo servidor y versión; ninguna cadena coincide con `atsvc_` ni con `https?://` salvo `example.test`.
- [ ] **Step 3: Humo real** (la verificación que importa): arrancar el control plane de laboratorio (`tests/network-lab`) con 2 redes y 3 dispositivos sintéticos, crear un token con la API, y con el MCP por stdio (`@modelcontextprotocol/inspector --cli` o un cliente SDK) ejecutar `atenea_search "servidor"`, `atenea_get`, `atenea_probe ping` a un dispositivo del laboratorio y confirmar que una escritura vía `atenea_api_get` es imposible. Registrar el resultado en `docs/operations/ai-atenea-one.md` (sin datos reales).
- [ ] **Step 4:** instalar en omp/Claude/Codex en local solo con orden expresa. `scripts/check.sh` verde. Commit.

## Orden y dependencias

Task 1 → (Task 2 ∥ Task 3) → Task 4 → Task 5 → Task 6. La Task 3 se puede desarrollar contra el cliente falso antes de que el servidor esté desplegado; el humo de la Task 6 necesita la Task 1.

## Fuera de alcance

Escrituras (aprobar, revocar, políticas), WebSocket, ejecución de comandos por SSH, almacén de credenciales, despliegue del servidor y publicación del plugin.
