# Releases verificables — fase 9

Un release es el manifiesto Ed25519 de ADR-009 §6, no una etiqueta Docker.
La clave de releases **no es la raíz** de configuración de ADR-005. El
responsable debe decidir su identidad, custodia y canal de distribución de la
clave pública antes de publicar. No hay clave real ni clave privada de prueba en
el repositorio; las pruebas generan claves efímeras fuera del árbol.

## Ensayo local sin red

Requisitos ya instalados: Python 3.11+, OpenSSL 3 con Ed25519, Docker Linux con
cgroup v2, imágenes `atenea-lab`, `atenea-infra:local`,
`atenea/{control-plane,coordinator,relay}:local`, volúmenes `atenea-lab-target`
y `atenea-cargo-registry`. No se instala ninguna herramienta externa de SBOM o
firma. El ensayo no usa syft, cosign, minisign ni cargo-cyclonedx.

```sh
# dist/ debe estar vacío o no existir; archivar el ensayo anterior, no mezclarlo.
bash tests/release/run.sh
```

El runner compila los cinco binarios Linux con Rust 1.99.1 ya instalado,
`CARGO_BUILD_JOBS=2`, `--offline --locked` y Docker `--network none`. Los binarios
imprimen `nombre versión (commit)` con `--version`/`-V`; sin
`ATENEA_BUILD_COMMIT` una compilación de desarrollo informa `unknown`.
La versión del release se toma de `apps/cli/Cargo.toml`; la comprobación de los
cinco ejecutables impide mezclar versiones de los paquetes. El commit se toma
de HEAD. **El ensayo admite el árbol de trabajo sin commit**: demuestra el
mecanismo con las fuentes actuales, no certifica que un árbol modificado sea
idéntico a HEAD. Los runners construyen el checkout del commit del evento.

`build-local.sh` reutiliza las imágenes runtime locales, copia los ejecutables
recién compilados y añade `org.opencontainers.image.revision`. Conserva el panel
y las capas de las bases locales: **no demuestra una recompilación del panel**.
Las imágenes originales no se retaggean ni borran. Los tres Dockerfile de
`deploy/images/` también reciben `ARG ATENEA_BUILD_COMMIT`, lo propagan al
compilador y etiquetan la revisión para las compilaciones completas del runner.
En local se ensaya el corte offline mediante Dockerfiles efímeros; no se ejecutan
las etapas npm/pnpm de los Dockerfile completos, que podrían necesitar red.

Se construye un `.deb` real con compresión xz mediante
`packaging/deb/build.sh`. Su control contiene `Version` y `X-Atenea-Commit`.
`tests/packaging/run.sh --release DIST COMMIT` instala exactamente ese `.deb`
en Debian con systemd, comprueba el commit/versiones de los dos binarios
instalados y la unidad. No inicia el servicio: la configuración privada aún no
existe. La base de infraestructura no incluye systemd-resolved; no se descarga.
Este modo no sustituye el ensayo completo de DNS/TUN de `tests/packaging/run.sh`
sin argumentos, ni afirma haberlo repetido.

**No se fabrican `.pkg` ni ZIP de Windows en local.** Esos instaladores se
construyen en sus runners. El `.pkg` contiene
`/usr/local/share/atenea/BUILD-INFO.json`; el ZIP contiene `BUILD-INFO.json`.
Los runners comprueban `--version` antes de empaquetar y adjuntan binarios,
metadatos y SBOM de cada plataforma. Es posible aportar esos ficheros a `dist/`
antes de generar el manifiesto. No copiar paquetes de otro commit ni añadir
ficheros después de firmar.

El runner deja `dist/` (ignorado por Git), elimina claves temporales, contenedores
y etiquetas propias. Conserva los volúmenes de compilación y la caché normal de
BuildKit; no ejecuta prune ni toca recursos de otros ensayos. Las claves efímeras
se eliminan tras verificar: ese manifiesto no es una atestación de producción
reutilizable ni debe distribuirse como tal.

## SBOM e identidad de las imágenes

`scripts/release/sbom.py` produce CycloneDX **1.5 JSON** y comprueba sus campos
obligatorios, identidades únicas, hashes y referencias de dependencias:

