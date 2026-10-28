# Integración continua: solo a demanda

- **Estado:** desde 2026-10-28 la CI **no se lanza sola**: ni al hacer push, ni en pull requests, ni semanalmente. Solo se ejecuta cuando el responsable la pide (decisión suya: la CI tardaba demasiado en cada cambio).
- **Cómo lanzarla:** *Actions → CI → Run workflow* en GitHub, o `gh workflow run CI` (con `--ref` para otra rama). Como el disparo es manual, corre **todos** los jobs.

## Jobs

| Job | Obligatorio para `main` | Duración medida (2026-10-28, una ejecución) |
| --- | --- | --- |
| `changes` | no | segundos |
| `check` | **sí**, pero nunca se ejecuta solo (ver riesgos) | ~4,5 min |
| `panel` | no | ~1 min |
| `supply-chain` | no | ~1 min |
| `macos` | no | 3,9 min |
| `windows` | no | 4,2 min |
| `desktop-macos` | no | 3,4 min |
| `desktop-windows` | no | 4,7 min |

Los jobs de plataforma corren en paralelo: la ejecución completa tarda lo del más lento (~5 min).

## Riesgos de no lanzarla sola (aceptados por el responsable)

- **Nada avisa de una rotura.** Un push que rompa formato, clippy, pruebas, macOS o Windows no se ve hasta que se lance la CI. Hay que ejecutarla **antes de publicar un release** y siempre que se toque código nativo.
- **Advisories nuevas.** La ejecución semanal que las detectaba sin que cambiara el repositorio ya no existe: `cargo deny` y `pnpm audit` solo corren a demanda. Lanzar la CI de vez en cuando por este motivo.
- **Protección de `main`.** El check requerido `check` no se publica solo; los pushes del propietario ya pasaban por la excepción de administrador, pero un pull request quedaría esperando un check que nadie lanza hasta ejecutarla a mano sobre su rama.
- **`release.yml` no cambia:** se dispara por tag `v*` y construye por su cuenta; no depende de esta CI, así que un release no prueba lo que la CI no hubiese comprobado antes.
- Comprobación local equivalente a `check`: `scripts/check.sh`.

## Filtro por rutas (sin efecto con el disparo manual)

`scripts/ci-changes.sh` decide qué jobs de plataforma necesita un cambio, y `tests/ci/changes.sh` (parte de `check.sh`) prueba sus reglas. Con `workflow_dispatch` devuelve siempre «todo»; se conserva para volver a activar `push`/`pull_request` sin reescribirlo: bastaría añadir esos eventos a `on:` en `ci.yml`. Reglas: *nativo* = `apps/agent/`, `apps/cli/`, `crates/`, `packaging/`, `tests/macos/`, `tests/windows/`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, este workflow y el propio script; *escritorio* = `apps/desktop/`, `crates/windows/`, `rust-toolchain.toml`, este workflow y el script. Ante la duda (sin base, evento desconocido) ejecuta todo.

## Límites

- `macos`, `windows` y los de escritorio no bloquean `main` aunque se lancen.
- Sin caché de compilación (no se ha medido su efecto).
- La aceptación privilegiada de macOS y Windows se ejecuta solo en runners desechables, nunca en un equipo real.
