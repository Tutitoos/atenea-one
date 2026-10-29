# Paquete para auditoría independiente

**Estado: no apto para datos sensibles hasta esta auditoría.** Este paquete no es una afirmación de seguridad ni sustituye un informe independiente. La contratación/designación del auditor, resolución de críticos/altos y retest siguen **PENDIENTE DEL RESPONSABLE** en la [matriz de aceptación](../operations/matriz-de-aceptacion.md#casos-de-aceptación); política de comunicación: [SECURITY.md](../../SECURITY.md).

## Actualización del 2026-10-29

El paquete se redactó contra `a0f8ac7` (2026-10-26). Desde entonces el repositorio ha recibido **89 commits** (`git rev-list --count a0f8ac7..HEAD`), con **458 ficheros** cambiados y 31 561 inserciones frente a 920 supresiones (`git diff --shortstat a0f8ac7 HEAD`). El código de referencia de esta actualización es **`8d975a6`** (2026-10-29), último commit antes de actualizar estos documentos. El [snapshot](../../scripts/audit/snapshot.sh) archiva el commit que contiene esta actualización, no `8d975a6`: su `COMMIT` es la referencia exacta de la entrega.

**Lo que cambia respecto a lo que leen los documentos restantes** (comprobado con `git diff --name-status a0f8ac7 HEAD`; ver cada documento para el detalle):

- **Control plane:** migraciones `0005` a `0008`; unión por código y solicitudes de unión sin token (endpoint público), `POST /api/v1/device/leave` firmado por el dispositivo, nombres y rol de dispositivo (metadatos **sin firmar**), información del servidor, miembros con correo y alta por correo, renombrado de organización y red.
- **Agente y CLI:** varias redes por dispositivo (`atenea network …`), servicio del sistema por red (systemd y launchd), migración de la instalación heredada con scripts privilegiados, tres correcciones del motor de recuperación de rutas.
- **Aplicación de escritorio (`apps/desktop`)**, nueva: nuevo límite de confianza; las operaciones que cambian el sistema se ejecutan elevando (`osascript` en macOS, `pkexec` en Linux) sobre el CLI (ADR-011), y puede escribir un fichero de arranque de usuario.
- **Panel web:** rediseño completo (bloques A y B) con E2E real.
- **Decisiones:** ADR-006 y ADR-010 a ADR-016 (el índice está en [architecture/README.md](../architecture/README.md)). ADR-016 documenta, con medición, que la ACL sin estado deja pasar el tráfico inverso (TCP: solo el SYN; UDP e ICMP: sin límite de puertos).
- **Operación:** releases `v0.1.0` a `v0.1.5` publicados y verificados, despliegue real del piloto (tres equipos), CI **solo manual**.

**Sigue sin hacerse** (estado a 2026-10-29): el soak de 72 h (hubo un intento el 2026-10-27, inválido por condiciones del equipo; hay otro programado), la auditoría independiente, la firma nativa y notarización de instaladores, el *updater* de la aplicación de escritorio y la prueba de la v0.1.6 contra el piloto real (solo laboratorio).

**Referencias de línea.** Las referencias `fichero:Lnn` de los documentos que no se han recalculado siguen siendo de `a0f8ac7` y llevan esa marca. Las de los ficheros de producción modificados desde entonces pueden haberse desplazado más de lo que explica la cabecera de copyright.

**Firma.** La firma que había en `dist/audit/` (2026-10-27) **no valida** el contenido nuevo (comprobado con `openssl pkeyutl -verify`: *Signature Verification Failure*, como corresponde a otro `SHA256SUMS`). La firma de esta entrega la debe hacer el responsable con su clave; ver «Firma de la instantánea».

## Referencia y alcance temporal

Referencia obtenida con `git rev-parse HEAD` al generar el paquete: **`a0f8ac71b5e83607603c45b46113a2aa0ea607b6`** ([commit público](https://github.com/Tutitoos/atenea-one/commit/a0f8ac71b5e83607603c45b46113a2aa0ea607b6)). Redacción: **2026-10-26**. Esta referencia fija el código leído, no atribuye al commit los documentos y scripts nuevos todavía sin commit. El [snapshot](../../scripts/audit/snapshot.sh) vuelve a resolver HEAD y fija su commit/fecha en `COMMIT`, `SOURCE_DATE_EPOCH` y en el propio tarball: **solo incluye archivos versionados en ese commit**, nunca cambios sin commit ni archivos ignorados. Antes de entregar el paquete definitivo, el responsable debe registrar sus cambios y regenerar; aquí no se realiza commit ni publicación.

**Diferencias entre el código analizado (`a0f8ac7`) y la instantánea `53e7ed9`** (comprobado con `git diff --name-only a0f8ac7 53e7ed9`, 2026-10-26): este paquete (`docs/audit/`, `scripts/audit/`, `tests/vectors/`), los ADR-006 y 009 y sus enlaces, `SECURITY.md`, la fila de auditoría de la matriz de aceptación, la acción `actions/setup-node` de la CI (6.5.0 → 7.1.0, fijada por SHA) y `windows-service` (0.8.0 → 0.8.1, `crates/windows/Cargo.toml` y `Cargo.lock`). **No hay otros cambios en código de producción en ese tramo.** Los commits posteriores a `53e7ed9` (cabeceras de copyright en los ficheros fuente y el diseño de la fase 10, solo documentación) son cambios de formato y de documentos: véase la nota siguiente sobre el desplazamiento de líneas. Las otras nueve actualizaciones propuestas por Dependabot se cerraron sin fusionar (entre ellas `rand`, `sha2` y `toml`, que se evaluarán tras la auditoría).

**Cambios de código de producción posteriores a `a0f8ac7` y a la instantánea `53e7ed9`** (decididos por el responsable el 2026-10-26 a partir de dos diagnósticos de rendimiento y entregados en el commit `2fd928c`; la lista exacta es `git diff --name-only 53e7ed9 2fd928c -- apps crates`; los diagnósticos son el del [control plane](../operations/diagnostico-control-plane-escala.md) y el de [relay y reconexión](../operations/diagnostico-reconexion-y-ciclo-relay.md)). **Esta versión SÍ difiere del código leído por el resto del paquete:**

1. **Control plane:** migración aditiva `0005_ip_allocations_device_family.sql` (índice `(device_id, family) INCLUDE (addr)`; no cambia el contrato), `build.rs` con `rerun-if-changed=migrations` y pruebas.
2. **Agente, renovación anticipada del relay** (`apps/agent/src/{relay.rs,run.rs}`): pide una concesión antes de `not_after` y refresca la asignación existente con un `ALLOC` nuevo (el relay conserva el `alloc_id`); presupuestos de peticiones por par y por dispositivo, backoff con jitter, fail-closed si se rechaza; contadores `relay_renewal_attempts/ok/failed/throttled`. **Relay:** solo `Relay::bind_with_clock` (reloj inyectable; `bind` conserva su firma y su comportamiento). Sin cambios de formato de cable ni de contrato.
3. **Agente, reintento de la presencia tras la cuota del coordinator** (`apps/agent/src/{run.rs,coord.rs}`): conserva un único announce pendiente, backoff 2/4/8 s con jitter y separación mínima de 2 s, y acepta un `Retry-After` solo en segundos enteros ≤ 60 s. Sin cambios de cuotas ni de contrato.
4. **Agente, fuente de aleatoriedad del jitter del sondeo** inyectable en el motor (producción conserva `crypto::random_u32` y la misma fórmula 750–1250 ms).
5. **Todos los ficheros fuente** llevan además la cabecera de copyright (+2 líneas, descrita abajo). Las referencias de línea de este paquete siguen siendo de `a0f8ac7`: en los ficheros de los puntos 1–4 el desplazamiento es mayor que 2 líneas y **no se ha recalculado**.

Verificado tras estos cambios (2026-10-26/27, Docker en Mac arm64 y CI de GitHub): `check.sh`, clippy y 40 suites en Linux, suite del agente 100 ejecuciones y 40 bajo carga de CPU sin fallos, laboratorios de red, NAT, relay, subredes, stack y upgrade en verde, soak de 12 min sin recreación del relay (`relay_down`/`relay_ready` constantes; 3 renovaciones correctas), reconexión tras reinicio medida (a↔c 5,05–5,32 s frente a ≈20 s) y control plane con el índice `0005` medido con la misma matriz (a 10 000 dispositivos en 79 redes y 200 clientes, p95 de lectura 15,2 → 0,43 s y de emisión 19,5 → 1,41 s, 0 errores; no es un A/B aislado ni se hizo en un VPS, véase [rendimiento](../operations/rendimiento-y-presupuestos.md)). **Sigue sin hacerse:** el soak de 72 h.

**Desplazamiento de líneas por las cabeceras de copyright.** Todas las referencias de línea de este paquete (p. ej. `identity/lib.rs L75`) son del commit `a0f8ac7`. En los commits posteriores que añaden la cabecera de dos líneas (`Copyright 2026 Tutitoos` y `SPDX-License-Identifier: Apache-2.0`) a los ficheros fuente `.rs`, `.ts`, `.tsx`, `.mjs`, `.js`, `.py`, `.sh` y `.ps1`, hay que **sumar 2 líneas** a cada referencia de esos ficheros (**2 después del shebang**, es decir, 3 en total, si el fichero empieza por `#!`). Las referencias no se han reescrito. Se comprueba con `git diff a0f8ac7 <commit> --stat -- <fichero>` (o `git diff a0f8ac7 <commit> -- <fichero>`: las dos primeras líneas añadidas, tras el shebang si lo hay, son la cabecera) y [`scripts/check-headers.py`](../../scripts/check-headers.py) enumera qué ficheros la exigen.

No confundir la referencia anterior con el corte de evidencia de la [matriz](../operations/matriz-de-aceptacion.md), que atribuye CI a `d9ab7ec` y resultados de laboratorio a árboles concretos con cambios locales. No hemos repetido esos laboratorios ni consultado GitHub para validar sus salidas.

## Orden recomendado de lectura

1. [Alcance y preguntas](alcance-y-preguntas.md): encargo, exclusiones, prioridades y entregables.
2. [Arquitectura para auditores](arquitectura-para-auditores.md): fronteras, flujos, claves, secretos y primitivas/versiones.
3. [Mapa del código](mapa-del-codigo.md): LOC, puntos de entrada, riesgo, tests/fuzz y `unsafe` de plataforma.
4. [Revisión interna](revision-interna.md): antecedentes sanitizados, correcciones y riesgos que permanecen abiertos. No equivale a retest independiente.
5. [Cómo reproducir](como-reproducir.md): prerrequisitos y comandos exactos para vectores, tests, fuzz, laboratorios y release.
6. [Evidencia y brechas](evidencia.md): qué acredita cada fuente y lo que no se ha revisado/probado; continuar con la [matriz original](../operations/matriz-de-aceptacion.md) sin sustituirla.
7. Contrastar con [T1–T30](../security/threat-model.md), [decisiones ADR](../architecture/README.md), [wire v2](../protocol/wire-v2.md), [relay v2](../protocol/relay-v2.md), [configuración v1](../protocol/signed-config-v1.md) y [v2](../protocol/signed-config-v2.md).

## Convención de evidencia

- **VERIFICADO — lectura**: existencia/contenido comprobado en archivos locales; no demuestra ejecución o ausencia de defectos.
- **VERIFICADO — ejecución local**: solo los comandos ejecutados expresamente al preparar este paquete (snapshot y comprobación documental, abajo).
- **PARCIAL**: hay evidencia, pero no satisface todo el criterio.
- **SIN COMPROBAR**: no se obtuvo evidencia suficiente ni se ejecutó el escenario aquí.
- **INFERIDO**: deducción del diseño/código, no medición.
- Las etiquetas **VERIFICADO EN LABORATORIO/CI** son atribuciones documentadas en la matriz, con sus límites y commit/árbol, no nuevas ejecuciones. Las afirmaciones técnicas se enlazan a fuentes; las preguntas no son hallazgos.

## Snapshot sin red y verificación

Requiere Git, Python **3.12 o posterior** (extracción `filter='data'`), Cargo/rustc y las dependencias ya disponibles localmente. Si existe rustup, resuelve los binarios instalados del toolchain fijado con `rustup which`, sin instalación automática; sin rustup, usa los binarios locales de PATH. Cargo usa `--offline --locked`. No instala herramientas ni obtiene dependencias. Ejecutar desde la raíz:

```sh
scripts/audit/snapshot.sh
scripts/audit/verify-snapshot.sh
shasum -a 256 dist/audit/source.tar.gz
```

Genera en `dist/audit/` (excluido por la regla existente `dist/` en [.gitignore](../../.gitignore)):

| Archivo | Contenido |
| --- | --- |
| `source.tar.gz` | `git archive` de HEAD, normalizado: nombres ordenados, uid/gid 0, propietarios vacíos, permisos 0644/0755, mtime del commit y gzip sin nombre/fecha variable. Incluye `AUDIT-SNAPSHOT.json` y `AUDIT-DEPENDENCIES.json`. |
| `COMMIT` | SHA completo del commit archivado. |
| `SOURCE_DATE_EPOCH` | Fecha del commit en segundos Unix, también aplicada al tar/gzip. |
| `dependencies.json` | `cargo metadata --offline --locked --format-version 1` del árbol archivado, condensado por nombre/versión/origen/licencia y dependencias; sin rutas absolutas. Incluye desarrollo/todas las plataformas; no es SBOM completo de pnpm, SO o Wintun. |
| `SHA256SUMS` | SHA-256 de los cuatro archivos anteriores. Integridad, **no firma ni prueba de autoría**. |

El [verificador](../../scripts/audit/verify-snapshot.sh) comprueba hashes, procedencia interna/externa, dependencias, orden, rutas, tipos y metadatos normalizados sin extraer ni ejecutar código del tarball. Puede recibir otro directorio como único argumento. La lista de dependencias se genera con manifiestos/lockfile del commit, no los del árbol de trabajo. El script rechaza enlaces y tipos especiales versionados para no extraer fuera del temporal. La reproducibilidad acreditada abajo es en este entorno; versión de Git/Python/zlib/Cargo puede requerir comparación adicional entre entornos.

**VERIFICADO — ejecución local, 2026-10-26:** dos ejecuciones consecutivas de `snapshot.sh`, cada una seguida de `verify-snapshot.sh`, terminaron correctamente y produjeron el mismo SHA-256 de `source.tar.gz`:

```text
3244f618a45a4a99acc774ad49c4570b96f745374a321293a47f4b0bb6d4f0e6
```

Ambas archivaron `a0f8ac71b5e83607603c45b46113a2aa0ea607b6`, sin los nuevos archivos de esta entrega. La evidencia de este ensayo es la salida local de los scripts; el binario generado permanece ignorado y no se presenta como artefacto publicado. Para compartirlo, transmitir también un hash esperado por un canal independiente; una modificación conjunta de tarball y `SHA256SUMS` no se detecta como falsificación.

### Firma de la instantánea (responsable)

`SHA256SUMS` solo da integridad. El responsable lo firma con una clave **Ed25519 propia**, generada y guardada fuera de este repositorio (`openssl genpkey -algorithm ed25519`); la misma operación que firma el manifiesto de release (`openssl pkeyutl -rawin`). La clave privada no se entrega a terceros ni a asistentes.

```sh
scripts/audit/snapshot.sh && scripts/audit/verify-snapshot.sh
scripts/audit/sign-snapshot.sh RUTA/clave-privada.pem    # crea SHA256SUMS.sig y signer-public.pem; imprime la huella
scripts/audit/verify-signature.sh HUELLA_ESPERADA        # el auditor, con la huella recibida por otro canal
```

`sign-snapshot.sh` rechaza claves que no sean Ed25519 o estén dentro del repositorio. `verify-signature.sh` exige la huella SHA-256 de la clave pública (SPKI DER) obtenida por un canal **independiente**: el `signer-public.pem` del directorio no es confianza por sí mismo. Si tarball, firma y clave pública viajan juntos, la firma solo demuestra coherencia, no autoría. Se ensayó con una clave desechable (firma válida, huella distinta rechazada, `SHA256SUMS` alterado rechazado, clave dentro del repo rechazada); **no se ha firmado con la clave real del responsable**. Además se recomienda un tag Git firmado sobre el commit del snapshot (`git tag -s`), cuya clave y publicación decide el responsable.

## Comprobación documental

El [comprobador de solo lectura](../../scripts/audit/check-docs.py) comprueba destinos relativos (incluidos fragmentos de encabezado), nombres privados prohibidos, rutas personales y patrones de credenciales de `docs/audit/*.md`. Ejecutar:

```sh
python3 scripts/audit/check-docs.py
```

No sustituye escaneo de secretos de toda la historia ni revisión humana: los vectores públicos y nombres de variables no son claves operativas. El resultado de la ejecución final y su cobertura se comunican junto a la entrega. No se han ejecutado compilación, suites Rust/web, fuzz, Docker, laboratorios ni release como parte de preparar este paquete.