- Rust: `cargo metadata --offline --locked --format-version 1 --filter-platform <triple>`,
  junto con `Cargo.lock`, aporta nombres, versiones, checksums SHA-256 del registro,
  expresiones de licencia declaradas y grafo de dependencias. **Un SBOM por
  objetivo** (`atenea:target`) y **solo producción**: se parte de los paquetes
  que se distribuyen (`--root`) y no se siguen aristas exclusivas de desarrollo
  (`dev-dependencies`). Los crates locales no tienen checksum de registro. Es el
  grafo resuelto para ese triple, no una afirmación de que todas las features se
  enlacen en cada ejecutable. El SBOM de Windows añade **Wintun** (`--wintun`;
  versión y SHA-256 salen de `packaging/windows/get-wintun.ps1`, que es quien lo
  verifica al instalar) y el de Linux las **dependencias de ejecución del `.deb`**
  (`--deb-depends`, de `packaging/deb/build.sh`; sin versión resuelta: son
  restricciones). La cadena de herramientas queda en `BUILD-INFO.json`.
- Panel: parser propio del lock pnpm v9, sin ejecutar pnpm; incluye versiones,
  integridades SHA-512/SHA-256 y dependencias/optionalDependencies de snapshots.
  No inventa licencias ausentes del lock; se consulta el inventario de licencias
  del proyecto por separado. Otro formato de lock requiere adaptar el parser.
- Imágenes: `docker create --network none` + `docker export`, leyendo
  `/var/lib/dpkg/status`, `status.d` de distroless o `/lib/apk/db/installed`.
  Añade hashes de ejecutables regulares en los directorios binarios; los
  ejecutables sin metadata declaran `unversioned`, no una versión inventada.
  Los componentes Rust/panel de una imagen se vinculan también por sus SBOM
  de fuentes en el manifiesto. No descarga ni arranca la imagen para inventariar.

`scripts/release/images.py` requiere exportación Docker con layout OCI
(`index.json` y `blobs/`). Conserva el archivo Docker y extrae **los bytes del
manifiesto OCI real** de la plataforma y las capas observadas en Docker. Distingue:

- `id`: ID observado por Docker (según el motor puede identificar el índice
  containerd, no necesariamente la configuración);
- `config_digest`: digest de la configuración referida por el manifiesto;
- `manifest_digest`: digest del manifiesto de la plataforma exportado, verificado
  contra el blob del archivo OCI; no es un hash inventado a partir de una etiqueta;
- `registry_digest`: `null` mientras no se haya publicado. El job protegido lo
  sustituye por `repositorio@sha256:…` tras push. Puede ser un índice de registro,
  distinto del manifiesto de plataforma local. Para desplegar use esta referencia
  completa por digest, nunca el ID como si fuera digest de registro.

## Firmar y verificar con clave aportada

Conservar la clave privada fuera del repo y pasarla por fichero con permisos
restrictivos. `SOURCE_DATE_EPOCH` es obligatorio al crear el manifiesto: su fecha
UTC depende solo de ese valor. La serialización JSON y el orden de hashes son
estables para los mismos ficheros de entrada; **no se afirma reproducibilidad
bit a bit de compiladores, instaladores o archivos Docker**.

```sh
export ATENEA_BUILD_COMMIT=$(git rev-parse HEAD)
export SOURCE_DATE_EPOCH=$(git show -s --format=%ct HEAD)
# Con dist/ ensamblado y, si procede, los paquetes de los runners ya presentes:
python3 scripts/release/manifest.py create dist /ruta/privada/release.key "$ATENEA_BUILD_COMMIT" 0.1.0
bash scripts/release/verify.sh dist /ruta/confiable/release.pub "$ATENEA_BUILD_COMMIT" 0.1.0
```

La firma binaria es `release-manifest.json.sig`, Ed25519 con
`openssl pkeyutl -rawin`. El verificador exige una clave Ed25519 externa, la
versión y el commit **esperados por el consumidor**, verifica primero la firma y
luego todos los tamaños/SHA-256 de artefactos y SBOM, identidades de SBOM y
BUILD-INFO, identidad del control Debian y del ZIP, digests de imágenes y
`SHA256SUMS`. Rechaza ficheros extra, rutas fuera de dist y enlaces simbólicos.
No ejecute Python con `-O`/`PYTHONOPTIMIZE`: el comprobador lo rechaza.

