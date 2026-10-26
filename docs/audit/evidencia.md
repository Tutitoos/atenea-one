# Evidencia por caso — paquete de auditoría

Referencia: `git rev-parse HEAD` de la copia revisada (al redactar `a0f8ac71b5e83607603c45b46113a2aa0ea607b6`). Las cifras y fechas de laboratorio **no se duplican**: viven en la [matriz de aceptación](../operations/matriz-de-aceptacion.md) (corte 2026-10-25, `d9ab7ec…`) y en los informes enlazados; aquí solo se enlaza y se resume el estado. Cómo reproducir: [como-reproducir.md](como-reproducir.md).

**No es un veredicto: ni «seguro», ni «auditado», ni «listo para producción».** La propia matriz exige auditoría independiente antes de datos sensibles.

## Cómo leer los estados

| Estado | Significado en este paquete |
| --- | --- |
| VERIFICADO | El artefacto (script, prueba, workflow, documento) existe y se leyó; **no** implica que se ejecutara ni su resultado |
| PARCIAL | Existe evidencia, pero la fuente enlazada declara una brecha (se cita la fila) |
| SIN COMPROBAR | Sin ejecución o artefacto accesible que lo demuestre |
| INFERIDO | Deducido del diseño/código; no medido |

Las etiquetas propias de la matriz (VERIFICADO EN LABORATORIO, VERIFICADO EN CI, PARCIAL, SIN COMPROBAR, PENDIENTE DEL RESPONSABLE) se citan literalmente como «matriz: …».

