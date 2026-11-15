<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-017: Redes por instancia en Windows

- **Estado:** **Aceptado** por el responsable el 2026-10-29 con las seis recomendaciones de § «Decisiones del responsable». Sin código todavía; la fase 3 queda condicionada a que el responsable aporte una máquina Windows real (decisión 6).
- **Relacionados:** ADR-008 (cliente Windows), ADR-011 (aplicación de escritorio), ADR-013 (redes múltiples; aplazó Windows a una fase propia), ADR-014.

## Contexto

En Linux y macOS un dispositivo puede estar en varias redes: `atenea network join|up|down|leave|list|inspect` crea una instancia por red (claves, interfaz, puerto UDP, sockets y servicio del sistema propios). En Windows **todo `atenea network` devuelve «no soportado todavía»** (`apps/cli/src/network.rs:72`) y la app de escritorio no puede unirse a redes (`apps/desktop/src/networks.rs:173-175`). Lo que hay hoy es una única instancia «heredada»: el servicio `AteneaAgent` (LocalSystem, arranque manual, nombre fijo en `crates/windows/src/service.rs:18` y en `packaging/windows/install.ps1`), un adaptador Wintun, dos tuberías con nombre y un `agent.toml` en `%ProgramData%\Atenea`.

Lo que **sí** es portable y no hay que rehacer: el motor, la política de red (`netconf`), la guarda de solape de rutas (`apps/agent/src/run.rs:232-275`), el cliente Wintun, las rutas y direcciones por NetTCPIP, el DNS por NRPT, el ciclo de vida SCM y las tuberías con DACL (`crates/windows/src/security.rs`). Lo que falta es el **gestor** multirred.

Hechos que condicionan el diseño (leídos en el código; lo deducido está marcado):

- El nombre del servicio está fijo en el despachador SCM y en el instalador: **un solo servicio**.
- Cada ejecución del agente abre **una** TUN; el código no impone un límite global entre procesos *(INFERIDO: que el controlador Wintun admita varios adaptadores simultáneos no está comprobado)*, y `WintunCreateAdapter` se llama con GUID nulo (`crates/tunnel/src/windows.rs`), es decir, un GUID **aleatorio** por creación. *(INFERIDO: puede acumular perfiles de red en Windows; hay que medirlo.)*
- El nombre de interfaz admite 1–15 caracteres ASCII alfanuméricos o `_` (`apps/agent/src/config.rs:191-199`).
- Las tuberías son `\\.\pipe\atenea-control` y `\\.\pipe\atenea-status`; los nombres de tubería son **planos** (no admiten subdirectorios como los sockets `<run>/<id8>/…`).
- `%ProgramData%\Atenea` es **íntegramente privado** (SYSTEM y Administradores): hoy no hay datos públicos legibles sin privilegio, y `network list`/la app los necesitan sin elevar.
- No hay automatización de cortafuegos de Windows (`docs/operations/instalacion-windows.md`).
- El preflight de DNS (`dns_domain_conflict`) lo implementa macOS pero **no Windows**: una colisión de NRPT se detecta tarde, tras configurar enlace y rutas.
- El instalador rechaza reinstalar si `AteneaAgent` existe y `uninstall.ps1 -Purge` borra todo `%ProgramData%\Atenea` (también identidades de redes).
- La app eleva con `osascript` (macOS) o `pkexec` (Linux) y **lee la salida estándar** de la operación elevada (`apps/desktop/src/networks.rs:182-186`). En Windows, `Start-Process -Verb RunAs` **no permite redirigir** la salida *(limitación conocida de PowerShell, no probada aquí)*, y el crate de la app tiene `unsafe_code = forbid` (no puede llamar a `ShellExecuteEx` por FFI).
- La CI de Windows ejecuta un smoke real **de una instancia** (servicio, Wintun, ACL, NRPT, ping a un par de pruebas), pero **no** dos redes simultáneas ni UAC (`tests/windows/smoke.py`).

## Decisión propuesta

### 1. Un servicio de Windows por red, con el mismo binario

- Nombre `AteneaNetwork-<id8>` (el `id8` son los **últimos** 8 hex del UUID de la red, como en Unix) y nombre visible fijo `Atenea One (<id8>)`. **El nombre de la red no entra jamás** en el nombre ni en el nombre visible del servicio: viene del servidor y es entrada no confiable.
- Ejecuta `atenea-agent.exe run --service --config %ProgramData%\Atenea\networks\<UUID>\device\agent.toml`, como **LocalSystem** (igual que ADR-008), con las mismas acciones de recuperación (5/15/60 s).
- El despachador SCM recibe el nombre por argumento en vez de la constante `NAME`. El servicio heredado `AteneaAgent` sigue funcionando mientras exista.
- El CLI crea, arranca, para y borra servicios con la **API del SCM** (`windows-service`, ya es dependencia), no ejecutando `sc.exe`. *(Sin comprobar que la versión 0.8 expone todo lo necesario, incluidas las acciones de recuperación: lo decide el primer spike de la fase 1.)*
- `up` deja el servicio en arranque **automático retrasado** y lo inicia; `down` lo para y lo deshabilita.

