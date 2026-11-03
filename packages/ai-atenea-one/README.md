# ai-atenea-one

Plugin de IA (omp, Claude Code y Codex) de **solo lectura** para Atenea One: un MCP stdio y la skill `atenea-one` para descubrir redes, dispositivos y servidores y verificar su alcance. Diseño: [ADR-020](../../docs/architecture/ADR-020-token-de-servicio-de-solo-lectura.md) y [plan](../../docs/superpowers/plans/2026-11-03-ai-atenea-one.md). Operación y verificación: [docs/operations/ai-atenea-one.md](../../docs/operations/ai-atenea-one.md).

## Tools

| Tool | Uso |
| --- | --- |
| `atenea_search` | Busca en organizaciones, redes, dispositivos, rutas, políticas y DNS. Todos los términos deben coincidir; máximo 50 resultados. |
| `atenea_get` | Recurso completo por `kind` e `id`. |
| `atenea_api_get` | `GET` libre sobre la lista permitida de `/api/v1` (auditoría, métricas…); respuesta truncada a 64 KiB. |
| `atenea_probe` | Verifica **un** dispositivo del inventario: `ping`, `tcp`, `ssh-banner` (sin autenticar) o `dns` (solo servidores DNS de la red). |

Ninguna tool escribe: el cliente solo emite `GET` y rechaza cualquier ruta fuera de la lista derivada de `docs/api/openapi.json` (sin `join-code`, `events`, `session`, `device/*` ni `service-tokens`).

## Requisitos

Node ≥ 22.18 (ejecuta TypeScript directamente) y las dependencias instaladas con `pnpm install` en el monorepo.

## Token de servicio

1. Un `owner` de la organización crea el token en el panel (o con `POST /api/v1/orgs/{org}/service-tokens {"name","ttl_secs"}`). El valor `atsvc_…` se muestra **una sola vez**.
2. Guárdalo fuera del repositorio, en `~/.config/ai-atenea-one/config.toml` con modo `0600` (el MCP rechaza el fichero si otros usuarios pueden leerlo):

   ```toml
   url = "https://control.example.test"
   token = "atsvc_<43 caracteres base64url>"
   ```

   ```sh
   mkdir -p ~/.config/ai-atenea-one && chmod 700 ~/.config/ai-atenea-one
   chmod 600 ~/.config/ai-atenea-one/config.toml
   ```

   Alternativa: variables de entorno `ATENEA_AI_URL` y `ATENEA_AI_TOKEN`.
3. Caduca (3600 s a 90 días) y se revoca desde el panel; con `401 invalid_session` pide uno nuevo.

El token no se escribe en logs ni en respuestas de las tools. No lo pegues en commits, issues ni documentos públicos.

## Instalación

- **omp:** instala el directorio de este paquete como plugin local (`.omp-plugin/marketplace.json`, `package.json#omp`).
- **Claude Code:** plugin local desde este directorio (`.claude-plugin/plugin.json`; el MCP se declara en `.mcp.json`).
- **Codex:** plugin local desde este directorio (`.codex-plugin/plugin.json`); lanza el MCP con `scripts/launch-codex-mcp.ts`, que reinstala las dependencias si el plugin se materializó sin `node_modules`.

Instalar en cada cliente es una acción local explícita del usuario; nada se publica ni despliega desde el repositorio.

## Desarrollo

```sh
pnpm --filter ai-atenea-one test
pnpm --filter ai-atenea-one typecheck
pnpm --filter ai-atenea-one mcp   # arranca el MCP por stdio (requiere configuración)
```

El código TypeScript debe ser *erasable* (sin `enum` ni propiedades de parámetro): Node solo borra tipos.
