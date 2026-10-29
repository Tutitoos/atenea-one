# Cómo reproducir — guía para auditor externo

**Actualizado el 2026-10-29 al commit 8d975a6**.

Referencia de esta guía: `8d975a6`; anote `git rev-parse HEAD` de la copia que audita. Las ejecuciones históricas enlazadas conservan su commit/fecha propios y no se trasladan automáticamente a este corte.

**No apto para datos sensibles hasta la auditoría.** Esta guía describe reproducción, no acredita auditoría independiente ni aptitud para producción. En esta actualización solo se ejecutaron recuentos/lecturas de Git y el comprobador documental/higiene; **SIN COMPROBAR aquí:** compilaciones, suites Rust/web/desktop, fuzz, Docker, laboratorios, release, snapshot y su verificador. Las ejecuciones anteriores citadas abajo son evidencia documental, no ejecuciones de esta sesión.

Etiquetas: **VERIFICADO — lectura** (script/fichero inspeccionado), **VERIFICADO — ejecución local** (comando ejecutado aquí, con resultado indicado), **PARCIAL**, **SIN COMPROBAR**, **INFERIDO**. Las menciones históricas «VERIFICADO» de esta guía significan lectura, salvo atribución explícita a una ejecución anterior.

## 1. Clonar y requisitos

```sh
git clone https://github.com/Tutitoos/atenea-one.git
cd atenea-one
git rev-parse HEAD            # anotar; comparar con el commit bajo revisión
```

- Clon anónimo: la [matriz](../operations/matriz-de-aceptacion.md) informa comprobación el 2026-10-26 de `a0f8ac7`; es evidencia histórica leída, no un clon de HEAD ejecutado aquí (**SIN COMPROBAR** en esta sesión).
- Rust fijado en [`rust-toolchain.toml`](../../rust-toolchain.toml): `1.99.1` con `rustfmt` y `clippy` (VERIFICADO). Necesita `rustup`.
- Node y pnpm: versiones en `package.json` ([README](../../README.md)); [`scripts/install-pnpm.sh`](../../scripts/install-pnpm.sh) instala pnpm fijado (usado por [CI](../../.github/workflows/ci.yml)). Node 24 en CI.
- Docker Linux (cgroup v2 para los ensayos con systemd) para laboratorios, fuzz, release y stack. Python 3.11+ para scripts de release ([release](../operations/release.md)).

## 2. Build y pruebas Rust/web: comandos normales

Lo que ejecuta CI, tal cual está en [`scripts/check.sh`](../../scripts/check.sh) (VERIFICADO por lectura):

```sh
scripts/setup.sh      # comprueba herramientas e instala dependencias (README)
scripts/check.sh
```

Equivalente expandido (de `check.sh`):

```sh
python3 scripts/check-headers.py
sh tests/ci/changes.sh
cargo fmt --all --check
cargo fmt --manifest-path apps/desktop/Cargo.toml --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
pnpm install --frozen-lockfile
pnpm -r --if-present run lint
pnpm -r --if-present run typecheck
pnpm -r --if-present run test
pnpm --filter @atenea/web run build
pnpm --filter @atenea/desktop run build
```

Control plane con PostgreSQL real (CI `check`; requiere Docker o `ATENEA_TEST_DATABASE_URL`, usa contenedor `postgres:17`):

```sh
apps/control-plane/tests/run.sh
```

Estos comandos pueden **descargar dependencias** (cargo/pnpm) la primera vez. Los clientes macOS/Windows en CI usan subconjuntos de paquetes (`atenea-tunnel atenea-agent atenea-cli atenea-testpeer`, más `atenea-windows`); ver [ci.yml](../../.github/workflows/ci.yml).

### 2.1 Escritorio Tauri 2: workspace Rust separado