Limitaciones de procedencia (de la [matriz](../operations/matriz-de-aceptacion.md#cómo-leer-la-evidencia)):

1. «LOCAL-V2» = ejecución local sobre un árbol previo a `d9ab7ec` (Docker Desktop, Mac arm64, sin red externa), sin identidad byte a byte con ese commit. Solo la CI se atribuye a `d9ab7ec`.
2. Las salidas `/tmp/v2-*.txt`, `/tmp/v2b-*.txt` **no están versionadas**. El `.gitignore` raíz ignora `/tests/load/out/` (donde la matriz sitúa la salida de carga); `tests/load/.gitignore` (fichero aparte, versionado) ignora `results/`. Verificado con `git check-ignore`. Ninguna de las dos rutas viaja en un clon.
3. [CI 40228501883](https://github.com/Tutitoos/atenea-one/actions/runs/40228501883): verde en check, panel, macos, windows y supply-chain según la matriz; no he accedido a ese run (SIN COMPROBAR por mí).

## Tabla caso → enlaces → estado

| Caso | Enlaces de evidencia | Estado | Límite principal |
| --- | --- | --- | --- |
| Protocolo wire-v2 (formato, MAC1/MAC2, cookie) | [wire-v2](../protocol/wire-v2.md), [ADR-003](../architecture/ADR-003-cifrado-antirreplay.md), [`cookie/tests.rs`](../../crates/crypto/src/cookie/tests.rs), [`cookie/vectors.rs`](../../crates/crypto/src/cookie/vectors.rs) | VERIFICADO (existencia de contrato, vectores y pruebas); ejecución individual SIN COMPROBAR | El contrato admite revisión externa pendiente; origen Python de vectores no versionado ([§4](como-reproducir.md#4-vectores-wire-v2-y-relay-v2)) |
| Protocolo relay-v2 | [relay-v2](../protocol/relay-v2.md), [`relay-core/src/lib.rs`](../../crates/relay-core/src/lib.rs) (`published_vectors_match_independent_python`), [`apps/relay/tests/relay.rs`](../../apps/relay/tests/relay.rs) | VERIFICADO (existencia); ejecución SIN COMPROBAR | El propio doc dice que la prueba de vectores «queda pendiente de ejecución»; sin informe posterior específico encontrado |
| Modelo de amenazas | [threat-model](../security/threat-model.md), resumen T1–T30 en la [matriz](../operations/matriz-de-aceptacion.md#hallazgos-abiertos-y-riesgos-aceptados) | VERIFICADO (lectura del documento) | Estados T1–T30 son los de la matriz, no una verificación independiente |
| Self-hosted real | matriz fila «Self-hosted real»; [stack](../../tests/stack/run.sh), [infra](../../tests/infra/run.sh), [stack-produccion](../operations/stack-produccion.md), [infraestructura](../operations/infraestructura.md) | PARCIAL (matriz: PARCIAL) | Sin proveedor real; DNS/ACME/puertos públicos SIN COMPROBAR |
| P2P y fallbacks relay UDP/TLS | matriz fila «P2P y ambos fallbacks»; [relay E2E](../../tests/network-lab/nat/run-e2e-relay.sh), [panel](../../tests/e2e-panel/run.sh) | VERIFICADO EN LABORATORIO (matriz) | Solo namespaces/Docker; sin Internet ni proxies corporativos |
| Cifrado E2E / captura | matriz fila «E2E»; [lab base](../../tests/network-lab/run.sh), [relay](../../tests/network-lab/nat/run-e2e-relay.sh), [subredes](../../tests/network-lab/subnet/run.sh), [threat-model](../security/threat-model.md), [ADR-005](../architecture/ADR-005-custodia-clave-raiz.md) | PARCIAL (matriz: PARCIAL) | Capturas con marcador no son análisis criptográfico ni prueba exhaustiva de ausencia de claves en memoria/artefactos; falta revisión independiente del límite de claves; pcap original no archivado públicamente; no impide que un emisor comprometido autorice identidades |
| Identidad y claves | matriz fila «Identidad y claves»; [lab fase 3](../operations/lab-fase3.md), [copias y recuperación](../operations/copias-y-recuperacion.md), [ADR-005](../architecture/ADR-005-custodia-clave-raiz.md), [identity](../../crates/identity/src/tests.rs) | PARCIAL (matriz) | Rotación de raíz y reincorporación sin ensayo completo; token hardware pendiente |
| Replay y reinicio | matriz fila «Replay y reinicio»; [lab fase 2](../operations/lab-fase2.md), [`replay.rs`](../../crates/crypto/src/replay.rs), fuzz `replay_window`/`session_decrypt` | PARCIAL (matriz) | Sin prueba formal de unicidad de nonce; rekey bajo carga sin trazabilidad específica |
| Autorización/ACL/RBAC | matriz fila «Autorización»; [E2E fase 3B](../operations/e2e-fase3b.md), [E2E fase 7](../operations/e2e-fase7.md), [control-plane tests](../../apps/control-plane/tests/run.sh) | PARCIAL (matriz) | Sin auditoría independiente; ACL inversa sin puertos (F6) aceptada |
| Revocación y caídas | matriz fila «Revocación/offline»; [caídas y leases](../operations/caidas-y-leases.md), [drills](../../tests/stack/drills/run.sh) | PARCIAL (matriz) | Simulacros CP/BD piloto son **pre-v2**; repetir post-v2; atribución del fallo multipar histórico abierta |
| Red real (NAT, MTU, IPv6) | matriz fila «Red real»; [NAT lab](../../tests/network-lab/nat/run-e2e.sh), [fase 4](../operations/e2e-fase4.md), [MTU](../protocol/mtu.md) | PARCIAL (matriz) | Todo emulado en un kernel; CGNAT/roaming/MTU/IPv6 reales SIN COMPROBAR |
| Clientes Linux/macOS/Windows | matriz fila «Cliente local»; [macOS](../operations/instalacion-macos.md), [Windows](../operations/instalacion-windows.md), [Linux](../operations/instalacion-linux.md), [`ci.yml`](../../.github/workflows/ci.yml) | PARCIAL (matriz) | Runners GitHub, no máquinas físicas; `.pkg`/ZIP sin firma nativa |
| Operación: backup/restore/upgrade/rollback | matriz fila «Operación»; [copias y recuperación](../operations/copias-y-recuperacion.md), [upgrade](../../tests/stack/upgrade.sh), [soporte](../operations/soporte-limites-y-recuperacion.md) | PARCIAL (matriz) | Docker/Mac, dataset pequeño, no VPS; sin restauración real |
| Panel (UI/accesibilidad) | matriz fila «Panel»; [e2e-fase6](../operations/e2e-fase6.md), [Playwright](../../tests/e2e-panel/run.sh) | PARCIAL (matriz) | Falta revisión visual humana y recorrido solo teclado |
| Rendimiento, presupuestos y capacidad | matriz fila «Pruebas de carga/soak»; [rendimiento y presupuestos](../operations/rendimiento-y-presupuestos.md), [metodología de control](../operations/carga-control-metodologia.md), [`budgets.json`](../../tests/load/budgets.json) | PARCIAL (matriz: PARCIAL) | Sin soak de 72 h (el de 12 min no lo sustituye); latencia del control plane a 5 000–10 000 dispositivos (causa SIN COMPROBAR), relay saturado en un núcleo, reconexión tras reinicio hasta ≈25 s, recreación de sesión de relay cada ≈300 s; solo Docker/Mac arm64, no dimensiona un VPS |
| Fuzzing (13 objetivos) | [fuzz/README](../../fuzz/README.md), [`fuzz_targets/`](../../fuzz/fuzz_targets), [`run.sh`](../../fuzz/run.sh) | VERIFICADO (13 objetivos existen); ejecución v2 SIN COMPROBAR | Última corrida histórica pre-v2 (90 s/objetivo, 0 fallos); sin campaña larga; `fuzz/regressions/` sin reproductores |
| Release y firma de manifiestos | [release](../operations/release.md), [`tests/release/run.sh`](../../tests/release/run.sh), [`verify.sh`](../../scripts/release/verify.sh), [`release.yml`](../../.github/workflows/release.yml), [ADR-009](../architecture/ADR-009-operacion-y-releases.md) | PARCIAL (release.md: ensayo local sin red; publicación SIN COMPROBAR) | Sin release publicado, sin clave real, atestaciones y `gh attestation verify` no ejecutados |
| Cadena de suministro | [deny.toml](../../deny.toml), [dependencies](../architecture/dependencies.md), job `supply-chain` de [`ci.yml`](../../.github/workflows/ci.yml) | VERIFICADO EN CI (matriz) | Dos excepciones temporales (RUSTSEC-2026-0387, RUSTSEC-2023-0071) con revisión antes de 2027-01-23; confianza residual en GitHub |
| Observabilidad | [observabilidad](../operations/observabilidad.md), [`tests/observability`](../../tests/observability/run.sh) | VERIFICADO (existencia); resultados no revisados por mí | No resumida en la matriz como caso propio |
| Repositorio público / secretos | [SECURITY.md](../../SECURITY.md), [ADR-006](../architecture/ADR-006-licencia.md), matriz fila «Repositorio público» | PARCIAL (matriz) | Licencia ratificada el 2026-10-26. Clon anónimo y escaneo de los 38 commits del historial el 2026-10-26 (sin claves privadas, tokens, alias privados ni `PLAN.md`); no prueba ausencia absoluta de secretos |
| Auditoría independiente | matriz fila «Auditoría independiente»; [SECURITY.md](../../SECURITY.md) | PENDIENTE DEL RESPONSABLE (matriz) | Bloqueante para datos sensibles; este paquete no la sustituye |
| Snapshot y comprobación del paquete | [README del paquete](README.md#snapshot-sin-red-y-verificación), [`snapshot.sh`](../../scripts/audit/snapshot.sh), [`verify-snapshot.sh`](../../scripts/audit/verify-snapshot.sh), [`check-docs.py`](../../scripts/audit/check-docs.py), [cómo reproducir §9](como-reproducir.md#9-snapshot-del-paquete-y-comprobador-documental) | VERIFICADO — ejecución local según el README (dos snapshots+verificador con mismo SHA-256; comprobador al final); no he repetido la ejecución | Integridad local, no firma ni autoría; solo HEAD (excluye cambios sin commit); no valida el código |

## Lo NO revisado ni probado

- **Docker en Mac arm64**: todos los resultados LOCAL-V2 proceden de ese entorno; no representa un VPS ni Linux x86_64 nativo.
- **Runners de CI**: solo se declaran en la matriz y en `ci.yml`; no se inspeccionó el log del run ni se reprodujo. CI no ejecuta los laboratorios Docker, fuzz, carga ni infra (VERIFICADO leyendo `ci.yml`).
- **Hardware real**: NAT/CGNAT domésticos, roaming, MTU de redes reales, máquinas físicas macOS/Windows, token hardware de raíz, proveedor/Proxmox.
- **Ejecución por mi parte**: no ejecuté suites Rust/web, fuzz, Docker, laboratorios ni release. Sí se ejecutaron snapshot, verificador y comprobador documental, con el alcance del [README](README.md#snapshot-sin-red-y-verificación). Todo «VERIFICADO» sobre código/tests significa leído.
- **Vectores**: origen Python no versionado; independencia no demostrable desde el repositorio.
- **Post-v2**: simulacros de caída piloto, fuzz y soak largos; informes históricos (`e2e-fase5`, `stack-produccion`, `caidas-y-leases`) hablan de wire/relay-v1 y no certifican v2 ([matriz](../operations/matriz-de-aceptacion.md#cómo-leer-la-evidencia)).
- **Paquete documental**: el snapshot solo archiva HEAD; los documentos nuevos no se incluyen hasta que se registren en un commit ([README](README.md#referencia-y-alcance-temporal)).
