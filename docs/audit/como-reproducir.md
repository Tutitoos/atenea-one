# Cómo reproducir — guía para auditor externo

Referencia de esta guía: `git rev-parse HEAD` en la copia que se audita (al redactar, `a0f8ac71b5e83607603c45b46113a2aa0ea607b6`; fíjese el hash del árbol revisado, no este). La [matriz de aceptación](../operations/matriz-de-aceptacion.md) tiene corte documental `d9ab7ec…`; su CI verde se atribuye solo a ese commit.

**Este documento no afirma que el software sea seguro, esté auditado ni listo para producción.** Describe cómo reproducir comprobaciones. No ejecuté suites Rust/web, fuzz, Docker, laboratorios ni release; solo snapshot, verificador y comprobador documental ([apartado 9](#9-snapshot-del-paquete-y-comprobador-documental)). Los comandos se transcriben de los scripts enlazados salvo los marcados INFERIDOS.

Etiquetas: **VERIFICADO** (leído en el repositorio al redactar, sin ejecutar), **PARCIAL**, **SIN COMPROBAR**, **INFERIDO** (deducido del código/script, no leído en un resultado). «Ejecutado» solo aparece donde un documento enlazado lo informa, con su propio alcance.

## 1. Clonar y requisitos

```sh
git clone https://github.com/Tutitoos/atenea-one.git
cd atenea-one
git rev-parse HEAD            # anotar; comparar con el commit bajo revisión
```

- URL tomada de los enlaces de la [matriz](../operations/matriz-de-aceptacion.md) (VERIFICADO). Que el clon anónimo funcione está **PENDIENTE/SIN COMPROBAR** según esa matriz (caso «Repositorio público»).
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
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
pnpm install --frozen-lockfile
pnpm -r --if-present run lint
pnpm -r --if-present run typecheck
pnpm -r --if-present run test
pnpm --filter @atenea/web run build
```

Control plane con PostgreSQL real (CI `check`; requiere Docker o `ATENEA_TEST_DATABASE_URL`, usa contenedor `postgres:17`):

```sh
apps/control-plane/tests/run.sh
```

Estos comandos pueden **descargar dependencias** (cargo/pnpm) la primera vez. Los clientes macOS/Windows en CI usan subconjuntos de paquetes (`atenea-tunnel atenea-agent atenea-cli atenea-testpeer`, más `atenea-windows`); ver [ci.yml](../../.github/workflows/ci.yml).

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

## 5. Fuzzing (13 objetivos)

Fuente: [`fuzz/README.md`](../../fuzz/README.md), [`fuzz/run.sh`](../../fuzz/run.sh), [`fuzz/Dockerfile`](../../fuzz/Dockerfile).

```sh
fuzz/run.sh                  # 13 objetivos, 60 s cada uno
fuzz/run.sh 300              # 300 s cada uno
fuzz/run.sh 60 hex_decode    # un objetivo
```

Objetivos en [`fuzz/fuzz_targets/`](../../fuzz/fuzz_targets) (13 ficheros, VERIFICADO): `agent_engine`, `cert_bundle_state_parse`, `config_v2_parse`, `datagram_parse`, `device_config_parse`, `handshake_init`, `hex_decode`, `ip_classify`, `relay_parse`, `replay_window`, `session_decrypt`, `signed_config_verify`, `stun_parse`. Qué comprueba cada uno: tabla del README del fuzz.

- Requiere Docker; nightly + `cargo-fuzz` dentro de la imagen `atenea-fuzz`. `run.sh` construye la imagen (el `Dockerfile` hace `FROM rust:1.99-bookworm` y `cargo install cargo-fuzz`: **ese paso necesita red**, INFERIDO) y ejecuta los objetivos con `--network none` y `CARGO_NET_OFFLINE=true`, dependiendo de los volúmenes `atenea-fuzz-target` y `atenea-fuzz-cargo`. El repositorio no documenta cómo se llena `atenea-fuzz-cargo`; sin esa caché el arranque offline falla (INFERIDO de «falla en vez de descargar»).
- Salida ≠ 0 si hay fallo o regresión versionada. Semillas en `fuzz/seeds/` para 8 de los 13 objetivos; `fuzz/regressions/` contiene solo su README (sin reproductores versionados, VERIFICADO).
- Reproducir un fallo: comando del README (`cargo +nightly fuzz run <objetivo> regressions/<objetivo>/<fichero>`).
- Estado: la última ejecución completa informada (90 s × 13, Docker Mac arm64, 0 fallos) es **histórica y anterior al cutover wire-v2/relay-v2**; el propio README dice que no verifica los objetivos/semillas v2 y que no se ejecutó fuzzing en esa integración. Para v2: **SIN COMPROBAR**. No hay campaña de horas/días con corpus persistente (corpus no versionado).

## 6. Laboratorios Docker y privilegios

Todos exigen Docker; casi todos usan `--privileged` (namespaces, nftables, TUN, netem, cgroups). Ejecutar solo en VM/máquina desechable, no en el host de producción ([tests/packaging/run.sh](../../tests/packaging/run.sh) lo advierte). Privilegios VERIFICADOS por lectura de los scripts; «qué prueba» según su README/cabecera y la [matriz](../operations/matriz-de-aceptacion.md), no por ejecución mía:

| Laboratorio | Comando | Qué prueba | Privilegio observado |
| --- | --- | --- | --- |
| Red base | `tests/network-lab/run.sh` ([README](../../tests/network-lab/README.md)) | Dos agentes y un no autorizado en namespaces: ping/TCP/UDP íntegros IPv4/IPv6, MTU 1400, captura sin texto claro, replay/contador alterado, pérdida/reorden/latencia (netem), reinicio, limpieza | `docker run --privileged` |
| NAT directo / relay | `tests/network-lab/nat/run-e2e.sh`, `.../run-e2e-relay.sh` | Topologías NAT, directo, bloqueo P2P→UDP→TLS y vuelta, integridad por camino | `--privileged` |
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

- CI ([ci.yml](../../.github/workflows/ci.yml)) ejecuta: `scripts/check.sh`, control-plane con PostgreSQL, pnpm lint/typecheck/test/build, macOS (fmt, clippy, tests, `.pkg` sin firma y `tests/macos/run.sh`), Windows (clippy, tests, ZIP y `tests/windows/run.ps1`) y supply-chain (`cargo-deny`, `pnpm audit --prod`). **CI no ejecuta** los laboratorios Docker de red, stack, infra, backup, carga ni fuzz (VERIFICADO: no figuran en `ci.yml`).
- Los resultados de esos laboratorios son «LOCAL-V2» (Docker Desktop, Mac arm64, árbol previo a `d9ab7ec`, sin identidad byte a byte con él) y sus salidas `/tmp/v2-*.txt` **no están versionadas ni accesibles en un clon** ([matriz](../operations/matriz-de-aceptacion.md#cómo-leer-la-evidencia)). Un auditor no puede reproducir esas cifras sin ejecutarlas él mismo.

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
bash scripts/release/verify.sh dist /ruta/confiable/release.pub "$ATENEA_BUILD_COMMIT" 0.1.0
```

(`0.1.0` es la versión de `apps/cli/Cargo.toml`, VERIFICADO.) Cosas que el auditor debe aportar: la clave pública **obtenida por un canal independiente**, el commit y la versión esperados por el consumidor. El verificador comprueba firma, tamaños/SHA-256, SBOM, identidades, digests de imágenes y `SHA256SUMS`; no ejecute Python con `-O`. No existe clave real del proyecto en el repositorio. Verificación de un release publicado y `gh attestation verify` (workflow [release.yml](../../.github/workflows/release.yml), job `publish`): **SIN COMPROBAR**; no consta ningún release publicado ni tag creado.

## 8. No revisado ni probado (para el auditor)

- Docker en Mac arm64: todo resultado de laboratorio es de ese entorno (VM de 5 vCPU/7,75 GiB); no representa un VPS. No hay ejecución Linux x86_64 con Docker propia documentada fuera del runner `ubuntu-latest` de CI (que no ejecuta los laboratorios).
- Runners de GitHub (macOS/Windows) no son hardware físico; `.pkg`/ZIP sin firma nativa/notarización.
- Sin hardware real: NAT domésticos/CGNAT, roaming, MTU de redes reales, token hardware de raíz, despliegue en proveedor/Proxmox, DNS/ACME públicos.
- Sin auditoría independiente, sin revisión criptográfica externa ([matriz](../operations/matriz-de-aceptacion.md)).
- Soak ≥ 72 h no ejecutado ([rendimiento](../operations/rendimiento-y-presupuestos.md)); fuzz continuado no realizado.
- Sin ejecutar por mí: suites Rust/web, fuzz, Docker, laboratorios y release. Las rutas de comandos del apartado 3 son INFERIDAS. Sí se ejecutaron snapshot, verificador y comprobador documental (apartado 9, según el [README del paquete](README.md#snapshot-sin-red-y-verificación)); eso no valida el código.

## 9. Snapshot del paquete y comprobador documental

Sin red ni compilación del producto (requiere Git, Python 3.12+ y `cargo metadata --offline --locked` ya posible en local). Desde la raíz, el orden previsto es snapshot, verificador, segunda pasada idéntica y comprobador al final ([detalle y SHA-256 registrado](README.md#snapshot-sin-red-y-verificación)):

```sh
scripts/audit/snapshot.sh && scripts/audit/verify-snapshot.sh
scripts/audit/snapshot.sh && scripts/audit/verify-snapshot.sh   # el SHA-256 de dist/audit/source.tar.gz debe coincidir
shasum -a 256 dist/audit/source.tar.gz
python3 scripts/audit/check-docs.py
```

El snapshot archiva **HEAD**, no cambios sin commit; `SHA256SUMS` es integridad, no firma. El comprobador solo revisa enlaces/higiene de `docs/audit/*.md`.