**VERIFICADO — lectura:** [`apps/desktop/Cargo.toml`](../../apps/desktop/Cargo.toml), [`package.json`](../../apps/desktop/package.json), [`ci.yml`](../../.github/workflows/ci.yml). `cargo … --workspace` de la raíz **no incluye** desktop. Requiere Rust fijado, Node 24/pnpm fijado, dependencias pnpm instaladas y la interfaz compilada antes de compilar Tauri. En Linux se requieren dependencias nativas de Tauri/WebKitGTK; CI construye el Rust de escritorio en macOS y prueba también Windows. Windows no soporta operaciones de redes múltiples.

Desde la raíz, primero `pnpm install --frozen-lockfile`. Después, en `apps/desktop`:

```sh
pnpm run typecheck
pnpm run lint
pnpm run test
pnpm run build
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --offline
```

El comando offline es una **variante INFERIDA** del `cargo test --locked` del workflow, solicitada para reproducción sin red: exige la caché Cargo de **este workspace y su Cargo.lock** ya poblada (por ejemplo `cargo fetch --locked` con red desde `apps/desktop`). El build frontend también debe estar disponible. No implica que las dependencias nativas puedan instalarse sin red. **Todos estos comandos: SIN COMPROBAR aquí.** Fmt, clippy y tests de lógica no demuestran diálogos de elevación, notificaciones, autostart real ni conectividad del sistema.

## 3. Alternativa sin red, macOS (`--offline --locked`, `CARGO_BUILD_JOBS=2`)

