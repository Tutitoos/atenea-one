# ai-atenea-one: verificación y operación

Plugin de solo lectura (`packages/ai-atenea-one`, [ADR-020](../architecture/ADR-020-token-de-servicio-de-solo-lectura.md)). Todos los datos de este documento son sintéticos.

## Verificación automática

- `pnpm --filter ai-atenea-one test`: cliente, configuración, inventario, tools, sondas, skill, manifiestos y el humo por stdio.
- `pnpm --filter ai-atenea-one typecheck`.
- Las dependencias nuevas constan en [dependencies.md](../architecture/dependencies.md).

## Humo por stdio contra un control plane falso (hecho)

`tests/stdio.test.ts` arranca el MCP real (`node mcp/server.ts`) con un cliente del SDK MCP sobre stdio y un servidor HTTP local que imita el contrato (datos sintéticos de `tests/fixtures.ts`, token `atsvc_…` ficticio). Comprueba:

- `tools/list` devuelve exactamente `atenea_search`, `atenea_get`, `atenea_api_get` y `atenea_probe`.
- `atenea_search` («servidor») y `atenea_get` leen el inventario.
- `atenea_probe` rechaza destinos fuera del inventario (IP válida no inventariada, nombre inexistente, metacaracteres).
- `atenea_api_get` no puede alcanzar `service-tokens`, `join-code` ni `device/*`; el servidor falso solo recibió `GET` con `Authorization: Bearer`.

## Humo contra el control plane real (hecho, 2026-11-03)

Control plane real (binario `atenea-control-plane`) con PostgreSQL 17 y rol de aplicación restringido, una organización, una red `10.77.0.0/24` y dos dispositivos creados por el flujo real (`atenea device-init` + `POST /enroll` con token `auto_approve`, uno con `role: server`). Token de servicio creado con `POST /orgs/{org}/service-tokens` (owner + CSRF). El MCP se ejecutó por stdio con un cliente del SDK:

- `tools/list`: las 4 tools.
- `atenea_search "servidor"` devuelve el dispositivo `role: server` con `ipv4`/`ipv6` y `declared`; `atenea_get` devuelve el recurso completo.
- `atenea_probe ping` ejecutó el `ping` real y respondió `ok:false, "sin respuesta"` (no había túnel, resultado esperado); un nombre inexistente se rechazó antes de lanzar nada.
- `atenea_api_get /api/v1/orgs` funciona; `/orgs/{org}/service-tokens` → `path_not_allowed`.
- El mismo token en `PATCH /networks/{net}` → `403 service_token_read_only`.

No cubierto: `tcp`, `ssh-banner` y `dns` reales, `report` de un agente real y una segunda red. Entorno temporal eliminado después.

## Instalación local en omp, Claude Code y Codex

No realizada: requiere orden expresa del usuario (el plan, Task 6 Step 4).
