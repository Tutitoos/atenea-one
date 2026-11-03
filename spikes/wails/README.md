# Spike de Wails v3 (ADR-019)

Programa mínimo para convertir en hechos las afirmaciones de [ADR-019](../../docs/architecture/ADR-019-aplicacion-de-escritorio-wails.md) antes de depender de Wails. **No es la aplicación de escritorio** y no se fusiona en `main`: vive en la rama `spike/wails`.

| Spike | Qué comprueba | Dónde |
| --- | --- | --- |
| S1 | La CSP de ADR-011 (adaptada) bloquea `fetch`, WebSocket, imagen y script remotos, script en línea y `eval`, y deja pasar al propio puente | `frontend/spike.js` en un webview real |
| S2 | Qué globales inyecta el *runtime* y qué métodos Go puede invocar la página; guardia de superficie del servicio | `frontend/spike.js`, `main_test.go`, `internal/surface` |
| S3 | La bandeja se crea y se actualiza (icono y menú) | `tray.go` |
| S4 | Backend portado de Rust con sus mismos casos de prueba: validación, orden elevada, socket de estado con tope de 4 MiB | `internal/backend` |
| S5 | Empaquetado oficial de la plantilla (`wails3 package`), SBOM CycloneDX, `go mod vendor`/`verify` | workflow |
| S6 | Licencias de los módulos vendorizados frente a la lista permitida de `deny.toml` | `tools/license_report.py` |

Lo ejecuta `.github/workflows/spike-wails.yml` al empujar a `spike/wails` (Linux, macOS y Windows, Wails v3.0.0 y v3.1.0) y deja un informe por sistema como artefacto. Nada de esto instala nada en el Mac del operador.

Límites: que la bandeja se cree sin error no prueba que sea visible (eso solo se ve en un escritorio real); el empaquetado se prueba con la plantilla oficial, no con esta aplicación; las licencias se clasifican por palabras clave.

## Resultado (ejecución `41680261150`, 2026-11-03, commit `063345b`)

Ocho trabajos: Ubuntu 24.04 (GTK4/WebKitGTK 6.0), macOS y Windows con Wails v3.0.0 y v3.1.0, y Ubuntu 22.04 con `-tags gtk3` con las dos versiones. Los resultados coinciden en todos. El detalle y las conclusiones están en [ADR-019](../../docs/architecture/ADR-019-aplicacion-de-escritorio-wails.md#resultado-de-los-spikes-2026-11-03).

| Spike | Resultado |
| --- | --- |
| S1 | CSP aplicada: las 6 sondas (fetch, WebSocket, imagen y script remotos, script en línea, `eval`) bloqueadas con violación de CSP; el origen propio y el puente funcionan; sin ningún socket de red del proceso (medido con la app viva) |
| S2 | Todo método exportado del servicio es invocable desde la página; los no exportados y los de tipos no registrados, no. El runtime ofrece además Clipboard, Window, System, Screens, Browser, Application, Dialogs y Events **sin forma oficial de desactivarlos**; un filtro por objeto en el servidor de recursos deja pasar solo `Call` y `CancelCall` y rechaza el resto, con la aplicación funcionando |
| S3 | La bandeja se crea y se actualiza (icono y menú) en los tres sistemas; la visibilidad real no se comprueba en CI |
| S4 | 16 pruebas del backend portado y 5 del filtro, en Linux, macOS y Windows |
| S5 | AppImage, DEB, RPM y Arch (Linux), `.app` y DMG (macOS), instalador NSIS (Windows); compilación reproducible; `go mod vendor`/`verify` y SBOM CycloneDX |
| S6 | 8 módulos (v3.0.0) y 7 (v3.1.0), todos dentro de la lista permitida; `govulncheck` sin vulnerabilidades |

Hallazgo de la ronda 1: en Ubuntu 24.04 la aplicación **se cierra al arrancar** (`bwrap: setting up uid map: Permission denied`) porque AppArmor restringe los espacios de nombres de usuario que usa el sandbox de WebKit. En el runner se resolvió con `kernel.apparmor_restrict_unprivileged_userns=0`; en un escritorio real hay que comprobarlo.