Patrón que [release.md](../operations/release.md#evidencia-y-límites) informa como usado en macOS (`cargo fmt --all`, clippy de cinco paquetes con `--offline --locked --all-targets -- -D warnings`, 155 pruebas correctas, `CARGO_BUILD_JOBS=2`). **Prerrequisito online, fuera del bloque offline:** tener la caché de cargo poblada, p. ej. una vez con red `cargo fetch --locked`. Sin esa caché los comandos de abajo fallan en vez de descargar.

Los comandos siguientes son **INFERIDOS** (combinan `check.sh` con esas banderas; no hay script que los transcriba):

```sh
export CARGO_BUILD_JOBS=2
cargo fmt --all --check
cargo build --workspace --offline --locked        # build debug exacto
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo test --workspace --offline --locked
```

Equivalente normal (con red; deja que cargo resuelva la caché): `cargo build --workspace --locked`.

Nota: `release.md` solo atribuye ese resultado a «los cinco paquetes» (los de release: `atenea-cli atenea-agent atenea-control-plane atenea-coordinator atenea-relay`), no al workspace completo; el test del control plane sin PostgreSQL puede requerir `ATENEA_TEST_DATABASE_URL`. **SIN COMPROBAR** aquí.

Los workflows de release construyen con `cargo build --offline --locked --release -p atenea-cli -p atenea-agent -p atenea-control-plane -p atenea-coordinator -p atenea-relay` y `CARGO_BUILD_JOBS: 2` tras `cargo fetch --locked` ([release.yml](../../.github/workflows/release.yml), VERIFICADO).

## 4. Vectores wire-v2 y relay-v2

Pruebas concretas (existencia VERIFICADA; ejecución wire-v2 informada al final del apartado):

| Contrato | Prueba | Normal | macOS sin red |
| --- | --- | --- | --- |
| [wire-v2 §7](../protocol/wire-v2.md) vectores 1–4 (MAC1/MAC2/cookie/COOKIE_REPLY) | `independent_vectors_1_to_4` en [`crates/crypto/src/cookie/tests.rs`](../../crates/crypto/src/cookie/tests.rs); constantes en [`vectors.rs`](../../crates/crypto/src/cookie/vectors.rs) | `cargo test -p atenea-crypto --locked independent_vectors_1_to_4` | `CARGO_BUILD_JOBS=2 cargo test -p atenea-crypto --offline --locked independent_vectors_1_to_4` |
| wire-v2 vector 5 (Noise real y transporte bidireccional) | `independent_real_noise_vector_and_bidirectional_transport` (mismo `tests.rs`) | `cargo test -p atenea-crypto --locked independent_real_noise_vector_and_bidirectional_transport` | `CARGO_BUILD_JOBS=2 cargo test -p atenea-crypto --offline --locked independent_real_noise_vector_and_bidirectional_transport` |
| [relay-v2 §9](../protocol/relay-v2.md) hex (ALLOC, tramas TCP300, cookie relay) | `published_vectors_match_independent_python` en [`crates/relay-core/src/lib.rs`](../../crates/relay-core/src/lib.rs) (lee la tabla de `relay-v2.md` con `include_str!`) | `cargo test -p atenea-relay-core --locked published_vectors_match_independent_python` | `CARGO_BUILD_JOBS=2 cargo test -p atenea-relay-core --offline --locked published_vectors_match_independent_python` |

La columna «macOS sin red» exige la caché de cargo ya poblada (ver apartado 3).

**Generadores independientes versionados:** [`wire_v2_vectors.py`](../../tests/vectors/wire_v2_vectors.py)
y [`relay_v2_vectors.py`](../../tests/vectors/relay_v2_vectors.py). Solo requieren
Python 3 y `cryptography` **ya instalado**; no invocan Rust, red, Docker ni
instaladores. Wire calcula HChaCha20 y transcript/KDF Noise por separado de
snow, usando `hashlib.blake2s`, X25519 y ChaCha20Poly1305 de `cryptography`.
Relay deriva Ed25519 con `cryptography` y su cookie con `hashlib`; comparte
únicamente el comparador de tablas con el script wire. Las claves y efímeras
fijas son exclusivamente vectores de prueba, nunca configuración de producción.

Desde la raíz del repositorio:

```sh
python3 tests/vectors/wire_v2_vectors.py --check
python3 tests/vectors/relay_v2_vectors.py --check
```

Sin `--check`, cada script emite su tabla hex determinista. Con `--check`,
calcula todos los bytes de nuevo y compara nombres y valores con las tablas de
los documentos; filas ausentes, duplicadas, adicionales o distintas dan salida
no cero. No copia las salidas de la documentación para generarlas.

**Ejecutado el 2026-10-26, macOS sin red:** wire-v2 **28 salidas hex coinciden**
y relay-v2 **15 salidas hex coinciden**, sin cambiar las tablas publicadas.
En la verificación Rust de esta integración, el comando
`CARGO_BUILD_JOBS=2 cargo test -p atenea-protocol -p atenea-crypto --offline --locked`
aprobó **32 pruebas** (crypto25, protocol7), incluidas
`independent_vectors_1_to_4` e
`independent_real_noise_vector_and_bidirectional_transport`: confirman
Python=Rust para vectores wire1–5 y transporte bidireccional.
El generador relay confirma aquí Python=tabla; este paso no vuelve a ejecutar
la prueba Rust relay de la tabla anterior. No sustituye revisión criptográfica externa.

## 5. Fuzzing (14 objetivos)

Fuente: [`fuzz/README.md`](../../fuzz/README.md), [`fuzz/run.sh`](../../fuzz/run.sh), [`fuzz/Dockerfile`](../../fuzz/Dockerfile).

```sh
fuzz/run.sh                  # 14 objetivos, 60 s cada uno
fuzz/run.sh 300              # 300 s cada uno
fuzz/run.sh 60 hex_decode    # un objetivo
```

Objetivos en [`fuzz/fuzz_targets/`](../../fuzz/fuzz_targets) (14 ficheros, **VERIFICADO — lectura**): `agent_engine`, `cert_bundle_state_parse`, `config_v2_parse`, `datagram_parse`, `device_config_parse`, `handshake_init`, `hex_decode`, `ip_classify`, **`join_code_parse`**, `relay_parse`, `replay_window`, `session_decrypt`, `signed_config_verify`, `stun_parse`. `join_code_parse` cubre decodificación del código de unión en identity; es nuevo desde a0f8ac7. El README conserva antecedentes de campañas de 13 objetivos, no resultados del inventario actual.

- Requiere Docker; nightly + `cargo-fuzz` dentro de la imagen `atenea-fuzz`. `run.sh` construye la imagen (el `Dockerfile` hace `FROM rust:1.99-bookworm` y `cargo install cargo-fuzz`: **ese paso necesita red**, INFERIDO) y ejecuta los objetivos con `--network none` y `CARGO_NET_OFFLINE=true`, dependiendo de los volúmenes `atenea-fuzz-target` y `atenea-fuzz-cargo`. El repositorio no documenta cómo se llena `atenea-fuzz-cargo`; sin esa caché el arranque offline falla (INFERIDO de «falla en vez de descargar»).
- Salida ≠ 0 si falla un objetivo o regresión versionada. Semillas en `fuzz/seeds/`; `fuzz/regressions/` contiene su README, no reproductores versionados. Ejecución de los 14 objetivos actuales: **SIN COMPROBAR**.
- Reproducir un fallo: comando del README (`cargo +nightly fuzz run <objetivo> regressions/<objetivo>/<fichero>`).
- Estado: la última ejecución completa informada (90 s × 13, Docker Mac arm64, 0 fallos) es **histórica y anterior al cutover wire-v2/relay-v2**; el propio README dice que no verifica los objetivos/semillas v2 y que no se ejecutó fuzzing en esa integración. Para v2: **SIN COMPROBAR**. No hay campaña de horas/días con corpus persistente (corpus no versionado).

## 6. Laboratorios Docker y privilegios

Todos exigen Docker; casi todos usan `--privileged` (namespaces, nftables, TUN, netem, cgroups). Ejecutar solo en VM/máquina desechable, no en el host de producción ([tests/packaging/run.sh](../../tests/packaging/run.sh) lo advierte). Privilegios VERIFICADOS por lectura de los scripts; «qué prueba» según su README/cabecera y la [matriz](../operations/matriz-de-aceptacion.md), no por ejecución mía:

| Laboratorio | Comando | Qué prueba | Privilegio observado |
| --- | --- | --- | --- |
| Red base | `tests/network-lab/run.sh` ([README](../../tests/network-lab/README.md)) | Dos agentes y un no autorizado en namespaces: ping/TCP/UDP íntegros IPv4/IPv6, MTU 1400, captura sin texto claro, replay/contador alterado, pérdida/reorden/latencia (netem), reinicio, limpieza | `docker run --privileged` |
| NAT directo / relay | `tests/network-lab/nat/run-e2e.sh`, `tests/network-lab/nat/run-e2e-relay.sh` | Topologías NAT, directo, bloqueo P2P→UDP→TLS y vuelta, integridad por camino | `--privileged` |
| Subredes | `tests/network-lab/subnet/run.sh` | Router de subred, rutas aprobadas, SNAT, DNS dividido | `--privileged` |
| Multi-peer | `tests/network-lab/run-multi-peer.sh` | Convergencia de varios pares vía relay y cuotas por IP | `--privileged`, `--network none` |
| Stack producción | `tests/stack/run.sh`; `tests/stack/upgrade.sh`; `tests/stack/drills/run.sh lab\|pilot` | Compose de ocho servicios con OIDC/HAProxy/relays; upgrade N→N+1 y rollback; caídas de componentes y expiración de lease | runner `--privileged` en bridge propio; sin puertos de host |
| Panel Playwright | `tests/e2e-panel/run.sh` | Flujo del panel con control plane, PostgreSQL, Dex y dos agentes tras NAT | `--privileged` |
| Empaquetado `.deb` | `tests/packaging/run.sh` | Instalación del `.deb` en Debian con systemd/resolved, servicio, reinicio y retirada | `--privileged --cgroupns=host`, cgroup rw |
| Infraestructura | `bash tests/infra/run.sh` (imagen `atenea-infra:local` preexistente) | Convergencia de roles Ansible/OpenTofu en un nodo sin red externa | `--privileged --cgroupns=host`, `--network none` |
| Backup/restore | `tests/backup/run.sh` | Copia/restauración PostgreSQL (WAL/PITR) y claves cifradas, medición de RTO/RPO | `--privileged` en el laboratorio |
| Observabilidad | `tests/observability/run.sh` | Métricas, alertas y logs con Prometheus/Loki/Alloy/Grafana | `--privileged` |
| Carga | `bash tests/load/run.sh tunnel\|handshake\|relay`, `bash tests/load/stack.sh control`, `bash tests/load/soak.sh 0.34` ([rendimiento](../operations/rendimiento-y-presupuestos.md#reproducción-segura)) | Throughput/latencia del túnel, abuso de handshake, capacidad del relay, control plane/coordinator y soak | `--privileged --network none` (datos) |
| macOS utun | `tests/macos/run.sh` | Servicio/IPC/rutas/DNS en utun con limpieza | solo root en VM macOS de GitHub Actions (el script se niega fuera de ella) |
| Windows Wintun | `tests/windows/run.ps1` | Instalación ZIP, servicio, desinstalación con `-Purge` sin residuos | exige administrador y `GITHUB_ACTIONS=true` (se niega fuera de la VM desechable) |

Varios necesitan imágenes y volúmenes locales previos (`atenea-lab`, `atenea-lab-target`, `atenea-cargo-registry`, `atenea-panel-lab`, `atenea/*:local`); los scripts usan `--pull never` y no construyen todo. Cómo prepararlos desde cero offline no está resumido en un único paso: ver [stack](../operations/stack-produccion.md), [infraestructura](../operations/infraestructura.md), [carga de control](../operations/carga-control-metodologia.md).

**Evidencia de laboratorio frente a CI** (detalle y estado por caso en [evidencia.md](evidencia.md)):

- **VERIFICADO — lectura:** la CI ordinaria [`ci.yml`](../../.github/workflows/ci.yml) tiene únicamente **`workflow_dispatch`**: no se lanza con push, PR ni calendario. Los jobs definidos incluyen `check`, `panel`, macOS/Windows nativos, escritorio macOS/Windows y supply-chain (workspace raíz y desktop). Los comentarios históricos sobre ejecuciones semanales no constituyen un trigger. `release.yml` es independiente.
- La CI define `scripts/check.sh`, PostgreSQL real, pnpm lint/typecheck/test/build, empaquetado/aceptación privilegiada macOS y Windows, y análisis de dependencias. No define los laboratorios Docker de red, stack, infra, backup, carga ni fuzz. Resultados de jobs de HEAD: **SIN COMPROBAR aquí**. Los «LOCAL-V2» antiguos siguen siendo históricos y sus salidas no versionadas no están en un clon; los ensayos posteriores se distinguen abajo, sin atribuir a HEAD sus cifras.

### 6.1 Ensayos añadidos o ampliados hasta 8d975a6

**VERIFICADO — lectura** de cada script enlazado y de sus arneses; **ninguno ejecutado aquí (SIN COMPROBAR)**. Usar máquina/VM desechable: construyen imágenes, crean contenedores/redes y algunos trabajan con privilegios. No confundir recuentos publicados con resultados propios.

| Ensayo / comando exacto desde la raíz | Prerrequisitos y efectos | Qué demuestra si termina correctamente |
| --- | --- | --- |
| `bash tests/network-lab/run-two-networks.sh` ([script](../../tests/network-lab/run-two-networks.sh), [arnés](../../tests/network-lab/two-networks.sh)) | Docker; imágenes locales `atenea-lab`, `postgres:17`, Dex `v2.41.1`; volúmenes `atenea-lab-target` y `atenea-cargo-registry` poblados. Runner `--privileged`, PostgreSQL y Dex reales. No reconstruye ni retaguea `atenea-lab`. | CLI multirred con identidades/estado separados, unión/servicio/conflicto/salida; incluye rol servidor y `/server/info`. El registro de cambios informa 127/127 en una ejecución anterior; no se certifica ese resultado para HEAD. |
| `bash tests/network-lab/run-e2e.sh` ([script](../../tests/network-lab/run-e2e.sh), [arnés](../../tests/network-lab/e2e-cp.sh)) | Mismas imágenes/cachés, Docker y runner `--privileged`; `CARGO_NET_OFFLINE=true`. Tampoco reconstruye `atenea-lab`. | Control plane/PostgreSQL/Dex reales y agentes TUN; políticas TCP/UDP, aislamiento de dispositivo nuevo y comportamiento de la regla inversa. Esta no equivale a firewall de seguimiento de conexiones: el arnés comprueba también tráfico inverso no limitado por puerto. |
| `bash tests/stack/rollback-0008.sh` ([script](../../tests/stack/rollback-0008.sh)) | Git con tag `v0.1.5`, Docker, Python 3 y directorio temporal montable por Docker. Imágenes/cachés locales: `atenea-lab:latest`, `atenea-panel-lab:latest`, PostgreSQL y runtime distroless por digest fijado en el script. Compila N y N+1 offline; crea/retira un worktree temporal. Variables opcionales reales: `UPGRADE_BUILDER_IMAGE`, `UPGRADE_RUNTIME_IMAGE`, `UPGRADE_PROBE_IMAGE`. | Arranque normal de N sobre 0008 y datos nuevos, integridad de historial/filas y operaciones HTTP, sin down migration. El migrador antiguo se rechaza: no es lo mismo que arrancar N normalmente. [Informe](../operations/v0.1.6.md): 58/58, ejecución histórica, no repetida aquí. |
| `bash tests/e2e-panel/run.sh` ([script](../../tests/e2e-panel/run.sh)) | Docker, pnpm/Node; ejecuta `pnpm install --frozen-lockfile`, build web y **reconstruye** `atenea-lab`/`atenea-panel-lab` (puede necesitar red). PostgreSQL 17/Dex y cachés Cargo/pnpm; runner `--privileged`. Escribe `apps/web/test-results`. | Chromium/Playwright contra panel real, OIDC, CP y dos agentes tras NAT; flujos administrativos bloques A/B. [Informe](../operations/v0.1.6.md): 3/3 anterior, no repetido aquí. |

### 6.2 Infraestructura: N → fallo tras 0008 → rescue → N+1

Fuentes **VERIFICADO — lectura**: [`rehearsal-images.sh`](../../tests/infra/rehearsal-images.sh), [`run.sh`](../../tests/infra/run.sh), [`in-container.sh`](../../tests/infra/in-container.sh). Se necesita Docker con **cgroup v2**, systemd/Docker anidado, imagen de herramientas `atenea-infra:local`, imágenes base locales de los tres servicios, `atenea/tools:local`, `postgres:17`, `atenea-infra-pin/haproxy:pinned`, `atenea-infra-pin/caddy:pinned`, Dex `v2.41.1`, `atenea-lab` y cachés Cargo del laboratorio ya pobladas. El preparador usa `/private/tmp`: esa ruta debe existir y poder montarse en Docker (implementación orientada al anfitrión macOS). No descarga bases; compila binarios release offline y crea etiquetas de ensayo.

Ejemplo **INFERIDO** de composición de las interfaces reales: prepare primero un worktree de N (aquí v0.1.5) y use el árbol actual como N+1:

```sh
git worktree add --detach /tmp/atenea-audit-n v0.1.5
bash tests/infra/rehearsal-images.sh /tmp/atenea-audit-n audit-n
bash tests/infra/rehearsal-images.sh . audit-next
INFRA_CONTROL_PLANE_IMAGE=atenea-rehearsal/control-plane:audit-n \
INFRA_COORDINATOR_IMAGE=atenea-rehearsal/coordinator:audit-n \
INFRA_RELAY_IMAGE=atenea-rehearsal/relay:audit-n \
INFRA_NEXT_CONTROL_PLANE_IMAGE=atenea-rehearsal/control-plane:audit-next \
INFRA_NEXT_COORDINATOR_IMAGE=atenea-rehearsal/coordinator:audit-next \
INFRA_NEXT_RELAY_IMAGE=atenea-rehearsal/relay:audit-next \
INFRA_N_COMMIT="$(git rev-parse v0.1.5)" \
INFRA_NEXT_COMMIT="$(git rev-parse HEAD)" \
bash tests/infra/run.sh
git worktree remove /tmp/atenea-audit-n
```

Use una ruta temporal libre y anote cambios sin commit: el preparador copia el árbol local, aunque la identidad embebida procede de HEAD. `INFRA_IMAGE` cambia la imagen de herramientas; las tres `INFRA_*_IMAGE` fijan N; las tres `INFRA_NEXT_*_IMAGE` deben aportarse juntas para activar el ensayo real N→N+1; sin ellas no se demuestra ese recorrido. `INFRA_N_COMMIT`/`INFRA_NEXT_COMMIT` activan comparación de identidad informada. `KEEP_INFRA=1` conserva el contenedor para diagnóstico y exige retirarlo después; no es el modo normal.

El arnés usa Ansible/Compose reales, fuerza fallo después de migrar 0008, comprueba rescue de imágenes/mapa/entorno a N conservando esquema y datos, y reintenta N+1 con coordinator/relay reales. No demuestra agentes ni restauración de datos desde backup ni comportamiento del piloto. **SIN COMPROBAR aquí:** preparación de imágenes, ejecución y resultados.

### 6.3 Selector de cambios de CI (sin Docker)

[`scripts/ci-changes.sh`](../../scripts/ci-changes.sh) recibe `EVENT`, `BEFORE` (push), `BASE_SHA` (PR), `SHA` (por defecto HEAD) y opcionalmente `GITHUB_OUTPUT`. Emite `native=true|false` y `desktop=true|false`; ante evento manual/desconocido o base inválida elige ambos. Requiere Git y shell POSIX.

```sh
EVENT=workflow_dispatch sh scripts/ci-changes.sh
EVENT=push BEFORE=a0f8ac7 SHA=HEAD sh scripts/ci-changes.sh
sh tests/ci/changes.sh
```

La prueba [changes.sh](../../tests/ci/changes.sh) crea un repositorio temporal y commits sintéticos (no commits en el repositorio de producto): comprueba rutas, borrados/renombrados, múltiples commits, bases inválidas y eventos. `scripts/check.sh` la incluye. **VERIFICADO — lectura; SIN COMPROBAR ejecución aquí.** El selector no reactiva triggers automáticos: actualmente solo `workflow_dispatch` lanza la CI.

## 7. Release: construir y verificar

Ensayo local sin red, ya instaladas las imágenes/volúmenes (ver [release.md](../operations/release.md#ensayo-local-sin-red)); `dist/` debe no existir o estar vacío:

```sh
bash tests/release/run.sh
```

Internamente (VERIFICADO en [`tests/release/run.sh`](../../tests/release/run.sh)): `scripts/release/build-local.sh`, claves Ed25519 efímeras, `manifest.py create`, `verify.sh`, reproducibilidad con `SOURCE_DATE_EPOCH`, SBOM, `tests/release/tamper.py` (cinco rechazos) e instalación del `.deb`. Declara «SIN COMPROBAR: GHCR/Releases reales, digest de registro, .pkg/ZIP de runners, clave real».

Verificación **exacta** de un release con clave aportada ([release.md](../operations/release.md#firmar-y-verificar-con-clave-aportada)):

```sh
export ATENEA_BUILD_COMMIT=$(git rev-parse HEAD)
export SOURCE_DATE_EPOCH=$(git show -s --format=%ct HEAD)
bash scripts/release/verify.sh dist /ruta/confiable/release.pub "$ATENEA_BUILD_COMMIT" 0.1.5
```

(`0.1.5` es la versión actual de `apps/cli/Cargo.toml`; el auditor debe ajustar commit/versión al artefacto concreto, no al HEAD arbitrario.) Aporte la clave pública **obtenida por un canal independiente**, commit y versión esperados. El verificador recibe exactamente cuatro argumentos; comprueba firma/manifiesto, no confianza en el origen de la clave. La documentación de [release](../operations/release.md) y la historia registran publicaciones v0.1.0–v0.1.5; ya no se afirma ausencia de releases. **SIN COMPROBAR aquí:** descarga de artefactos publicados, firmas de esas publicaciones, `gh attestation verify` y correspondencia con el piloto. Firma de manifiesto Ed25519 no equivale a firma nativa ni notarización de `.pkg`/ZIP.

## 8. No revisado ni probado (para el auditor)

- Los resultados históricos Docker Mac arm64 no representan un VPS. Hay documentación posterior de [piloto real](../operations/infraestructura.md) con **tres equipos**, Ansible/Compose y [copias/restauración](../operations/copias-y-recuperacion.md); es evidencia leída, no ejecución propia ni prueba de todos los escenarios de laboratorio.
- Runners GitHub macOS/Windows no son hardware físico; firma nativa/notarización y token hardware siguen sin acreditarse.
- NAT domésticos distintos/CGNAT, roaming, MTU real y matrices completas entre plataformas: no se presumen cubiertos por el piloto. Una comprobación real puntual no generaliza a otras topologías.
- Auditoría independiente no contratada ni revisión criptográfica externa acreditada.
- **Soak ≥72 h sigue sin hacerse:** el intento del 2026-10-27 se invalidó por el Mac fuera de casa; el nuevo está programado para el 2026-10-31, no es evidencia de ejecución. Fuzz continuado no realizado.
- **SIN COMPROBAR aquí, inventario completo de ejecuciones no realizadas:** instalación/setup, compilación/formato/clippy/tests Rust (raíz y desktop), suites/build web/desktop, PostgreSQL real, vectores Python/Rust, fuzz, todos los laboratorios Docker/macOS/Windows y ensayos nuevos de §6.1–6.2, selector/prueba CI de §6.3, workflows remotos, release/atestaciones, snapshot/verificador de snapshot. La única verificación ejecutada en esta actualización es el comprobador documental y búsqueda de higiene; no valida el producto.

## 9. Snapshot del paquete y comprobador documental

Sin red ni compilación del producto (requiere Git, Python 3.12+ y `cargo metadata --offline --locked` ya posible en local). Desde la raíz, el orden previsto es snapshot, verificador, segunda pasada idéntica y comprobador al final ([detalle y SHA-256 registrado](README.md#snapshot-sin-red-y-verificación)):

```sh
scripts/audit/snapshot.sh && scripts/audit/verify-snapshot.sh
scripts/audit/snapshot.sh && scripts/audit/verify-snapshot.sh   # el SHA-256 de dist/audit/source.tar.gz debe coincidir
shasum -a 256 dist/audit/source.tar.gz
python3 scripts/audit/check-docs.py
```

El snapshot archiva **HEAD**, no cambios sin commit; `SHA256SUMS` es integridad, no firma. El comprobador solo revisa enlaces/higiene de `docs/audit/*.md`.

Para esta actualización se ejecutan solo `python3 scripts/audit/check-docs.py` y la búsqueda de patrones prohibidos del paquete (**VERIFICADO — ejecución local**, resultado en la entrega). Los comandos de snapshot anteriores son la receta existente, **SIN COMPROBAR aquí**; archivar HEAD no incluye estos documentos editados sin commit.
