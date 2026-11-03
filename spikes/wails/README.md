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
