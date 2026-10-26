# Mapa del código

**Referencia:** `a0f8ac71b5e83607603c45b46113a2aa0ea607b6` (`git rev-parse HEAD`). Complementa la [arquitectura para auditores](arquitectura-para-auditores.md). No afirma seguridad, auditoría realizada ni aptitud para producción. Convención de etiquetas (VERIFICADO por lectura, PARCIAL, SIN COMPROBAR, INFERIDO): [README del paquete](README.md#convención-de-evidencia). No se ejecutaron compilaciones, pruebas ni fuzz para preparar este mapa; las cifras proceden de un script de lectura de ficheros.

## 1. Criterio de las métricas

- **Universo:** ficheros versionados (`git ls-files`) del árbol de trabajo al redactar; se excluyen documentos y scripts nuevos sin versionar. Total: **518 ficheros versionados**, de ellos **114 `.rs`** y **34 143 líneas Rust**. El recuento describe los bytes locales de las fuentes, no acredita identidad de todo el árbol de trabajo con HEAD.
- **LOC = líneas físicas** del fichero (equivale a `wc -l`; incluye blancos y comentarios; no es SLOC). Reproducible con `git ls-files '*.rs' | xargs wc -l` por fichero; la agregación por componente usa el primer o segundo segmento de la ruta (`crates/<x>`, `apps/<x>`).
- **Clasificación de tests:** *ext.* = ficheros en `tests/`, `tests.rs`, `*_tests.rs`, `testutil.rs` y `vectors.rs`. *Inline* = bloques `#[cfg(test)] mod … { … }` dentro de ficheros de producción, delimitados por emparejamiento de llaves (**aproximación**: una llave dentro de una cadena podría desplazar el límite algunas líneas). *Prod.* = total − ext. − inline; incluye los `#[cfg(test)] fn` sueltos (p. ej. constructores de paquetes de prueba en `packet.rs`), que no se descontaron.
- **`#[test]`** cuenta atributos `#[test]` y `#[tokio::test]` por expresión regular; no son casos de ejecución ni cobertura. Las pruebas `proptest` (7 bloques `proptest!` en protocol, crypto, identity ×2, policies, nat-traversal, relay-core) cuentan en la columna de su fichero.
- Lo anterior es **VERIFICADO (lectura/script)**; no mide calidad ni cobertura de pruebas.

## 2. Componentes: tamaño, responsabilidad, riesgo, tests y fuzz

La columna «Riesgo» es un **juicio del autor (INFERIDO)** basado en exposición a entradas no confiables, privilegios y papel en la cadena de confianza; no es una puntuación ni un hallazgo.

| Componente | Ficheros `.rs` | LOC total | LOC prod. | LOC tests (ext.) | LOC tests (inline) | `#[test]` | Responsabilidad | Riesgo (INFERIDO) | Tests / fuzz |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | --- |
| [`crates/protocol`](../../crates/protocol) | 1 | 330 | 201 | 0 | 129 | 7 | Formato de datagramas v2 y parser estricto. | Alto: parser de entrada no autenticada | Inline + `proptest`; fuzz `datagram_parse` |
| [`crates/crypto`](../../crates/crypto) | 6 | 1530 | 853 | 397 | 280 | 25 | Noise IK (`snow`), sesión, antirreplay, rekey, MAC1/MAC2, cookie. | Alto: nonce/replay/cookie propios | `cookie/tests.rs` (+ `vectors.rs`), inline, `proptest`; fuzz `handshake_init`, `session_decrypt`, `replay_window`, `hex_decode`, `agent_engine` |
| [`crates/identity`](../../crates/identity) | 8 | 2139 | 1260 | 789 | 90 | 19 | Configuración firmada v1/v2, verificación, estado, `api_auth`. | Alto: raíz de confianza y anti-rollback | `tests.rs`, `inspect_tests.rs`, `v2_tests.rs`, `tests/fixtures/identity/`; fuzz `signed_config_verify`, `device_config_parse`, `config_v2_parse`, `cert_bundle_state_parse` |
| [`crates/policies`](../../crates/policies) | 1 | 474 | 152 | 0 | 322 | 12 | Clasificación IP y ACL deny-by-default. | Alto: autorización por paquete | Inline + `proptest`; fuzz `ip_classify` |
| [`crates/nat-traversal`](../../crates/nat-traversal) | 1 | 423 | 227 | 0 | 196 | 10 | STUN y candidatos. | Medio: parser UDP | Inline + `proptest`; fuzz `stun_parse` |
| [`crates/relay-core`](../../crates/relay-core) | 1 | 1135 | 627 | 0 | 508 | 15 | Protocolo relay-v2, tokens, cookie, tramas. | Alto: autorización de asignaciones | Inline + `proptest` (vectores publicados); fuzz `relay_parse` |
| [`crates/observability`](../../crates/observability) | 1 | 240 | 196 | 0 | 44 | 1 | Métricas Prometheus acotadas. | Bajo | 1 test; sin fuzz |
| [`crates/tunnel`](../../crates/tunnel) | 4 | 664 | 598 | 0 | 66 | 3 | TUN L3 Linux/macOS/Windows (FFI). | Alto: `unsafe`, paquetes del SO | Inline mínimo; sin fuzz; cada backend solo compila en su SO |
| [`crates/windows`](../../crates/windows) | 3 | 1092 | 670 | 0 | 422 | 6 | DACL, pipes, impersonación, SCM (FFI). | Alto: privilegios locales, `unsafe` | Inline, solo Windows (`#![cfg(windows)]`); sin fuzz |
| [`crates/testpeer`](../../crates/testpeer) | 5 | 976 | 657 | 319 | 0 | 9 | Par de pruebas y herramientas de carga. | Bajo (pruebas) | `peer/tests.rs` |
| [`apps/agent`](../../apps/agent) | 27 | 11797 | 7358 | 763 | 3676 | 136 | Agente: motor, sync, coord/relay, IPC, red del host. | Alto: privilegios, red, estado; `engine.rs` es el mayor fichero | Inline, `netconf_*_tests.rs`, `testutil.rs`; fuzz `agent_engine` |
| [`apps/control-plane`](../../apps/control-plane) | 22 | 6659 | 3800 | 2670 | 189 | 38 | API, OIDC, RBAC, IPAM, emisión firmada, panel. | Alto: autenticación y multi-organización | `tests/` (abuse, integration, oidc, panel, routes_dns) contra PostgreSQL; sin fuzz |
| [`apps/coordinator`](../../apps/coordinator) | 8 | 2264 | 1091 | 1028 | 145 | 18 | Presencia, rendezvous, STUN, tokens de relay. | Medio-alto: emite tokens, cuotas | `tests/api.rs`; sin fuzz del servidor |
| [`apps/relay`](../../apps/relay) | 8 | 2289 | 1080 | 627 | 582 | 23 | Reenvío UDP/TLS de datagramas opacos. | Alto: DoS, audiencia, TLS | `tests/relay.rs`, inline; sin fuzz del servidor (solo parser) |
| [`apps/cli`](../../apps/cli) | 4 | 1331 | 993 | 0 | 338 | 10 | Claves, emisión, enrolamiento, IPC. | Medio-alto: manejo de claves | Inline; sin fuzz |
| **Total Rust del workspace** | 100 | 33343 | 19763 | 6593 | 6987 | 332 | | | |
| [`fuzz`](../../fuzz) (fuera del workspace) | 13 | 683 | 683 | 0 | 0 | 0 | 13 objetivos `cargo-fuzz` | — | Semillas en `fuzz/seeds/`; ejecución SIN COMPROBAR |
| [`deploy/images/healthcheck.rs`](../../deploy/images/healthcheck.rs) | 1 | 117 | 69 | 0 | 48 | 2 | Sonda de salud de imágenes | Bajo | — |

Notas:

- Fuera del workspace Rust también se contaron líneas físicas exactas de fuentes versionadas con el mismo criterio; estos recuentos no son una revisión de su código:

| Componente | Ficheros / extensiones contadas | LOC | Responsabilidad | Riesgo (INFERIDO) | Tests asociados |
| --- | --- | ---: | --- | --- | --- |
| [Panel web](../../apps/web) | 32 `.ts`/`.tsx` | 999 | UI de administración y cliente de sesión/CSRF | Alto: operaciones administrativas en navegador | [screens](../../apps/web/app/screens.test.tsx), [subnets](../../apps/web/app/subnets.test.tsx), [live](../../apps/web/app/live.test.tsx), [Playwright](../../tests/e2e-panel/run.sh) |
| [API cliente](../../packages/api-client) | 2 `.ts` | 54 | Peticiones del panel al control plane | Medio-alto: transporte de autorización/errores | [client.test.ts](../../packages/api-client/src/client.test.ts) |
| [Laboratorios](../../tests) | 37 `.py`, 40 `.sh`, 5 `.ps1` | 5228 / 3303 / 354 respectivamente | Reproducción e integración, algunos con privilegios | Alto al ejecutarlos; no son servicios de producción | [Guía de ejecución](como-reproducir.md#6-laboratorios-docker-y-privilegios) |

- [Infraestructura](../../infra), [despliegue](../../deploy) y [empaquetado](../../packaging) quedan fuera de esos recuentos de lenguajes; sus permisos y cadena de suministro sí forman parte del [alcance propuesto](alcance-y-preguntas.md).
- Los 12 ficheros Rust más largos (líneas físicas):

| Fichero | Líneas |
| --- | ---: |
| [`apps/agent/src/engine.rs`](../../apps/agent/src/engine.rs) | 3618 |
| [`apps/agent/src/relay.rs`](../../apps/agent/src/relay.rs) | 1564 |
| [`crates/relay-core/src/lib.rs`](../../crates/relay-core/src/lib.rs) | 1135 |
| [`apps/coordinator/tests/api.rs`](../../apps/coordinator/tests/api.rs) | 1028 |
| [`apps/agent/src/run.rs`](../../apps/agent/src/run.rs) | 955 |
| [`crates/windows/src/security.rs`](../../crates/windows/src/security.rs) | 928 |
| [`apps/control-plane/tests/integration.rs`](../../apps/control-plane/tests/integration.rs) | 902 |
| [`apps/control-plane/src/openapi.rs`](../../apps/control-plane/src/openapi.rs) | 815 |
| [`apps/relay/tests/relay.rs`](../../apps/relay/tests/relay.rs) | 627 |
| [`crates/crypto/src/lib.rs`](../../crates/crypto/src/lib.rs) | 615 |
| [`apps/relay/src/state.rs`](../../apps/relay/src/state.rs) | 614 |
| [`apps/agent/src/coord.rs`](../../apps/agent/src/coord.rs) | 548 |

## 3. Puntos de entrada

| Binario / librería | Entrada | Observación |
| --- | --- | --- |
| `atenea-agent` | [`apps/agent/src/main.rs`](../../apps/agent/src/main.rs) → [`run.rs` `main`](../../apps/agent/src/run.rs) L52, `run` L278 | Bucle de configuración, red, IPC y motor. |
| `atenea` (CLI) | [`apps/cli/src/main.rs`](../../apps/cli/src/main.rs) | Comandos de claves/emisión/enrolamiento. |
| Control plane | [`apps/control-plane/src/main.rs`](../../apps/control-plane/src/main.rs); rutas en [`api.rs`](../../apps/control-plane/src/api.rs) | Verificación de permisos de la clave de emisor al arrancar. |
| Coordinator | [`apps/coordinator/src/main.rs`](../../apps/coordinator/src/main.rs); rutas en [`lib.rs`](../../apps/coordinator/src/lib.rs) | STUN en [`stun.rs`](../../apps/coordinator/src/stun.rs). |
| Relay | [`apps/relay/src/main.rs`](../../apps/relay/src/main.rs); UDP/TLS en [`lib.rs`](../../apps/relay/src/lib.rs), [`tcp.rs`](../../apps/relay/src/tcp.rs), lógica en [`state.rs`](../../apps/relay/src/state.rs) | Máquina de estados separada de los sockets. |
| Herramientas de prueba | [`crates/testpeer/src`](../../crates/testpeer/src) (`atenea-ipc-probe`, `atenea-load-sign`) | No forman parte del producto. |

## 4. Fuzzing y su alcance

13 objetivos en [`fuzz/fuzz_targets/`](../../fuzz/fuzz_targets) (VERIFICADO, lectura de los `use`): `datagram_parse` (protocol), `handshake_init`, `session_decrypt`, `replay_window`, `hex_decode` (crypto), `signed_config_verify`, `device_config_parse`, `config_v2_parse`, `cert_bundle_state_parse` (identity), `ip_classify` (policies), `relay_parse` (relay-core), `stun_parse` (nat-traversal) y `agent_engine` (motor del agente con pares simulados). El crate `fuzz` está excluido del workspace ([`Cargo.toml` L20](../../Cargo.toml)) y tiene su propio [`Cargo.lock`](../../fuzz/Cargo.lock).

**Sin objetivo de fuzz** (VERIFICADO por ausencia en la lista): handlers HTTP de control plane y coordinator, estado del relay (`state.rs`) y sus sockets, IPC del agente, `crates/windows`, `crates/tunnel`, JSON de `coord.rs`. En [`ci.yml`](../../.github/workflows/ci.yml) no hay coincidencias de «fuzz» (VERIFICADO, búsqueda de texto); si se ejecutan en otro workflow o a mano: SIN COMPROBAR. Ejecución y hallazgos de fuzz: SIN COMPROBAR. Cómo reproducir: [como-reproducir.md](como-reproducir.md).

## 5. Inventario de bloques `unsafe`

### 5.1 Política y alcance

- El workspace declara `unsafe_code = "forbid"` ([`Cargo.toml` L28–L29](../../Cargo.toml)). Los miembros que heredan `[lints] workspace = true` no pueden contener `unsafe` (VERIFICADO, lectura de manifiestos: protocol, crypto, identity, policies, nat-traversal, relay-core, observability, testpeer, agent, control-plane, coordinator, relay, cli).
- Solo **`crates/tunnel`** ([`Cargo.toml` L11–L13](../../crates/tunnel/Cargo.toml)) y **`crates/windows`** ([`Cargo.toml` L9–L10](../../crates/windows/Cargo.toml)) fijan `unsafe_code = "deny"` y lo permiten con `#[allow(unsafe_code)]`.
- Búsqueda de la palabra `unsafe` en todos los `.rs` versionados fuera de esos dos crates: sin usos de código; solo aparecen una mención en un comentario ([`platform/local_ipc.rs` L22](../../apps/agent/src/platform/local_ipc.rs)) y las cadenas CSP `'unsafe-inline'` ([`panel.rs` L424](../../apps/control-plane/src/panel.rs)). El crate `fuzz` no se alinea con el workspace y tampoco contiene `unsafe` (VERIFICADO).
- **Recuento (tokens `unsafe` fuera de comentarios):** `tunnel/linux.rs` 4 · `tunnel/macos.rs` 3 bloques · `tunnel/windows.rs` 15 (6 bloques + 2 `unsafe impl` + 7 alias `unsafe extern "system" fn`) · `windows/security.rs` 62 (41 en código de producción y 21 en `#[cfg(test)]`) · `windows/service.rs` **0**. Total **84 sitios**, más 8 atributos `allow(unsafe_code)` (4 en `linux.rs`, 1 `#![allow]` en `macos.rs`, 1 en `tunnel/windows.rs`, 2 en `windows/lib.rs`). Método: script de lectura sobre las líneas; los bloques multilínea se cuentan en su línea de apertura. Las dependencias (`snow`, `ring`, `tokio`…) contienen su propio `unsafe`: **no inventariado** (no se ejecutó `cargo-geiger` ni similar).
- Comentarios `// SAFETY:` presentes: `tunnel/linux.rs`, `macos.rs` y `windows.rs` en todos los bloques; `windows/security.rs` **ninguno por bloque**, solo el comentario de módulo en [`lib.rs` L3](../../crates/windows/src/lib.rs) (L4 `#[allow(unsafe_code)] mod security`).
- `#[allow(unsafe_code)] mod service;` ([`lib.rs` L8](../../crates/windows/src/lib.rs)) cubre `service.rs`, que **no contiene `unsafe`** (usa `windows-service`): el permiso es más amplio que lo necesario.
- `crates/windows` solo compila en Windows (`#![cfg(windows)]`, [`lib.rs` L2](../../crates/windows/src/lib.rs)); cada backend de `tunnel` solo en su SO ([`tunnel/lib.rs`](../../crates/tunnel/src/lib.rs)). La verificación en ejecución de estos bloques depende de los runners de macOS/Windows de CI ([`ci.yml` L63–L99](../../.github/workflows/ci.yml)), cuyos resultados **no** se consultaron aquí (SIN COMPROBAR).
- La columna «Garantía» resume lo que el código/comentario sostiene (VERIFICADO, lectura) y **no es una demostración de solidez**: indica cuándo el bloque confía en el SO o en la DLL.

### 5.2 `crates/tunnel` — Linux ([`linux.rs`](../../crates/tunnel/src/linux.rs))

| Línea | Función/ítem | Operación | Garantía que sostiene el bloque (según el código) |
| --- | --- | --- | --- |
| [L57](../../crates/tunnel/src/linux.rs#L57) | create | ioctl(TUNSETIFF) sobre /dev/net/tun | `req` es un `IfReq` `repr(C)` inicializado y vive durante la llamada; el fd es propio. Nombre validado antes (`valid_name`, ≤15 B). Comentario SAFETY presente. |
| [L66](../../crates/tunnel/src/linux.rs#L66) | create | `AsyncFd::register(OwnedFd)` | El `OwnedFd` posee un descriptor válido no bloqueante; `AsyncFd` lo conserva sin cerrarlo por otra vía. SAFETY presente. |
| [L82](../../crates/tunnel/src/linux.rs#L82) | read_packet | `libc::read` en el fd | Búfer `buf` válido de `buf.len()` bytes; fd abierto mientras `self` vive. SAFETY presente. |
| [L102](../../crates/tunnel/src/linux.rs#L102) | write_packet | `libc::write` en el fd | `pkt` válido de `pkt.len()` bytes; fd abierto. SAFETY presente. |

### 5.3 `crates/tunnel` — macOS ([`macos.rs`](../../crates/tunnel/src/macos.rs), `#![allow(unsafe_code)]` en L2)

| Línea | Función/ítem | Operación | Garantía que sostiene el bloque (según el código) |
| --- | --- | --- | --- |
| [L69](../../crates/tunnel/src/macos.rs#L69) | create (L69–L118) | `socket`/`ioctl(CTLIOCGINFO)`/`connect`/`getsockopt`/`fcntl` + `AsyncFd::register` | Un único bloque grande: estructuras ABI (`ControlInfo`, `ControlAddr`) inicializadas; `OwnedFd::from_raw_fd` envuelve el descriptor justo tras comprobar `raw >= 0`, así que todos los retornos anticipados cierran el fd. Nombre de interfaz leído en búfer de 16 B con NUL comprobado. SAFETY presente. |
| [L128](../../crates/tunnel/src/macos.rs#L128) | read_packet (L128–L147) | `readv` con dos `iovec` (cabecera 4 B + búfer) | `header` y `buf` son exclusivos y válidos durante la llamada; `n < 4` y familia se validan (`normalize`) antes de exponer el paquete. SAFETY presente. |
| [L158](../../crates/tunnel/src/macos.rs#L158) | write_packet (L158–L180) | `writev` con dos `iovec` | `as_ptr().cast_mut()` solo por la firma de `iovec`; `writev` únicamente lee; se comprueba escritura completa. SAFETY presente. |

### 5.4 `crates/tunnel` — Windows/Wintun ([`windows.rs`](../../crates/tunnel/src/windows.rs), `#![allow(unsafe_code)]` en L2)

| Línea | Función/ítem | Operación | Garantía que sostiene el bloque (según el código) |
| --- | --- | --- | --- |
| [L16](../../crates/tunnel/src/windows.rs#L16) | tipos | `unsafe extern "system" fn` (Create) | Declaración de tipo de puntero a función del ABI de Wintun 0.14.1; la corrección de la firma no la comprueba el compilador. |
| [L17](../../crates/tunnel/src/windows.rs#L17) | tipos | `unsafe extern fn` Close/End | idem |
| [L18](../../crates/tunnel/src/windows.rs#L18) | tipos | `unsafe extern fn` Start | idem |
| [L19](../../crates/tunnel/src/windows.rs#L19) | tipos | `unsafe extern fn` Event | idem |
| [L20](../../crates/tunnel/src/windows.rs#L20) | tipos | `unsafe extern fn` Receive | idem |
| [L21](../../crates/tunnel/src/windows.rs#L21) | tipos | `unsafe extern fn` Release/Send | idem |
| [L22](../../crates/tunnel/src/windows.rs#L22) | tipos | `unsafe extern fn` Allocate | idem |
| [L40](../../crates/tunnel/src/windows.rs#L40) | `Session` | `unsafe impl Send` | Wintun documenta las operaciones como thread-safe y el cierre ocurre tras `join` (SAFETY presente). Depende de documentación externa no verificada aquí; contiene `HANDLE`s crudos. |
| [L41](../../crates/tunnel/src/windows.rs#L41) | `Session` | `unsafe impl Sync` | idem (cubierto por el mismo comentario). |
| [L45](../../crates/tunnel/src/windows.rs#L45) | `Session::drop` | `WintunEndSession`, `WintunCloseAdapter`, `CloseHandle(stop)` | Handles propios, cerrados una vez, en orden inverso a la creación; `Session` solo se destruye al soltarse el último `Arc` (tras `join` del hilo). SAFETY presente. |
| [L115](../../crates/tunnel/src/windows.rs#L115) | `Tun::create` (L115–L176) | `Library::new`, resolución de 9 símbolos, `CreateEventW`, `WintunCreateAdapter`, `WintunStartSession` | DLL de ruta absoluta (`ATENEA_WINTUN_DLL` o junto al ejecutable); símbolos obtenidos con `libloading` (error si faltan) y tipados manualmente; `Library` se guarda en `Session` para sobrevivir a los punteros. Los retornos anticipados cierran `stop`/adaptador. No hay verificación de hash/firma de la DLL en este código. SAFETY presente. |
| [L185](../../crates/tunnel/src/windows.rs#L185) | hilo lector (L185–L188) | `WaitForMultipleObjects` sobre `stop` y el evento de lectura | El `Arc<Session>` clonado mantiene sesión y eventos vivos hasta que termina el hilo. SAFETY presente. |
| [L222](../../crates/tunnel/src/windows.rs#L222) | `read_packet` (L222–L245) | `WintunReceivePacket`, `from_raw_parts`, copia, `WintunReleaseReceivePacket` | El paquete pertenece a la sesión y se libera antes de retornar; se comprueba `buffer.len()` y `validate` (cabecera IP) antes de copiar; longitud la entrega la DLL (confianza en Wintun). SAFETY presente. |
| [L260](../../crates/tunnel/src/windows.rs#L260) | `write_packet` (L260–L267) | `WintunAllocateSendPacket`, `copy_nonoverlapping`, `WintunSendPacket` | Reserva del tamaño exacto; puntero nulo comprobado; `SendPacket` transfiere la propiedad a Wintun; `validate` acota longitud (≤65535) antes del cast a `u32`. SAFETY presente. |
| [L274](../../crates/tunnel/src/windows.rs#L274) | `Tun::drop` | `SetEvent(stop)` | `stop` vive hasta después del `join` y del último `Arc`. SAFETY presente. |

### 5.5 `crates/windows` — código de producción ([`security.rs`](../../crates/windows/src/security.rs) L1–L529)

Sin comentario SAFETY por bloque. Patrón común: `Local` y `Handle` son contenedores RAII de propiedad única que liberan con `LocalFree`/`CloseHandle`.

| Línea | Función/ítem | Operación | Garantía que sostiene el bloque (según el código) |
| --- | --- | --- | --- |
| [L52](../../crates/windows/src/security.rs#L52) | `Local::drop` | `LocalFree` | Puntero devuelto por una API Win32 que asigna con `LocalAlloc`; tipo no `Clone`, se libera una vez. |
| [L60](../../crates/windows/src/security.rs#L60) | `Handle::drop` | `CloseHandle` | Handle de propiedad única (token); no `Clone`. |
| [L68](../../crates/windows/src/security.rs#L68) | `descriptor` | `ConvertStringSecurityDescriptorToSecurityDescriptorW` | SDDL UTF-16 terminada en NUL (`wide` rechaza NUL interior); salida envuelta en `Local`. |
| [L78](../../crates/windows/src/security.rs#L78) | `process_token` | `OpenProcessToken(…, TOKEN_QUERY)` | Recibe pseudo-handle de proceso; el token de salida se envuelve en `Handle` solo si la llamada tiene éxito. |
| [L85](../../crates/windows/src/security.rs#L85) | `user_sid` | `GetTokenInformation` (consulta de tamaño) | Búfer nulo/longitud 0: fallo esperado; se exige `len != 0` después. |
| [L93](../../crates/windows/src/security.rs#L93) | `user_sid` | `GetTokenInformation(TokenUser)` | Búfer `Vec<usize>` de tamaño ≥ `len` (alineación de `usize`). |
| [L98](../../crates/windows/src/security.rs#L98) | `user_sid` | Reinterpretación `&*… as TOKEN_USER` | Búfer relleno por el SO y alineado; la referencia no sobrevive a `data` (`sid_string` se llama dentro). |
| [L103](../../crates/windows/src/security.rs#L103) | `sid_string` | `ConvertSidToStringSidW` | `sid` apunta a memoria viva del token/ACE; cadena de salida asignada con `LocalAlloc` → `Local`. |
| [L108](../../crates/windows/src/security.rs#L108) | `sid_string` | Escaneo hasta NUL (`*text.add(len)`) | Cadena UTF-16 terminada en NUL producida por el SO; lectura dentro de la asignación. |
| [L113](../../crates/windows/src/security.rs#L113) | `sid_string` | `slice::from_raw_parts(text, len)` | `len` calculada por el escaneo; la asignación sigue viva (`allocation` se suelta después). |
| [L122](../../crates/windows/src/security.rs#L122) | `privileged` | `GetTokenInformation` (tamaño, TokenGroups) | Como L85. |
| [L129](../../crates/windows/src/security.rs#L129) | `privileged` | `GetTokenInformation(TokenGroups)` | Como L93. |
| [L134](../../crates/windows/src/security.rs#L134) | `privileged` | Reinterpretación `&*… as TOKEN_GROUPS` | Búfer alineado y relleno por el SO, vivo durante el uso. |
| [L136](../../crates/windows/src/security.rs#L136) | `privileged` | `slice::from_raw_parts(Groups, GroupCount)` | Longitud tomada del `GroupCount` que entrega el SO sobre una matriz de tamaño variable; no hay comprobación adicional contra `len` (confianza en el SO). |
| [L150](../../crates/windows/src/security.rs#L150) | `create_private` | `GetCurrentProcess()` | Pseudo-handle siempre válido; `unsafe` solo por ser FFI. |
| [L163](../../crates/windows/src/security.rs#L163) | `create_private` | `CreateFileW(CREATE_NEW, DACL protegida)` | Ruta UTF-16 con NUL; `SECURITY_ATTRIBUTES` y su descriptor viven durante la llamada; creación exclusiva sin compartir y sin seguir reparse points; retorno `INVALID_HANDLE_VALUE` comprobado. |
| [L177](../../crates/windows/src/security.rs#L177) | `create_private` | `File::from_raw_handle` | Handle válido y único (comprobado); la propiedad pasa a `File`. |
| [L181](../../crates/windows/src/security.rs#L181) | `append_private` | `GetCurrentProcess()` | Como L150. |
| [L194](../../crates/windows/src/security.rs#L194) | `append_private` | `CreateFileW(OPEN_ALWAYS, append)` | Como L163 pero abre existente; el objeto se valida después (`GetFileInformationByHandle`, `validate_dacl`). |
| [L208](../../crates/windows/src/security.rs#L208) | `append_private` | `File::from_raw_handle` | Como L177. |
| [L209](../../crates/windows/src/security.rs#L209) | `append_private` | `mem::zeroed::<BY_HANDLE_FILE_INFORMATION>()` | Estructura C de enteros: el patrón de ceros es válido. |
| [L210](../../crates/windows/src/security.rs#L210) | `append_private` | `GetFileInformationByHandle` | `handle` sigue vivo (poseído por `file`); rechaza reparse points y directorios. |
| [L225](../../crates/windows/src/security.rs#L225) | `read_private` | `CreateFileW(OPEN_EXISTING, READ_CONTROL)` | Sin atributos de seguridad (solo lectura); `OPEN_REPARSE_POINT`; handle comprobado. |
| [L239](../../crates/windows/src/security.rs#L239) | `read_private` | `File::from_raw_handle` | Como L177. |
| [L240](../../crates/windows/src/security.rs#L240) | `read_private` | `mem::zeroed` | Como L209. |
| [L241](../../crates/windows/src/security.rs#L241) | `read_private` | `GetFileInformationByHandle` | Como L210; valida el mismo objeto que se lee. |
| [L259](../../crates/windows/src/security.rs#L259) | `validate_dacl` | `GetSecurityInfo(DACL|OWNER)` | Handle del `File` prestado; punteros de salida (`owner`, `acl`) apuntan dentro del descriptor `sd`, que `Local` libera al final de la función. |
| [L283](../../crates/windows/src/security.rs#L283) | `validate_dacl` | `GetSecurityDescriptorControl` | `sd` válido de la llamada anterior. |
| [L299](../../crates/windows/src/security.rs#L299) | `validate_dacl` | Desreferencia `(*acl).AceCount` | `acl` no nulo (comprobado L289–L291) y vivo dentro de `sd`. |
| [L301](../../crates/windows/src/security.rs#L301) | `validate_dacl` | `GetAce` | Índice < `AceCount`; puntero de salida dentro de la ACL. |
| [L304](../../crates/windows/src/security.rs#L304) | `validate_dacl` | Reinterpretación `&*ace as ACE_HEADER` | Puntero de `GetAce`; solo lee `AceType`. |
| [L311](../../crates/windows/src/security.rs#L311) | `validate_dacl` | Reinterpretación `&*ace as ACCESS_ALLOWED_ACE` | Solo tras `AceType == 0`; el SID es variable y no se contrasta su longitud con `AceSize` (confianza en el SO). |
| [L337](../../crates/windows/src/security.rs#L337) | `replace_private` | `MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)` | Rutas UTF-16 con NUL vivas durante la llamada; el temporal se crea con DACL protegida. |
| [L395](../../crates/windows/src/security.rs#L395) | `create_pipe` | `create_with_security_attributes_raw` | `attrs` y el descriptor `sd` viven durante la llamada; el SO copia el descriptor; `reject_remote_clients(true)`; `status_gid` solo 544/545. |
| [L411](../../crates/windows/src/security.rs#L411) | `Revert::drop` | `RevertToSelf` | Si falla, `abort()` (no se continúa suplantado). |
| [L416](../../crates/windows/src/security.rs#L416) | `authorize_pipe_client` | `ImpersonateNamedPipeClient` | Handle de pipe prestado y conectado; la guarda `Revert` se crea inmediatamente después. |
| [L421](../../crates/windows/src/security.rs#L421) | `authorize_pipe_client` | `OpenThreadToken(…, OpenAsSelf=1)` | Token de salida envuelto en `Handle`; se usa mientras dura la suplantación. |
| [L430](../../crates/windows/src/security.rs#L430) | `connect_pipe` | `CreateFileW` con `SECURITY_IDENTIFICATION` | El servidor puede identificar pero no suplantar al cliente; handle comprobado. |
| [L444](../../crates/windows/src/security.rs#L444) | `connect_pipe` | `File::from_raw_handle` | Como L177. |
| [L449](../../crates/windows/src/security.rs#L449) | `connect_pipe` | `GetSecurityInfo(OWNER)` | Handle vivo (propiedad de `file`); `sd` envuelto en `Local`; se exige propietario Administrators. |
| [L489](../../crates/windows/src/security.rs#L489) | `pipe_reply` | `PeekNamedPipe` | Handle poseído por `file`; punteros no usados nulos; lectura acotada a 4096 B por iteración y 65536 B totales. |

### 5.6 `crates/windows` — pruebas (`#[cfg(test)]`, [`security.rs`](../../crates/windows/src/security.rs) L530–L928)

Solo se compilan en pruebas en Windows; ejercitan suplantación, `AccessCheck` y DACL reales.

| Línea | Función/ítem | Operación | Garantía que sostiene el bloque (según el código) |
| --- | --- | --- | --- |
| [L537](../../crates/windows/src/security.rs#L537) | `Restore::drop` (test) | `SetThreadToken`/`RevertToSelf` | Restaura el token previo; aborta si falla. |
| [L549](../../crates/windows/src/security.rs#L549) | `as_restricted_user` (test) | `OpenThreadToken` | Guarda el token previo de hilo. |
| [L563](../../crates/windows/src/security.rs#L563) | `as_restricted_user` (test) | `ImpersonateLoggedOnUser` | Token restringido válido. |
| [L574](../../crates/windows/src/security.rs#L574) | `actual_pipe_descriptor` (test) | `GetSecurityInfo` | Pipe vivo; `sd` a `Local`. |
| [L590](../../crates/windows/src/security.rs#L590) | `actual_pipe_descriptor` (test) | `ConvertSecurityDescriptorToStringSecurityDescriptorW` | `sd` válido; salida a `Local`. |
| [L605](../../crates/windows/src/security.rs#L605) | `actual_pipe_descriptor` (test) | Escaneo hasta NUL | Como L108. |
| [L612](../../crates/windows/src/security.rs#L612) | `actual_pipe_descriptor` (test) | `from_raw_parts` | Como L113. |
| [L620](../../crates/windows/src/security.rs#L620) | `assert_effective_access_denied` (test) | `OpenThreadToken` | Como L421. |
| [L632](../../crates/windows/src/security.rs#L632) | `assert_effective_access_denied` (test) | `MapGenericMask` | Estructuras locales válidas. |
| [L640](../../crates/windows/src/security.rs#L640) | `assert_effective_access_denied` (test) | `AccessCheck` | Búfer de privilegios local de tamaño informado. |
| [L669](../../crates/windows/src/security.rs#L669) | prueba de usuario restringido | `OpenProcessToken` | Pseudo-handle de proceso. |
| [L690](../../crates/windows/src/security.rs#L690) | prueba de usuario restringido | `CreateWellKnownSid` | Búfer `[usize;16]` con tamaño informado. |
| [L708](../../crates/windows/src/security.rs#L708) | prueba de usuario restringido | `CreateRestrictedToken` | Punteros a datos locales vivos. |
| [L729](../../crates/windows/src/security.rs#L729) | prueba de usuario restringido | `ConvertStringSidToSidW` | Cadena UTF-16 con NUL; salida a `Local`. |
| [L742](../../crates/windows/src/security.rs#L742) | prueba de usuario restringido | `SetTokenInformation(TokenIntegrityLevel)` | Etiqueta local viva; tamaño calculado con `GetLengthSid`. |
| [L769](../../crates/windows/src/security.rs#L769) | prueba de usuario restringido | `create_with_security_attributes_raw(null)` (control) | Atributos nulos: comprueba que Administrators sí puede crear otra instancia. |
| [L808](../../crates/windows/src/security.rs#L808) | prueba de usuario restringido | `create_with_security_attributes_raw(null)` | Espera `ERROR_ACCESS_DENIED` bajo suplantación. |
| [L816](../../crates/windows/src/security.rs#L816) | prueba de usuario restringido | `CreateFileW` con `WRITE_DAC`/`WRITE_OWNER` | Espera denegación; cierra el handle si se concediera. |
| [L870](../../crates/windows/src/security.rs#L870) | `private_files_are_exclusive_and_replaceable` | `GetCurrentProcess()` | FFI trivial. |
| [L905](../../crates/windows/src/security.rs#L905) | `rejects_public_secret_dacl` | `CreateFileW` con DACL pública | Crea un fichero con ACL no privada para comprobar el rechazo. |
| [L917](../../crates/windows/src/security.rs#L917) | `rejects_public_secret_dacl` | `File::from_raw_handle` | Handle comprobado; se cierra al soltar. |

### 5.7 Observaciones para el auditor (INFERIDO a partir de la lectura)

1. La confianza en datos que entrega el SO o la DLL sin comprobación adicional: `GroupCount` ([L136](../../crates/windows/src/security.rs#L136)), tamaño de ACE/SID ([L311](../../crates/windows/src/security.rs#L311)), longitud y puntero de paquete de Wintun ([`tunnel/windows.rs` L222](../../crates/tunnel/src/windows.rs#L222)).
2. `unsafe impl Send/Sync for Session` depende de la documentación de Wintun 0.14.1 ([L40–L41](../../crates/tunnel/src/windows.rs#L40)); no se verificó contra la DLL.
3. La carga de `wintun.dll` ([L115](../../crates/tunnel/src/windows.rs#L115)) usa ruta absoluta, pero el código no comprueba hash ni firma; depende del instalador y de los permisos del directorio ([ADR-008 §4](../architecture/ADR-008-clientes-windows-macos.md)).
4. `append_private` abre con `OPEN_ALWAYS` y valida el objeto después ([L194](../../crates/windows/src/security.rs#L194)); `create_private` usa `CREATE_NEW` ([L163](../../crates/windows/src/security.rs#L163)). La diferencia es deliberada para el registro de diagnóstico; revisar la ventana entre apertura y validación.
5. Falta de comentarios SAFETY por bloque en `security.rs` (62 sitios).
6. El bloque único de `create` en macOS (L69–L118) agrupa nueve llamadas; la liberación del descriptor se apoya en `OwnedFd` ([L74](../../crates/tunnel/src/macos.rs#L74)).

Las preguntas de revisión correspondientes están en [alcance y preguntas](alcance-y-preguntas.md) (pregunta 14) y en el modelo de amenazas T25/T28 ([threat-model](../security/threat-model.md)).

## 6. Límites

Lectura estática del HEAD indicado. Los números de línea se desplazan con cualquier cambio posterior. No se midió cobertura de pruebas ni se ejecutó ninguna suite. El código del panel web, los scripts de laboratorio y la infraestructura solo se contaron, no se revisaron. El estado de [evidencia](evidencia.md) y la [matriz de aceptación](../operations/matriz-de-aceptacion.md) siguen siendo la referencia de qué se ha ejecutado y con qué resultado.