`SHA256SUMS` se deriva de los hashes firmados; no incluye el propio manifiesto ni
su firma (evita una referencia circular). Cambiarlo también hace fallar la
verificación. La clave pública recibida junto al artefacto **no es confianza**:
obtenga su huella por un canal independiente controlado por el responsable.

La integración de un token hardware compatible con Ed25519 queda **pendiente**
de elección por el responsable (ADR-005). El fichero actual no se sustituye por
una clave de raíz, ni se inventa un provider PKCS#11: tras elegir token/provider
se deberá adaptar exclusivamente la operación de firma y ensayar su semántica
raw Ed25519. La verificación por clave pública seguirá siendo independiente.

## Workflow, aprobación y publicación

`.github/workflows/release.yml` se dispara por tags `v*` o manualmente. El disparo
manual **solo construye** (sirve de ensayo): `publish` exige un push de tag `v*`.
Los jobs Linux/macOS/Windows construyen y suben artefactos internos sin publicar,
con el toolchain de `rust-toolchain.toml`, y cada uno **atesta la procedencia** de
sus ficheros (`actions/attest-build-provenance`, permisos `id-token` y
`attestations` solo en esos jobs). El runner Linux sí puede descargar
dependencias para el build completo; esto no es parte del ensayo offline del Mac.
Las imágenes se construyen con `--provenance=false --sbom=false`: con el almacén containerd, BuildKit adjunta por defecto un manifiesto de atestación que convierte la imagen en un índice, y GHCR rechazó (403) el `docker push` de ese índice en el primer intento de publicación. La procedencia ya se atesta por fichero con `actions/attest-build-provenance` y el SBOM se genera con `scripts/release/sbom.py`, así que la atestación de BuildKit era redundante.

Todas las acciones, también `actions/upload-artifact` y `download-artifact`, se
fijan por SHA completo (comentario con la etiqueta); Dependabot propone las
actualizaciones. Cada job declara sus `permissions` mínimos; el valor por defecto
del workflow es ninguno.

### Requisitos del environment `release` (verificables)

El repositorio no puede aplicarlos; `publish` los **comprueba** antes de tocar el
secreto (`scripts/release/check-environment.py`, con `actions: read`) y falla si
no se cumplen. Settings > Environments > release:

- [ ] Revisores obligatorios (al menos uno). «Prevent self-review» **no** se exige: con un único mantenedor, una segunda cuenta propia no aporta separación real (decisión del responsable, 2026-10-27). Riesgo residual: quien controla la cuenta del mantenedor puede lanzar y aprobar un release; lo limitan la política de solo tags `v*`, el ruleset que impide mover o borrar esos tags y la atestación de procedencia por fichero y commit, no una segunda persona.
- [ ] Deployment branches and tags: política personalizada **solo con tags `v*`**
      (y reglas de protección/ruleset de tags `v*` en el repositorio).
- [ ] El secreto `ATENEA_RELEASE_ED25519_PEM` es un secreto **del environment**, no
      del repositorio ni de la organización.

La comprobación es un detector, no la barrera: la barrera es la configuración del
environment. Un workflow modificado en una rama no puede usar el secreto si la
política de despliegue solo admite tags `v*` protegidos.

### Procedencia del commit

Antes de usar el secreto, `publish` ejecuta `gh attestation verify` sobre **cada**
fichero de `dist/` exigiendo este repositorio, este workflow como firmante, el
commit (`--source-digest $GITHUB_SHA`) y la ref del tag, sin runners propios. Un
fichero cambiado entre el job de construcción y la firma, o construido desde otro
commit, no tiene atestación válida y el release se aborta. Además `manifest.py`
comprueba que commit y versión de `BUILD-INFO`, ZIP, `.deb` y SBOM coinciden. La
firma Ed25519 del manifiesto cubre commit y SHA-256 de cada artefacto, por lo
que atestigua artefactos cuya procedencia se verificó en ese commit.

