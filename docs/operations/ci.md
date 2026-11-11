# Integración continua: en pull requests y a demanda

- **Estado:** desde 2026-11-11 la CI **corre en los pull requests contra `main`** y también a demanda. No corre al hacer push ni semanalmente (decisión del responsable de 2026-10-28, que se mantiene en esas dos partes: la CI tardaba demasiado en cada cambio). Se reactivó en pull requests porque `main` exige el check `check` y, sin él, ningún PR podía integrarse.
- **Cómo lanzarla a mano:** *Actions → CI → Run workflow* en GitHub, o `gh workflow run CI` (con `--ref` para otra rama). Como el disparo manual no tiene base de comparación, corre **todos** los jobs; en un pull request, los de plataforma corren solo si el cambio los necesita (ver «Filtro por rutas»).

## Jobs

| Job | Obligatorio para `main` | Duración medida (2026-10-28, una ejecución) |
| --- | --- | --- |
| `changes` | no | segundos |
| `check` | **sí**; corre en cada pull request | ~4,5 min |
| `panel` | no | ~1 min |
| `supply-chain` | no | ~1 min |
| `macos` | no | 3,9 min |
| `windows` | no | 4,2 min |
| `desktop-macos` | no | 3,4 min |
| `desktop-windows` | no | 4,7 min |

Los jobs de plataforma corren en paralelo: la ejecución completa tarda lo del más lento (~5 min).

## Riesgos de no lanzarla en cada push (aceptados por el responsable)

- **Un push directo a `main` no se comprueba.** Solo se comprueba lo que pasa por un pull request. Hay que ejecutarla **antes de publicar un release** y siempre que se toque código nativo fuera de un PR.
- **Advisories nuevas.** La ejecución semanal que las detectaba sin que cambiara el repositorio ya no existe: `cargo deny` y `pnpm audit` solo corren en pull requests y a demanda. Lanzar la CI de vez en cuando por este motivo.
- **Protección de `main`.** El check requerido `check` lo publica el pull request. Con `strict` activado, un PR atrasado respecto a `main` debe actualizarse (y la CI vuelve a correr) antes de integrarse.
- **`release.yml` no cambia:** se dispara por tag `v*` y construye por su cuenta; no depende de esta CI, así que un release no prueba lo que la CI no hubiese comprobado antes.
- Comprobación local equivalente a `check`: `scripts/check.sh`.

## Filtro por rutas

`scripts/ci-changes.sh` decide qué jobs de plataforma necesita un cambio, y `tests/ci/changes.sh` (parte de `check.sh`) prueba sus reglas. En un pull request compara con la base del PR; con `workflow_dispatch` devuelve siempre «todo». Reglas: *nativo* = `apps/agent/`, `apps/cli/`, `crates/`, `packaging/`, `tests/macos/`, `tests/windows/`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, este workflow y el propio script; *escritorio* = `apps/desktop/`, `crates/windows/`, `rust-toolchain.toml`, este workflow y el script. Ante la duda (sin base, evento desconocido) ejecuta todo.

## Límites

- `macos`, `windows` y los de escritorio no bloquean `main` aunque se lancen.
- Sin caché de compilación (no se ha medido su efecto).
- La aceptación privilegiada de macOS y Windows se ejecuta solo en runners desechables, nunca en un equipo real.
