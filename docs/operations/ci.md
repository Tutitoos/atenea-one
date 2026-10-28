# Integración continua: qué corre y cuándo

- **Estado:** vigente desde 2026-10-28. Las duraciones salen de los tiempos **por paso medidos** en la ejecución de `main` del commit `b81af18`, cuando `macos` y `windows` aún incluían la aplicación de escritorio; las de los jobs ya separados son **la suma de sus pasos, no una medición**. Se sustituirán por medidas reales tras unas ejecuciones.

## Jobs

| Job | Cuándo corre | Obligatorio para `main` | Duración (medida o derivada de pasos medidos) |
| --- | --- | --- | --- |
| `changes` | siempre | no | segundos |
| `check` | siempre (salvo la ejecución semanal) | **sí** | ~4,5 min |
| `panel` | siempre | no | ~1 min |
| `supply-chain` | siempre | no | ~1 min |
| `macos` | si cambia código nativo, o en la ejecución semanal/manual | no | ~3 min (agente, CLI y aceptación privilegiada) |
| `windows` | igual que `macos` | no | ~4 min |
| `desktop-macos` | si cambia `apps/desktop/`, `crates/windows/` o la toolchain, o en la ejecución semanal/manual | no | ~2,5 min |
| `desktop-windows` | igual que `desktop-macos` | no | ~4,5 min |

Antes de esta reorganización, `macos` y `windows` corrían siempre y compilaban dentro la aplicación de escritorio (4,4 min de los ~8,5 de Windows): ~10 min por push. Ahora el desplegable más lento corre en paralelo y solo cuando procede.

## Qué cuenta como «código nativo» y como «escritorio»

Lo decide `scripts/ci-changes.sh` comparando los ficheros cambiados entre el commit anterior del push (o la base de la PR) y el nuevo:

- **Nativo:** `apps/agent/`, `apps/cli/`, `crates/`, `packaging/`, `tests/macos/`, `tests/windows/`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, este workflow y el propio script.
- **Escritorio:** `apps/desktop/`, `crates/windows/` (única dependencia del workspace en Windows), `rust-toolchain.toml`, este workflow y el script.

No disparan plataforma: documentación, `apps/web`, `apps/control-plane`, `apps/coordinator`, `apps/relay`, `tests/network-lab`, `infra/`, `deploy/` y otros workflows. **Ante la duda ejecuta todo**: primer push, base inexistente o historia reescrita, ejecución semanal, manual y cualquier evento desconocido.

`tests/ci/changes.sh` prueba esas reglas con un repositorio temporal (cambios por ruta, varias confirmaciones en un push, borrados, renombrados, PR y todos los casos «ante la duda»); `scripts/check.sh` lo ejecuta. Se comprobó con una mutación (quitar `crates/` de las rutas vigiladas) que la prueba falla cuando debe.

## Red de seguridad

- **Ejecución semanal** (lunes, además de las advisories): corre todos los jobs de plataforma y de escritorio sin filtrar.
- **Ejecución manual completa:** *Actions → CI → Run workflow* (`workflow_dispatch`).
- **Releases:** `release.yml` compila todo en nativo para Linux, macOS y Windows; una rotura de plataforma que se hubiera saltado un filtro aparece, como tarde, al publicar.
- **Lo que los filtros NO ven:** un cambio fuera de las rutas vigiladas que rompa una plataforma (p. ej. un `cfg` en código compartido de otro componente). Por eso `Cargo.toml`, `Cargo.lock` y `crates/` sí están vigilados.

## Límites

- `macos`, `windows` y los de escritorio **no bloquean** `main`: un fallo no impide el push. Revisarlos antes de publicar.
- Sin caché de compilación (evaluada, no adoptada: añade una dependencia a la CI y no se ha medido su efecto).
- La aceptación privilegiada de macOS y Windows se ejecuta solo en runners desechables, nunca en un equipo real.