**Solo `publish`** tiene `packages: write`, `contents: write`, login GHCR,
`docker push`, firma y `gh release create`. Primero publica imágenes, captura sus
digests reales, ensambla y firma el manifiesto final incluyendo esos digests,
verifica y adjunta todos los ficheros a GitHub Releases. No muta un manifiesto ya
firmado. Requiere un tag `vVERSION` existente (`--verify-tag`), también al
invocarlo manualmente; el tag debe apuntar al mismo commit y versión del release,
comprobados antes de hacer push de las imágenes.
Una interrupción entre push y creación del release puede dejar imágenes por
commit en GHCR sin release: no desplegarlas hasta tener el manifiesto firmado.

## Evidencia y límites

El ensayo cubre compilación Linux offline, `.deb` instalado en Debian/systemd,
tres imágenes locales, SBOM validados, firma/verificación positiva y cinco
rechazos: byte de artefacto cambiado, SBOM alterado, otra clave, commit distinto y
manifiesto truncado. La salida real completa se entrega con la ejecución final.

Ejecución final local: salida 0, **37,12 s**, versión `0.1.0`, commit de ensayo
`9195241c4310bc97a14736af2488ce5bfa7bc9f4`. Firma Ed25519 y reproducción exacta
de manifiesto/firma correctas; cada imagen inventarió **14 paquetes Debian** más
sus ejecutables. Los cinco casos negativos fueron rechazados y se instaló el
`.deb` recién construido. No se detectaron descargas; el inventario Docker al
terminar no mostró contenedores ni etiquetas `atenea-release-*`.
En macOS: `cargo fmt --all`, clippy de los cinco paquetes con
`--offline --locked --all-targets -- -D warnings` y **155 pruebas correctas**,
con `CARGO_BUILD_JOBS=2`. No se repitió clippy/test Linux del workspace.

### Publicación real de `v0.1.0` (2026-10-27)

El tag firmado `v0.1.0` se publicó con autorización expresa del responsable tras **cuatro intentos**: el primero falló en el job `linux` porque `dpkg-deb --compression=xz` no existe en el `dpkg-deb` 1.21 del runner (ahora `-Zxz`); el segundo y el tercero, en `publish`, porque GHCR rechazó con 403 el manifiesto de la imagen (la etiqueta apuntaba a un manifiesto de atestación de BuildKit; el tercero ya llevaba la etiqueta OCI de origen); el cuarto construyó con `--provenance=false --sbom=false` y publicó. Los intentos fallidos no dejaron release; sus paquetes huérfanos de GHCR se borraron. El tag `v0.1.0` se recreó tres veces sobre commits distintos mientras no existía release; hoy apunta a `107fd0d` y el ruleset impide moverlo o borrarlo.

Resultado: GitHub Release con **33 ficheros** (binarios Linux/macOS/Windows, `.deb`, `.pkg` y ZIP sin firma nativa, SBOM, manifiesto y firma) y tres imágenes públicas en GHCR por digest (`control-plane`, `coordinator`, `relay`). `scripts/release/verify.sh` pasó con la clave pública del responsable obtenida fuera del release (firma Ed25519, identidad, artefactos, SBOM y `SHA256SUMS`); el manifiesto de cada imagen se lee sin credenciales. Los digests de registro están en `images.json` del release. El job `publish` ejecutó el comprobador del environment y `gh attestation verify` por fichero.

**SIN COMPROBAR:** que el 403 de GHCR se debiera a la atestación de BuildKit (se deduce del contenido del registro, no se reprodujo en local); firmas de plataforma/notarización macOS y Authenticode Windows (los instaladores van sin firmar); token hardware para la clave de firma; la instalación de los artefactos publicados en un host real. La clave de firma del release está en un fichero local del responsable, sin passphrase ni copia de seguridad verificada. Un solo mantenedor aprueba y publica (ADR-009). `0.1.0` es una versión de piloto: no es apta para datos sensibles hasta la auditoría independiente.

### Publicación real de `v0.1.1` (2026-10-27)

