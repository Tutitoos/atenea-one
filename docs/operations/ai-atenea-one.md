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

## Humo contra el control plane real (PENDIENTE: depende de la Task 1)

Todavía **no se ha ejecutado**: necesita el servidor con tokens de servicio (Task 1 del [plan](../superpowers/plans/2026-11-03-ai-atenea-one.md)). Cuando exista:

1. Arrancar el control plane del laboratorio (`tests/network-lab`) con 2 redes y 3 dispositivos sintéticos.
2. Crear un token con `POST /api/v1/orgs/{org}/service-tokens` (sesión de `owner` + CSRF) y guardarlo según el [README](../../packages/ai-atenea-one/README.md).
3. Por stdio (cliente SDK o `@modelcontextprotocol/inspector --cli`) ejecutar `atenea_search "servidor"`, `atenea_get` del resultado y `atenea_probe ping` a un dispositivo del laboratorio.
4. Confirmar que una escritura vía `atenea_api_get` es imposible y que el control plane responde `403 service_token_read_only` a un `POST` con el mismo token.
5. Registrar aquí el resultado (sin IP, alias ni inventarios reales).

## Instalación local en omp, Claude Code y Codex

No realizada: requiere orden expresa del usuario (el plan, Task 6 Step 4).