### 2. Distribución de datos y metadatos públicos

- Privado (SYSTEM y Administradores, DACL protegida): `%ProgramData%\Atenea\networks\<UUID>\device\` con claves, `agent.toml`, `config.bin`, estado y registro por red. Se reutilizan los helpers de custodia existentes.
- Público (lectura para `Users`): un índice aparte, `%ProgramData%\Atenea\public\<UUID>.json`, con los mismos campos que el `network.json` de Unix (nombre, servidor, huellas, interfaz, puerto, estado de unión). Así **no se toca la DACL del árbol privado** y `list`/la app funcionan sin elevar. Se descartó hacer que `list` exija administrador (rompería la pantalla Redes sin elevar).
- Se escribe de forma atómica, con propietario Administradores, sin seguir enlaces ni puntos de reanálisis.

### 3. Adaptador Wintun por red

- Nombre `atn_<id8>` (12 caracteres, válido para la regla de 1–15). Convive con `atenea0` heredado.
- **GUID del adaptador = UUID de la red** (se pasa a `WintunCreateAdapter`) en vez de aleatorio: identidad estable entre reinicios y sin acumular perfiles de red. No es una primitiva criptográfica: es solo un identificador que ya es público.
- Es el único cambio con `unsafe` nuevo, confinado a `crates/tunnel` (ya permitido).

### 4. Puerto UDP y cortafuegos

- Asignación desde 51820 evitando redes registradas, la heredada (incluso parada) y puertos ocupados (bind de prueba), igual que Unix; el puerto queda en los metadatos.
- **Sin reglas de cortafuegos automáticas**, como en ADR-008: el flujo directo funciona por el seguimiento de UDP saliente. Se documenta; automatizarlo queda fuera.

### 5. Canal de control: una tubería por red

- `\\.\pipe\atenea-<id8>-control` y `\\.\pipe\atenea-<id8>-status`, con la misma DACL, etiqueta de integridad e impersonación de ADR-008. `status_gid = 545` para que un usuario estándar lea el estado.
- La disponibilidad se comprueba **abriendo** la tubería, no con `Path::exists()` como en Unix.

### 6. Guarda de DNS antes de configurar

- Windows implementa `dns_domain_conflict`: detecta una regla NRPT del mismo dominio, ajena o de otra instancia de Atenea, **antes** de configurar el enlace y las rutas, con el mismo error tipado que macOS. Es **condición previa** para habilitar la segunda red.

### 7. Confianza en el binario del servicio

Un servicio LocalSystem cuyo `ImagePath` apunta a un binario escribible por un usuario estándar es una escalada de privilegios. Por tanto `network up` solo registra el servicio si:

- el binario es `%ProgramFiles%\Atenea\atenea-agent.exe` (ruta fija, entrecomillada),
- ni el fichero ni sus directorios padre permiten escritura a no administradores (comprobado con la DACL), y
- el propio CLI corre elevado.

Es el equivalente Windows de la comprobación `trusted` de Unix.

### 8. Elevación desde la app (UAC)

- La app lanza el CLI con `powershell.exe -NoProfile -Command Start-Process … -Verb RunAs -Wait` (sin FFI, sin `unsafe`), con argumentos validados en Rust como en macOS y Linux.
- **Canal de resultado:** la salida no se puede capturar, y **el proceso elevado nunca escribe en una ruta que controle el usuario** (sería una escritura privilegiada a una ruta atacante). El CLI elevado escribe `result.json` en `%ProgramData%\Atenea\ui-results\<nonce>\`, creado por el lado elevado con DACL de escritura solo para SYSTEM y Administradores y de lectura para `Users`; el `nonce` (32 hex) lo elige la app y se valida. Tras la operación la app relee `network list` (sin privilegios) como fuente de verdad del estado; `result.json` solo aporta el mensaje de error. El CLI limpia los resultados antiguos.
- Paridad aceptada con ADR-011: el código de unión (que puede llevar token de un solo uso) viaja como argumento de la orden elevada, igual que con `osascript` y `pkexec`.

### 9. Instalador y desinstalador

- El instalador deja de registrar `AteneaAgent` por defecto (ADR-013: sin instancia «Predeterminada»); instala binarios y Wintun. La instalación heredada existente se respeta.
- `uninstall.ps1` para y deshabilita los `AteneaNetwork-*`. **`-Purge` rechaza** borrar identidades si existen redes unidas, salvo confirmación explícita tras listarlas, y recomienda `atenea network leave` antes (que revoca en el servidor).

### 10. Migración de la instancia heredada: no se implementa

`atenea network legacy` en Windows informa de si existe `AteneaAgent` e indica reinstalar y unirse por código. No hay instalaciones Windows reales conocidas (solo laboratorio y CI); `migrate-legacy` (mover claves y convertir el servicio) tiene coste y riesgo propios sin usuarios que lo necesiten. Se reabre si aparece una instalación real.

## Fases y criterios de salida

| Fase | Alcance | Criterio de salida |
| --- | --- | --- |
| **1. Plataforma** | Despachador por nombre; spike de la API del SCM; GUID de adaptador; tuberías por red; `dns_domain_conflict` en Windows. | Pruebas unitarias; el smoke actual de una instancia sigue verde; ensayo en el runner de dos adaptadores simultáneos. |
| **2. CLI y servicios** | `network join/list/up/down/leave/inspect` en Windows, metadatos públicos, asignador de puerto, servicio por red, comprobación de confianza del binario, instalador/desinstalador. | Aceptación nueva en el runner: **dos redes a la vez** (dos servicios, dos adaptadores, dos tuberías, ACL, guarda de DNS, `down`/`up`/`leave` sin tocar la otra, desinstalación con y sin `-Purge`). |
| **3. App de escritorio** | UAC, rutas y tuberías de Windows, canal de resultado. | Pruebas unitarias con ejecutor simulado; **prueba manual en una máquina Windows real** (UAC no se puede ejercitar en el runner). |
| **4. (opcional) Migración** | Solo si hay una instalación real. | Decisión del responsable. |

Para el runner de CI no hay servidor de control en Windows, así que la fase 2 provisiona los directorios de red con configuraciones firmadas ya emitidas (como el smoke actual) en vez de ejecutar `join` contra un control plane; la unión real contra un servidor se prueba a mano en la máquina Windows de la fase 3.

## Alternativas descartadas

- **Un solo servicio que multiplexe varias redes:** reintroduce el gestor residente que ADR-013 descartó y rompe el aislamiento por proceso.
- **`sc.exe` desde el CLI:** más superficie de inyección en argumentos y salida frágil de parsear.
- **Que `list` exija administrador:** rompe la app sin elevar.
- **Capturar la salida con un fichero elegido por el usuario:** escritura privilegiada en una ruta controlada por un usuario sin privilegios.
- **FFI a `ShellExecuteEx` desde la app:** obliga a levantar `unsafe_code = forbid` en un crate de interfaz.

## Decisiones del responsable (2026-10-29)

Aceptadas todas con la opción indicada entre paréntesis.

1. **Migrar o no la instancia heredada (§10):** ¿hay alguna instalación Windows real que conservar? *(No: reinstalar y unirse.)*
2. **Arranque de las redes unidas (§1):** automático retrasado o manual. *(Automático retrasado: la red vuelve tras reiniciar, como en Linux y macOS.)*
3. **Metadatos públicos (§2):** índice aparte legible por `Users`, o `list` solo para administradores. *(Índice aparte.)*
4. **GUID del adaptador (§3):** UUID de la red o aleatorio. *(UUID de la red.)*
5. **Código de unión como argumento elevado (§8):** paridad con macOS y Linux, o un protocolo de fichero cifrado. *(Paridad: el riesgo es el mismo y la mitigación es el token de un solo uso.)*
6. **Pruebas:** CI con redes preprovisionadas más una máquina Windows real del responsable para UAC y unión de extremo a extremo. *(Sí; sin la máquina, la fase 3 no puede darse por completa.)*

## Consecuencias

- Windows alcanza el modelo de redes de Linux y macOS sin tocar el motor ni el protocolo.
- Aparece un nuevo límite de confianza: un CLI elevado que crea servicios LocalSystem. Su mitigación es el §7; es lo primero que debe revisar un auditor de esta fase.
- Las identidades de varias redes conviven en `%ProgramData%\Atenea\networks`: `-Purge` deja de ser inocuo y el §9 lo trata.

## Estado de la implementación

- **Fase 1 (parcial).** Hecho: `Tun::create_with_guid` y el GUID del adaptador igual al UUID de la red (§3); `dns_domain_conflict` en Windows, de solo lectura y previo a `set_dns` (§6); ensayo de dos adaptadores simultáneos con GUID propio en el runner (`crates/tunnel/examples/two_adapters.rs`). Las tuberías por red (§5) no necesitan código nuevo: el agente ya recibe `ipc_socket`/`status_socket` por configuración; las fija la fase 2.
- **Despachador por nombre (§1): sin cambio de código.** Con un servicio de tipo propio (`SERVICE_WIN32_OWN_PROCESS`) el SCM ignora el nombre pasado al despachador y al registrar el manejador de control; la CI lo confirma con servicios `AteneaNetwork-<id8>` reales.
- **Fase 2 (completa en CI).** Hecho en Windows: `atenea network inspect`, `list`, `legacy`, `up`, `down`, `join` y `leave`. `list` lee el índice público `%ProgramData%\Atenea\public\<UUID>.json` (SYSTEM y Administradores escriben, `Users` leen; lo escribe `join`) y pregunta al SCM antes de abrir la tubería de estado `\\.\pipe\atenea-<id8>-status`. `up` exige consola elevada y un binario `%ProgramFiles%\Atenea\atenea-agent.exe` que `check_trusted_binary` considere no sustituible (§7); registra el servicio con la API del SCM (LocalSystem, arranque automático retrasado, reinicio a los 5/15/60 s también ante salidas con error) y lo inicia; si ya existe un servicio con ese nombre y otra línea de órdenes, no lo toca. `down` desactiva el reinicio, para el servicio, espera a la limpieza del agente y lo deshabilita. `join` replica el de Unix (código con token o solicitud, confirmación, CA del relay verificada, claves nuevas por red, `agent.toml` con las tuberías `atenea-<id8>-control`/`-status` y `status_gid = 545`, puerto UDP libre desde 51820 que respeta el cliente heredado, registro en el servidor) y no deja nada en disco si el servidor rechaza la solicitud. `leave` revoca el dispositivo (salvo `--offline`), para el servicio, ejecuta `cleanup` del agente, borra el servicio, las claves y el índice, y se puede repetir si algo falla antes del final. Los elementos comunes con Unix viven en `apps/cli/src/network_common.rs`. La ruta de las redes y la del índice público son fijas: no admiten sustitución por variable de entorno, porque `join` y `leave` escriben y borran en ellas como administrador y de `networks` sale la configuración que ejecuta un servicio LocalSystem. **Instalador y desinstalador (§9):** el instalador ya no registra `AteneaAgent` (respeta uno existente), al actualizar solo vuelve a arrancar los servicios de Atenea que estaban en marcha, y el desinstalador para y deshabilita los `AteneaNetwork-*` (tras ejecutar el `cleanup` de cada red) y rechaza `/PURGE` mientras haya redes unidas, salvo `/PURGE /FORCE`, que las borra sin revocarlas en el servidor y que no admite la confirmación interactiva descrita en el §9 (el desinstalador solo acepta `/PURGE` desde la línea de órdenes).
- **Fase 3 (implementada; falta la prueba en un Windows real).** La aplicación de escritorio une y gestiona redes en Windows (`apps/desktop/internal/backend`): `List` y el estado de cada red salen del CLI y de `\\.\pipe\atenea-<id8>-status`; `join`, `up`, `down` y `leave` se elevan con `powershell.exe Start-Process -Verb RunAs` (sin FFI) como **una sola llamada al CLI** (`join <código> --yes --up`; no hay shell que encadene), con el CLI fijo `%ProgramFiles%\Atenea\atenea.exe` (carpeta conocida del sistema; la aplicación no lee su DACL: confía en la del instalador). El canal de resultado es `--result <nonce>`: el CLI elevado crea `%ProgramData%\Atenea\ui-results\<nonce>\result.json` (escritura SYSTEM/Administradores, lectura `Users`, 4 KiB máximo) y retira los de más de un día; el nonce son 128 bits aleatorios de la aplicación y se valida en los dos lados antes de entrar en una ruta. Cancelar el diálogo (error 1223) sale con 126 y se muestra como «Cancelado». No hay migración de la instalación heredada ni publicación de servicios (ADR-024) en Windows.

## Límites de esta evidencia

Lo anterior se contrastó con el código, la documentación y la CI del runner de Windows (2026-11-15): un servicio `AteneaNetwork-<id8>` arranca y responde, dos adaptadores Wintun con GUID propio coexisten, dos redes unidas funcionan a la vez y la aplicación ejecuta `up`, `down` y `leave` mediante `powershell Start-Process -Verb RunAs` y el canal `result.json` (`tests/windows/network-checks.ps1`). **Sin comprobar** (el runner es siempre administrador, UAC no pide confirmación y no hay un Windows real): **el diálogo de UAC y su cancelación**, una cuenta estándar que lea el índice público y la tubería de estado, el arranque automático retrasado tras un reinicio real, la unión contra un control plane real (la CI usa uno falso, sin firmas), ARM64 físico, suspensión/Wi-Fi y la matriz entre sistemas.

Observaciones de documentación desfasada detectadas al investigar (sin corregir aquí): `docs/security/threat-model.md` T28 habla de «DNS global» residual cuando la implementación usa NRPT por dominio, y `ADR-011` aún dice que la elevación de macOS está pendiente mientras `ADR-013` registra después una prueba con diálogo real.