El tag firmado `v0.1.1` (commit `5e3a268`) se publicó con autorización expresa del responsable, **al primer intento**: los cuatro jobs de construcción y el de ensamblado pasaron y `publish` esperó la aprobación del entorno `release`, que dio el propio responsable (un solo mantenedor; ADR-009). La ejecución de la API de aprobación falló dos veces por mi formato de petición (la API exige enteros, no texto) antes de aceptarla; no afectó al contenido publicado.

Contenido: las cinco crates de binario (`atenea`, `atenea-agent`, `atenea-control-plane`, `atenea-coordinator`, `atenea-relay`) suben a `0.1.1` (la versión sale de `apps/cli/Cargo.toml` y cada binario debe imprimir la misma); el resto de crates siguen en `0.1.0`. Respecto a `v0.1.0` incluye los nombres de dispositivo (ADR-012: migración `0006` del control plane, endpoint `GET /api/v1/device/peers`, nombre del sistema en el agente y `enroll --name` opcional) y las direcciones del túnel en el estado del agente. **No incluye** la aplicación de escritorio (ADR-011, tiene su propio workspace de Cargo y no entra en este flujo) ni la imagen de copias `postgres-backup`, que sigue construyéndose en el host.

Resultado: GitHub Release con **33 ficheros** (binarios Linux/macOS/Windows, `.deb`, `.pkg` y ZIP sin firma nativa, SBOM, manifiesto y firma) y tres imágenes públicas en GHCR por digest, todas con la versión `0.1.1` y el commit del tag. Se comprobó fuera de CI: `scripts/release/verify.sh` pasó con la clave pública del responsable obtenida por otro canal (firma Ed25519, identidad, artefactos, SBOM y `SHA256SUMS`) y el manifiesto de cada imagen se lee sin credenciales (HTTP 200). Los digests de registro están en `images.json` del release.

**Sin comprobar:** el despliegue de estos artefactos en un host real y la migración `0006` sobre una base con datos; firmas de plataforma (los instaladores de macOS y Windows siguen sin firmar ni notarizar); que el binario del agente de macOS arranque con la configuración del piloto. `0.1.1` sigue siendo una versión de piloto: no es apta para datos sensibles hasta la auditoría independiente.

### Publicación real de `v0.1.2` (2026-10-28)

El tag firmado `v0.1.2` (commit `9e80d7b`) se publicó con autorización expresa del responsable, al primer intento y con la aprobación del entorno `release` correcta a la primera (cuerpo JSON con `environment_ids` numérico). Solo cambia la versión de las cinco crates de binario y dos piezas de empaquetado que salieron del despliegue real de `v0.1.1`:

1. **`.deb`:** `prerm upgrade` recuerda si el agente estaba activo y `postinst` lo vuelve a arrancar solo en ese caso (antes `dpkg -i` sobre un agente en marcha lo dejaba **parado**; ocurrió en el nodo del piloto). Una instalación nueva sigue sin arrancar nada.
2. **`.pkg` de macOS:** el lanzador crea `/var/run/atenea` como 0750 root:`status_gid` cuando la configuración define `status_gid`, de modo que el estado se lea sin `sudo`.

Resultado: 33 ficheros y tres imágenes públicas en GHCR por digest. Verificado fuera de CI: `scripts/release/verify.sh` pasó con la clave pública del responsable obtenida por otro canal; los tres manifiestos de GHCR se leen sin credenciales (HTTP 200) y llevan la versión `0.1.2`; y los scripts `prerm` y `postinst` extraídos del `.deb` publicado y `atenea-launchd.sh` extraído del `.pkg` publicado son **idénticos byte a byte** a los del tag.

**Sin comprobar:** el comportamiento de ambos arreglos con los paquetes publicados instalados en un host real. El del `.deb` se probó antes con un `systemctl` simulado (no con PID 1 ni con este paquete); el del lanzador se observó en un Mac real bajo `launchd` pero con los binarios de `v0.1.1` y el lanzador instalado con `install-launchd.sh`, no desde este `.pkg`. `0.1.2` sigue siendo una versión de piloto: no es apta para datos sensibles hasta la auditoría independiente.

