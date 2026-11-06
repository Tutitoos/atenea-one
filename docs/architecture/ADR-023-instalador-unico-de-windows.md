<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-023: Un único instalador de Windows (agente, CLI, Wintun y aplicación)

- **Estado:** **Aceptado** por el responsable el 2026-11-06 con las ocho recomendaciones de § «Decisiones del responsable». **Sin código todavía** y nada probado en un Windows real; lo no comprobado va marcado. La fase 2 queda condicionada a releer la licencia de Wintun (decisión 3) y la fase 5 a una máquina Windows real.
- **Relacionados:** ADR-022 (el `.pkg` único de macOS; este ADR es su equivalente en Windows), ADR-008 (cliente Windows y excepción de Wintun), ADR-017 (un servicio y un adaptador por red; elevación con UAC), ADR-019 (aplicación sin privilegios, sin *updater*), ADR-021 (asistente de primera ejecución), ADR-011 §2 (autoridad = sistema operativo, binario fijo de administrador), ADR-009 (releases firmados con Ed25519).

## Contexto

El responsable quiere, como en macOS, **un solo instalador de Windows que lo deje todo hecho**: agente como servicio, CLI, DLL de Wintun y aplicación de escritorio. Hoy no existe ese instalador y la vía de Windows son piezas sueltas.

Hechos leídos en el código y en el flujo de publicación (rama `docs/adr-023-instalador-unico-de-windows`, 2026-11-06). Lo deducido está marcado **INFERIDO**.

**Qué hay para Windows**

- **ZIP con `install.ps1`.** `packaging/windows/build-zip.ps1` empaqueta `atenea.exe`, `atenea-agent.exe`, `install.ps1`, `uninstall.ps1`, `get-wintun.ps1` y la guía (líneas 13-19). **Se construye pero no se publica:** `release.yml` lo copia a `dist/` (líneas 142-147), por lo que entra en el manifiesto y en la atestación, pero la orden de publicación solo lista `*.pkg *.dmg *.deb *.rpm *.exe` más manifiesto y firma (línea 416).
- **`.exe` sueltos publicados.** El job `native` copia `atenea-cli-<v>-windows-<arq>.exe` y `atenea-agent-<v>-windows-<arq>.exe` a `dist/` (líneas 122-127) y el patrón `dist/*.exe` los publica (INFERIDO por el patrón; `docs/operations/release.md` líneas 58-60 los documenta como artefactos publicados). Un `.exe` suelto del agente no instala nada: sin DLL, servicio ni ACL.
- **`.exe` NSIS de la aplicación.** `packaging/desktop/windows/installer.nsi` instala **por usuario**, sin UAC (`RequestExecutionLevel user`, línea 21; `InstallDir $LOCALAPPDATA\Programs\Atenea One`, línea 20), no incluye agente ni CLI (líneas 3-5) y se detiene si falta el runtime WebView2 en vez de descargarlo (líneas 40-54). Se publica como `atenea-desktop-<v>-windows-<arq>.exe`; `/S` instala en silencio (lo ejercita `check-installer.ps1`, línea 14). NSIS 3.11 se instala por Chocolatey con versión fija (`release.yml` línea 227).
- **Instalación del cliente (`install.ps1`).** Exige administrador y rechaza reinstalar si existe `AteneaAgent` o `%ProgramFiles%\Atenea` (líneas 10-13); solo reutiliza un `%ProgramData%\Atenea` cuyo contenido pertenezca a SYSTEM/Administradores y sin puntos de reanálisis (líneas 14-27); fija la DACL privada `O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)` en los dos directorios (líneas 28-33); copia binarios y **descarga Wintun** (línea 38); añade lectura y ejecución para `Users` en `%ProgramFiles%\Atenea` (líneas 40-43); registra `AteneaAgent` como LocalSystem, arranque manual, con `sc failure` a 5/15/60 s y `failureflag 1` (líneas 45-53); **no arranca** el servicio (línea 61). Si algo falla, deshace el servicio y el directorio (líneas 54-60). Copia `uninstall.ps1` a `%ProgramFiles%\Atenea` (línea 35).
- **Desinstalación (`uninstall.ps1`).** Desactiva la recuperación SCM, para el servicio, ejecuta `cleanup`, borra el servicio y `%ProgramFiles%\Atenea`; conserva `%ProgramData%\Atenea` salvo `-Purge`, que borra también claves y estado (líneas 37-78).
- **Wintun hoy.** `get-wintun.ps1` descarga `wintun-0.14.1.zip` de wintun.net, comprueba el SHA-256 fijado **antes de extraer** y la firma Authenticode de la DLL de la arquitectura nativa, y conserva `WINTUN-LICENSE.txt` (líneas 8-30). ADR-008 §4 (línea 38) y `dependencies.md` (líneas 256-270) lo fijan: «no se versiona la DLL», sin hash configurable ni descarga de terceros. La excepción del responsable (2026-10-25, ADR-008 línea 48) se redactó con la DLL **descargada por el instalador y sin alojarla en el repositorio**. Licencia, leída por `dependencies.md` (líneas 259-261): uso mediante la API, **prohíbe modificar la DLL y retirar avisos**, y la redistribución solo se permite **junto al software que la usa mediante la API**.
- **SBOM y manifiesto.** `sbom.py` lee la versión y el hash de Wintun **del propio `get-wintun.ps1`** con expresiones regulares (líneas 53-59) y `release.yml` lo pasa con `--wintun` (línea 132). `manifest.py` asocia cada artefacto a su SBOM por el nombre: `atenea-desktop*` al inventario Go de su sistema, `atenea-one-*.pkg` a los dos de macOS y **cualquier `.exe` o `.zip` restante al de Rust de Windows** (líneas 115-125).
- **Plataforma publicada.** El job `native` corre solo en `macos-latest` y `windows-latest` (`release.yml` línea 95) y deriva `amd64`/`arm64` de `uname`: **solo se construye amd64** en Windows. `get-wintun.ps1` sí contempla ARM64 (líneas 10-14).
- **La aplicación en Windows hoy no puede unir redes.** `clientFor` marca `Supported` solo para darwin y linux (`apps/desktop/internal/backend/networks.go:50`); `List`, `Legacy`, `NetworkSocket` y `Elevate` devuelven «no soportado» o vacío en Windows (líneas 60-61, 71-73, 128-131, 168-170); `ElevationCommand` no tiene rama Windows (`ops.go` líneas 177-179). Tampoco el abrir el panel (`panel.go:47-48`), el `ping` (`ping.go:189-190`) ni el inicio con la sesión (`autostart.go:6`). La guía dice que en Windows el asistente informa de que «todavía no está soportado» (`instalacion-escritorio.md` línea 30).
- **Todo `atenea network` devuelve «no soportado todavía» en Windows** y la elevación con UAC no tiene implementación: ADR-017 (Aceptado, **sin código**) ya decidió el diseño (un servicio `AteneaNetwork-<id8>` y un adaptador `atn_<id8>` por red, canal de resultado `result.json`, §9: el instalador deja de registrar `AteneaAgent` por defecto).
- **CI.** El job `windows` de `ci.yml` construye el ZIP con binarios de depuración (líneas 126-127) y ejecuta `tests/windows/run.ps1`, que lo expande, lanza `install.ps1`, corre `smoke.py` y desinstala con `-Purge` (`run.ps1` líneas 47-48, 66, 72-74). `smoke.py` lee el ZIP (líneas 88-91) y da por hecho el servicio `AteneaAgent` (línea 150 y siguientes). El runner ya es administrador: **UAC no se ejercita**.
- **Firma.** Ningún artefacto de Windows está firmado con Authenticode (ADR-008 línea 49; `instalacion-escritorio.md` líneas 44-46: «SmartScreen avisará»). El certificado lo aparcó el responsable.

**Lo que se pide:** cerrar la brecha entre «piezas sueltas» y «un instalador», sin crear una segunda vía de instalación ni activos que confundan.

## Decisiones propuestas

Cada una con sus opciones, la **recomendación** y su porqué. Ninguna toca el protocolo ni los privilegios en tiempo de ejecución de la aplicación (ADR-019, ADR-021 se mantienen).

### 1. Qué instala y qué no

**Opciones**

- **A. Todo, sin servicio:** binarios, DLL, aplicación, acceso directo y desinstalador; los servicios los crea `atenea network` (ADR-017 §9).
- **B. Todo, con el servicio heredado `AteneaAgent` registrado y sin arrancar** (lo que hace `install.ps1`).
- **C. Solo el agente y el CLI** (sin aplicación): es el ZIP de hoy con otro envoltorio y no cumple lo pedido.

**Recomendación:** **B en las fases 2-3 y A desde la fase 3**, con la elección final del responsable. Razón: ADR-017 §9 ya decidió que el instalador **deja de registrar `AteneaAgent`** cuando existan los servicios por red, y ADR-013 descartó la instancia «predeterminada». Pero hasta que `atenea network` exista en Windows (fase 3) la única vía manual funcional es la guía actual (`device-init`, `enroll`, `agent-config`, `Start-Service AteneaAgent`), que necesita ese servicio, y el smoke existente también. Registrar el servicio heredado una versión y quitarlo en la siguiente deja un coste de migración (ADR-017 §10 decidió no migrar la heredada porque no hay instalaciones reales conocidas); si el responsable prefiere no dejar ese rastro, **A desde el principio** es limpio, pero la fase 2 entrega un instalador que no puede conectar nada y la prueba de CI tiene que registrar su propio servicio.

**Qué instala** (por máquina, ver decisión 4):

| Componente | Destino | Notas |
| --- | --- | --- |
| `atenea-agent.exe`, `atenea.exe` | `%ProgramFiles%\Atenea` | Ruta fija exigida por ADR-017 §7 para el servicio. |
| `wintun.dll`, `WINTUN-LICENSE.txt` | `%ProgramFiles%\Atenea` | Decisión 3. |
| `atenea-desktop.exe`, `BUILD-INFO.json` | `%ProgramFiles%\Atenea` (INFERIDO: misma carpeta; el responsable puede preferir `…\Atenea One`) | Decisión 4: propiedad de administrador, como el `.app` de root en macOS (ADR-022 línea 46). |
| Acceso directo «Atenea One» | Menú Inicio de todos los usuarios | Sin inicio automático (Windows no lo soporta aún, `autostart.go:6`). |
| `uninstall.exe` y entrada de desinstalación | `%ProgramFiles%\Atenea` y `HKLM` | Decisión 4. |
| Servicio `AteneaAgent` (solo opción B) | SCM, LocalSystem, arranque manual | Idéntico a `install.ps1` líneas 45-53, **sin arrancarlo**. |

**Qué NO hace el instalador:** no se une a ninguna red; no arranca ningún servicio que no estuviera ya en marcha antes de una actualización; no toca el cortafuegos (ADR-008, ADR-017 §4), el DNS global, las rutas por defecto ni las reglas NRPT; no añade `PATH` global (`instalacion-windows.md` línea 23); no descarga nada ni hace peticiones de red; no instala WebView2 (se detiene y lo dice, como `installer.nsi` líneas 40-54); no crea tareas programadas ni claves de inicio; no lanza la aplicación al terminar. **Este último punto es deliberado:** un instalador elevado que abre la aplicación la ejecutaría con el token elevado, contra ADR-019 (aplicación sin privilegios). El usuario abre la aplicación desde el menú Inicio, sin elevar.

### 2. Tecnología: NSIS frente a WiX/MSI

| | NSIS | WiX / MSI |
| --- | --- | --- |
| Ya en el repositorio | **Sí**: `installer.nsi`, `build-installer.ps1`, NSIS 3.11 fijado en CI, prueba de instalar/arrancar/desinstalar | No: herramienta y proyecto nuevos |
| Instalación silenciosa | `/S` (ya probado); parámetros propios (`/PURGE`) | `msiexec /i … /qn` estándar |
| Actualización en sitio | A mano: detectar la instalación previa, parar servicios, sustituir (la lógica es nuestra) | **Nativa** (actualización mayor), con reversión transaccional |
| Servicio, ACL, reversión | Llamadas a `sc.exe`/`icacls` o a un asistente de PowerShell; sin reversión transaccional | Tablas de servicio y permisos; reversión |
| GPO / Intune | Intune acepta un `.exe` empaquetado con regla de detección; **GPO de instalación de software solo admite MSI** | Soporte completo |
| Coste en CI | Casi nulo (reutiliza el job y la versión fijada) | Fijar una versión de WiX, escribir el XML, probar `msiexec`; **comprobar la licencia de la versión de WiX elegida** antes de adoptarla *(INFERIDO: las versiones recientes cambiaron de licencia; no se ha leído)* |
| Un solo `.exe` | Sí | Un `.msi` (un `.exe` de arranque exigiría además Burn) |

**Recomendación: NSIS.** Es lo que ya construye, instala y prueba el repositorio, y la mayor parte del trabajo es *lógica de instalación* (comprobaciones de `install.ps1`) que se escribe igual en los dos. La ventaja real de MSI (transacción, GPO, actualización mayor nativa) solo pesa con despliegue corporativo, y no hay petición de ello (INFERIDO: no consta en el repositorio). Se reabre si aparece GPO o Intune con exigencia de MSI.

**Cómo se hace la lógica (a validar en la fase 2):**

- Reutilizar la lógica probada de `install.ps1` y `uninstall.ps1` en un asistente de PowerShell 5.1 que **no se extrae a `%TEMP%`** (el usuario podría sustituirlo entre la extracción y la ejecución elevada): primero se crea `%ProgramFiles%\Atenea` con la DACL protegida (`icacls`), se extrae allí y se ejecuta desde allí.
- NSIS es de 32 bits: hay que usar `$PROGRAMFILES64` y la vista de 64 bits del registro para que la ruta sea `%ProgramFiles%\Atenea` (nativa) y no `Program Files (x86)`; ADR-017 §7 exige la ruta nativa (comprobable en CI, ver decisión 8).
- La plantilla es de tipo máquina (`RequestExecutionLevel admin`); se evalúa compartir con `installer.nsi` la comprobación de WebView2 por `!include`.

### 3. Wintun: incrustar o descargar

**Opciones**

- **A. Incrustar** la DLL oficial en el instalador: la construcción descarga `wintun-0.14.1.zip` de la URL oficial, **verifica el SHA-256 fijado** (`07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51`) y la firma Authenticode antes de empaquetar, y mete la DLL **sin modificar** con `WINTUN-LICENSE.txt`.
- **B. Descargarla al instalar** (`get-wintun.ps1` de hoy, adaptada al instalador): exige red, HTTPS y comprobación en el equipo.

| | A. Incrustar | B. Descargar |
| --- | --- | --- |
| Instalación sin red / en entornos cerrados | Funciona | No funciona |
| Un solo fichero verificable con el manifiesto del release | **Sí**: el hash del instalador cubre la DLL | No: depende de un tercero en el momento de instalar |
| Superficie en la instalación | Ninguna petición de red; una descarga menos en un proceso elevado | Cliente HTTP en un proceso elevado; GitHub/wintun.net deben estar disponibles |
| Encaje con la excepción del 2026-10-25 | **Cambia el hecho de base**: hoy Atenea no redistribuye la DLL, la descarga el usuario de wintun.net. Incrustarla hace que Atenea **la redistribuya** desde GitHub | Sin cambio |
| Licencia | La redistribución «junto al software que la usa mediante la API» está permitida según la lectura de `dependencies.md` (líneas 259-261); el instalador debe mantener la DLL sin modificar y los avisos | Sin cambio |

**Recomendación: A, con confirmación expresa del responsable.** Razón: la instalación deja de depender de la red y de un tercero, y el hash de la DLL queda protegido por el mismo manifiesto Ed25519 que el resto. Pero **la excepción aprobada el 2026-10-25 se redactó con la descarga del instalador y sin alojar la DLL**: incrustar es una redistribución, dentro de lo que la licencia permite según la lectura registrada, **no releída para este ADR** (no se ha vuelto a leer `LICENSE.txt` del ZIP oficial). Antes de aceptar la opción A el responsable debe **renovar la excepción** (o confirmar que su alcance cubre esta redistribución) y se debe releer la licencia del ZIP.

**Condiciones si se acepta A:**

- El hash y la URL fijados dejan de vivir en `get-wintun.ps1` (que desaparece, decisión 7). `sbom.py` los lee de ahí con expresiones regulares (líneas 53-59): se mueven a un fichero propio y se actualiza el lector; el SBOM sigue declarando Wintun con su hash.
- La construcción falla si el hash o la firma Authenticode no coinciden, y la DLL no se modifica en ningún paso; el instalador conserva `WINTUN-LICENSE.txt` junto a ella.
- La DLL incrustada es la de la arquitectura del instalador (`bin/amd64`; ver ARM64 en la decisión 7).
- Sigue sin versionarse la DLL en el repositorio (ADR-008).
- La firma de Wintun (WireGuard LLC) no firma Atenea (ADR-008 línea 49).

### 4. Alcance y elevación; actualización y desinstalación

**Opciones**

- **A. Por máquina**, con una elevación UAC al instalar: binarios en `%ProgramFiles%`, datos en `%ProgramData%`, accesos directos para todos los usuarios, entrada de desinstalación en `HKLM`.
- **B. Por usuario**, sin UAC (como el `.exe` NSIS de la aplicación hoy).
- **C. Mixta:** servicios y CLI por máquina, aplicación por usuario.

**Recomendación: A.** Razón: B **no puede** registrar un servicio LocalSystem ni crear una DACL en `%ProgramData%\Atenea`, y ADR-017 §7 exige que el binario del servicio esté en `%ProgramFiles%\Atenea` sin escritura para no administradores (equivalente de la comprobación `trusted` de Unix). C reintroduce el riesgo que ADR-022 evitó en macOS: una aplicación en carpeta del usuario puede ser sustituida por un proceso del mismo usuario. La aplicación no ejecuta con privilegios algo reemplazable (la elevación llama al CLI fijo, ADR-021), pero dejarla en `%ProgramFiles%` evita además dos copias y dos desinstaladores, y mantiene «un binario, un propietario».

**Permisos:**

- `%ProgramFiles%\Atenea`: DACL protegida con SYSTEM y Administradores con control total y `Users` con lectura y ejecución (igual que `install.ps1` líneas 28-43). Solo SYSTEM/Administradores escriben.
- `%ProgramData%\Atenea`: DACL privada `O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)` (`install.ps1` línea 31). Se reutiliza solo si todo su contenido es de SYSTEM/Administradores y sin puntos de reanálisis (líneas 14-27); si no, el instalador **aborta sin tocarlo**: cualquier usuario puede crear esa carpeta antes de la instalación.
- Por usuario quedan solo las preferencias de la aplicación (`first-run.json`, `notifications.json`), que el instalador no crea ni toca.

**Instalación sobre una instalación previa (actualización en sitio):**

- Detecta la instalación por su clave de desinstalación y por los servicios `AteneaAgent` y `AteneaNetwork-*`.
- Para cada servicio en marcha: lo para con plazo de 30 s (como `uninstall.ps1` líneas 23-35); si no para, **aborta sin tocar los ficheros** (no fuerza la muerte del servicio).
- Sustituye binarios y DLL, **conserva `%ProgramData%\Atenea` íntegro** (claves, identidades de redes, estado antirrollback) y no ejecuta `cleanup` de la red ajena.
- Rearranca solo los servicios que estaban en marcha antes de actualizar (su configuración ya existe); nunca arranca uno detenido. Es una elección para el responsable (alternativa: dejarlos todos detenidos y que el usuario lo vea en la aplicación).
- Si la aplicación está abierta, el instalador **no puede** sustituir `atenea-desktop.exe` (fichero en uso): pide cerrarla y, en silencio, termina con error y código distinto de cero.
- Rechaza instalar una versión anterior sobre una posterior salvo orden expresa (comportamiento propuesto, no existe hoy).
- Una instalación heredada de `install.ps1` (sin clave de desinstalación; `%ProgramFiles%\Atenea` con `uninstall.ps1`) **no se adopta**: el instalador se detiene e indica ejecutar su `uninstall.ps1` (el que `install.ps1` copió en la línea 35). Si hay instalaciones reales de ese tipo es **desconocido**; ADR-017 §10 las da por inexistentes.
- Una copia por usuario de la aplicación del `.exe` actual (`HKCU`, `%LOCALAPPDATA%\Programs\Atenea One`) **no se ve desde un proceso elevado con otra cuenta** (INFERIDO: depende de si la UAC eleva con la misma cuenta). Es una razón más para retirar el `.exe` por usuario (decisión 7) y documentar su desinstalación.

**Desinstalación** (`uninstall.exe`, y entrada en «Aplicaciones»):

- Replica `uninstall.ps1`: desactiva la recuperación SCM, para y borra `AteneaAgent` y `AteneaNetwork-*` (ADR-017 §9), ejecuta `cleanup` y quita `%ProgramFiles%\Atenea`, el acceso directo y la clave. Si `cleanup` o la parada fallan, **conserva la instalación** para poder repararla (como hoy).
- **Conserva `%ProgramData%\Atenea` por defecto.** Solo `/PURGE` (interactivo: casilla desmarcada y con advertencia irreversible) lo borra, y según ADR-017 §9 **rechaza** borrar identidades si existen redes unidas, salvo confirmación tras listarlas y recomendando `atenea network leave` antes.
- No toca los datos por usuario de la aplicación (`installer.nsi` líneas 73-81 ya lo declara).

### 5. Cómo se une a una red después de instalar

Esta decisión determina si «ya funciona».

**Opciones**

- **A. La aplicación une desde su asistente (ADR-021), con UAC por operación.** Exige (1) `atenea network join|up|down|leave|list|inspect` en Windows (ADR-017 fases 1-2), (2) en la aplicación, rama Windows en `clientFor`, `List`, `Legacy`, `NetworkSocket`, `Elevate` y `ElevationCommand` (los puntos de «Contexto»), y (3) resolver la salida de la operación elevada.
- **B. Scripts y CLI manual**, como hoy (guía `instalacion-windows.md` líneas 49-75, con el servicio heredado). No cambia, no necesita ADR-017, no cumple «lo deja todo hecho».

**Dependencia de ADR-017:** A **depende por completo** de ADR-017 (§1-§9) y de su fase 3 (aplicación: UAC, tuberías y canal de resultado). Sin esas fases, el instalador de este ADR **instala pero no puede unir** desde la aplicación; B queda como único camino.

**El problema del canal de salida.** La aplicación lee la salida estándar de la operación elevada en macOS y Linux. En Windows `Start-Process -Verb RunAs` **no permite redirigir** la salida *(limitación conocida de PowerShell, no probada aquí)* y el crate de la app tiene `unsafe_code = forbid` (ADR-017 línea 24). Opciones para el resultado:

| | Cómo | Valoración |
| --- | --- | --- |
| **A1. Fichero de resultado en `%ProgramData%\Atenea\ui-results\<nonce>\`** (ADR-017 §8, ya aceptado) | El lado elevado crea el directorio con DACL de escritura solo SYSTEM/Administradores y lectura para `Users`; el `nonce` de 32 hex lo elige la app y se valida; la app relee `network list` como fuente de verdad y usa el fichero solo para el mensaje de error | **Recomendada.** El proceso elevado **nunca escribe en una ruta que controle el usuario sin privilegios** |
| **A2. Fichero en una carpeta privada del usuario** (propuesta a evaluar) | La operación elevada escribe el resultado en una carpeta del perfil del usuario, legible sin elevar | **Desaconsejada tal como está.** Es una escritura privilegiada en una ruta controlada por un proceso sin privilegios: puede sustituir la carpeta por una unión o un enlace antes de que el proceso elevado escriba (reanálisis/TOCTOU). ADR-017 la descartó (§8, «Alternativas descartadas»). Además, con una cuenta estándar la UAC pide credenciales de **otro** administrador y «el perfil del usuario» del proceso elevado sería el de esa otra cuenta *(INFERIDO: comportamiento conocido de Windows, no probado aquí)*. Solo sería aceptable si el lado elevado abre el destino sin seguir enlaces y comprueba el propietario, lo que añade lógica sin ventaja sobre A1 |
| **A3. Solo el código de salida** | La app usa `-PassThru` y lee `ExitCode`; clases de error por código y relectura de `network list` | Complemento sin fichero: evita el canal, pero pierde el mensaje detallado y, con `Start-Process -PassThru`, Windows PowerShell 5.1 puede devolver `ExitCode = $null` (incidencia PowerShell #5421, citada en `instalacion-windows.md` línea 103). **INFERIDO y no probado**; valdría como respaldo, no como mecanismo único |
| **A4. Ayudante residente** (servicio o proceso con una tubería de órdenes para la app) | Sin UAC por operación | **Descartada.** Es un gestor residente que ADR-013 y ADR-017 ya descartaron: más superficie de ataque y aislamiento por proceso roto |

**Paridad aceptada** (ADR-017 §8, decisión 5 del responsable): el código de unión, que puede llevar un token de un solo uso, viaja como argumento de la orden elevada, como con `osascript` y `pkexec`.

**Recomendación:** **A con A1** como destino; **B sigue siendo el camino documentado y soportado hasta que A pase la fase 4**. No se promete «ya funciona» antes de esa fase (ver «Cuándo funciona de verdad»).

### 6. Firma de código

**Hechos.** Los instaladores de Windows van sin Authenticode (ADR-008 línea 49) y el responsable aparcó el certificado. Un instalador por máquina sin firma **pide elevación** y la ventana de UAC muestra editor «desconocido» *(comportamiento conocido de Windows, no comprobado aquí)*. SmartScreen avisará de un fichero descargado sin firma ni reputación (`instalacion-escritorio.md` líneas 44-46). **Smart App Control**, en equipos con Windows 11 donde esté activo, **no tiene excepción por aplicación**: Microsoft indica que solo se puede desactivarlo o pedir al desarrollador que firme ([preguntas frecuentes de Smart App Control](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions)); cómo trata exactamente un instalador sin firma que crea un servicio **no se ha probado**. Los antivirus pueden marcar con más frecuencia un ejecutable sin firma que escribe un servicio *(INFERIDO)*.

**Opciones**

- **A. Seguir sin firma** y documentarlo.
- **B. Firmar** el instalador y los binarios que lleva (incluido el desinstalador, que NSIS genera aparte) con un certificado de firma de código.

**Recomendación: A ahora, B como condición para presentarlo como instalador de uso general.** El certificado es una decisión y un gasto del responsable, y el instalador funciona sin él para pruebas. Con B cambiaría: el editor mostrado en UAC y en SmartScreen, y la reputación (no se promete que desaparezca el aviso desde el primer día: depende de la reputación acumulada), y probablemente menos falsos positivos de antivirus *(INFERIDO)*. Requiere firmar también `atenea.exe`, `atenea-agent.exe`, la aplicación y el desinstalador, y cambia el job de release (secretos, sello de tiempo).

**Cómo documentar el aviso sin pedir desactivar protecciones:** decir que el instalador **no está firmado por Atenea**, que Windows lo avisará por eso, y que la comprobación disponible es la del release (firma Ed25519 del manifiesto con la clave pública obtenida por otro canal, ADR-009, y `verify.sh`) antes de ejecutar. Si SmartScreen ofrece la opción de ejecutar tras «Más información», describirla como lo que es; **no** pedir desactivar SmartScreen, Smart App Control, el antivirus ni UAC. Si Smart App Control bloquea el instalador, la guía debe decir **honestamente** que hoy no hay una vía que no pase por cambiar esa protección o por esperar a la firma, y dejar la decisión al usuario.

### 7. Publicación y limpieza

**Nombre del activo:** `atenea-one-<versión>-windows-<arq>.exe`, igual que `atenea-one-<versión>-macos-<arq>.pkg` (ADR-022). Sin «setup», «install» ni «update» en el nombre (Windows aplica heurísticas de compatibilidad a esos ejecutables; `build-installer.ps1` línea 26).

**Qué entra en la publicación y en la verificación**

- El `.exe` único entra en el manifiesto, la atestación y `verify.sh` como los demás. `manifest.py` debe asociarlo a **dos** SBOM, `rust-Windows.cdx.json` y `go-desktop-Windows.cdx.json`, con una rama propia como la del `.pkg` único (líneas 119-121); hoy un `atenea-one-*.exe` caería en la rama `.exe` y solo recibiría el inventario de Rust (línea 124), y la aplicación quedaría sin inventario. El SBOM de Rust sigue declarando Wintun con su hash.
- Nuevo job `suite-windows` en `release.yml`, análogo a `suite-macos` (líneas 260-293): **necesita los jobs `native` y `desktop`**, construye el instalador, lo prueba (decisión 8), lo atesta y lo sube; `assemble` lo incluye en sus dependencias y en el patrón de artefactos (línea 296 y siguientes) y la orden de publicación ya cubre `dist/*.exe`.
- El instalador necesita el **binario** `atenea-desktop.exe`, que el job `desktop` hoy no sube (solo sube `dist/*`). Debe subirse como artefacto **interno** que `assemble` no mezcle en `dist/` (el mismo cuidado que con `desktop-ui`, `release.md`). Anidar el instalador por usuario dentro del nuevo no se recomienda.

**Limpieza (corte limpio, sin dos vías)**

| Hoy | Propuesta | Razón |
| --- | --- | --- |
| ZIP `atenea-client-unsigned.zip`, `install.ps1`, `uninstall.ps1`, `get-wintun.ps1`, `build-zip.ps1` | **Se eliminan** del repositorio y de `dist/`; `run.ps1`, `smoke.py` y `ci.yml` pasan al instalador | Ya no se publica; dos vías confunden y se desincronizan |
| `atenea-cli-<v>-windows-<arq>.exe`, `atenea-agent-<v>-windows-<arq>.exe` sueltos | **Dejan de publicarse**; pasan a entradas internas del instalador (como los binarios de macOS, que «solo viajan dentro del `.pkg`», `release.yml` línea 124) | Un `.exe` suelto del agente no instala nada: una descarga que parece válida y no lo es |
| `atenea-desktop-<v>-windows-<arq>.exe` (por usuario) | **Opción A: dejar de publicarlo** (recomendada) u **opción B: conservarlo** como el `.dmg` de macOS (ADR-022) | En Windows la aplicación sin cliente no hace nada (decisión 5) y conserva dos instalaciones posibles que conviven mal (decisión 4). Si se conserva, debe llevar el aviso de que no instala el cliente |
| Docs: `instalacion-windows.md`, `instalacion-escritorio.md`, `release.md` | Reescritas al instalador; el flujo manual de enrolar (guía líneas 49-75) se conserva como camino B de la decisión 5 | Una sola vía documentada |

**Arquitectura:** `windows-latest` es amd64 y no hay job ARM64 (`release.yml` línea 95). **Recomendación: publicar solo `amd64`** hasta poder construir y probar ARM64 (el ZIP oficial de Wintun trae `bin/arm64`, así que la DLL no es el obstáculo; el obstáculo es poder probarlo). Publicar un `arm64` no probado se descarta. La viabilidad de un runner ARM64 de GitHub y de compilar la aplicación (Go) y el agente (Rust) para ARM64 está **sin comprobar**.

### 8. Verificación en CI

Reutiliza `tests/windows/smoke.py`, `run.ps1` y la ejecución en runner desechable elevado, sustituyendo ZIP e `install.ps1` por el instalador **real** (el que se publica; un instalador de prueba distinto no prueba lo que se entrega).

**Pruebas del instalador (nuevas, `tests/windows/check-suite.ps1` o equivalente)**

1. `atenea-one-*.exe /S` termina con código 0 en un runner sin la instalación previa; **no arranca** ningún servicio.
2. Ficheros presentes en `%ProgramFiles%\Atenea` (nativo, no `Program Files (x86)`): `atenea.exe`, `atenea-agent.exe`, `atenea-desktop.exe`, `wintun.dll`, `WINTUN-LICENSE.txt`, `BUILD-INFO.json` (con versión y commit esperados), `uninstall.exe`.
3. **DLL con el hash esperado:** SHA-256 de la DLL instalada igual al fijado; `Get-AuthenticodeSignature` válido. La construcción ya lo exige: el test lo comprueba también sobre el resultado instalado.
4. **DACL:** `%ProgramData%\Atenea` con `O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)`; `%ProgramFiles%\Atenea` sin escritura para no administradores (reutiliza los lectores de `smoke_support.py`).
5. **Servicio** (opción B de la decisión 1): `AteneaAgent` registrado, LocalSystem, arranque manual, **detenido**, con la ruta fija entrecomillada y las acciones de recuperación 5/15/60 s y el indicador de fallo (lectura de `FailureActions`, como `smoke.py` líneas 246-250). En la opción A: **ningún** servicio de Atenea registrado.
6. Entrada de desinstalación en `HKLM` y acceso directo en el menú Inicio de todos los usuarios; la aplicación arranca con `apps/desktop/scripts/smoke.py` (ya existente) desde la ruta instalada.
7. **Actualización en sitio:** instalar la versión N, crear datos de prueba en `%ProgramData%\Atenea` (propiedad de SYSTEM), instalar de nuevo (otra versión de construcción), comprobar que los datos siguen intactos con su propietario y DACL y que los binarios son los nuevos; con un servicio en marcha, que se pare, se sustituya y se rearranque solo si estaba en marcha. Con un fichero de ejecutable en uso, termina con error y deja la instalación anterior utilizable.
8. **Negativas:** `%ProgramData%\Atenea` preexistente con contenido ajeno o con enlace → aborta sin tocar; instalación heredada → se detiene; WebView2 ausente → se detiene (si el runner permite simularlo, **sin comprobar**).
9. **Desinstalación:** `uninstall.exe /S` retira binarios, servicio, acceso directo y clave y **conserva** los datos; `uninstall.exe /S /PURGE` borra los datos (y, con la guarda de ADR-017 §9, se niega si hay redes unidas).
10. `smoke.py` (humo del agente: Wintun, rutas, NRPT, DACL de tuberías, parada, caída forzada, limpieza) pasa a partir de la instalación del `.exe`, no del ZIP. Su comprobación del ZIP (líneas 88-91) se reemplaza por las de los puntos 2-3.

**Coste en CI:** el job `windows` de `ci.yml` hoy compila solo Rust en depuración y construye el ZIP; para probar el instalador real necesita también el binario de escritorio (otro job con Go y la interfaz compilada, `ci.yml` `desktop`) *(INFERIDO: no se ha estimado el tiempo; el paso actual tiene un límite de 12 minutos)*. Se acepta ese coste antes que un instalador de prueba sin aplicación.

**Lo que el runner NO puede probar:** la elevación UAC interactiva (el runner ya es administrador; la aceptación de UAC y el editor «desconocido»); SmartScreen y Smart App Control; la marca de origen web (Mark of the Web) de un `.exe` descargado; antivirus de equipos reales; ARM64 físico; instalar con la aplicación abierta de un usuario real; suspensión, cambios de Wi-Fi y una instalación sobre una máquina con configuración previa del usuario; WebView2 ausente en la práctica. Estas pruebas son de la fase 5 en un Windows 11 real.

## Alternativas descartadas

- **Seguir con el ZIP y `install.ps1` como vía oficial:** no cumple «un instalador», exige abrir PowerShell elevado y adaptar la política de ejecución, y no se publica hoy.
- **Que el asistente de la aplicación descargue e instale el cliente:** descartado en ADR-022 y ADR-021 (un *updater* sin ADR).
- **Instalación por usuario (`RequestExecutionLevel user`) con servicio:** no puede crear un servicio LocalSystem ni la DACL de `%ProgramData%\Atenea`.
- **Anidar el instalador por usuario de la aplicación dentro del nuevo:** dos instaladores, dos desinstaladores y dos ubicaciones.
- **Ayudante residente para la elevación:** descartado en ADR-013 y ADR-017.
- **Extraer el asistente de PowerShell a `%TEMP%` y ejecutarlo elevado:** abre una ventana para sustituirlo entre la extracción y la ejecución.
- **Que el instalador abra la aplicación al terminar:** la aplicación heredaría el token elevado.

## Consecuencias

- Windows tiene, como macOS, **un solo instalador** con la misma versión de agente, CLI, DLL y aplicación, y **desaparecen** el ZIP y los `.exe` sueltos como vías de instalación.
- El instalador por máquina es un nuevo límite de confianza: un `.exe` sin firma que se ejecuta elevado, escribe `%ProgramFiles%`, registra un servicio LocalSystem y fija ACL. Su integridad la garantiza solo la comprobación del manifiesto por el usuario (ADR-009); es lo primero que debe revisar una auditoría de esta fase.
- Incrustar Wintun convierte a Atenea en redistribuidora de esa DLL (decisión 3): **la excepción del 2026-10-25 debe renovarse** si el responsable elige A.
- El instalador **no hace que Windows funcione por sí solo**: hasta que ADR-017 esté implementado y la aplicación pueda unir (fases 3 y 4), el instalador entrega el cliente y la aplicación instalados, y la unión sigue siendo manual.
- Coste: una plantilla NSIS nueva, un job `suite-windows`, cambios en `manifest.py`, `sbom.py`, `run.ps1`, `smoke.py`, `ci.yml` y las tres guías, y el mantenimiento de la lógica de actualización que MSI daría gratis.

## Fases y criterios de salida

| Fase | Alcance | Criterio de salida |
| --- | --- | --- |
| **1. ADR aceptado** | El responsable decide las ocho decisiones y, si elige incrustar, renueva la excepción de Wintun | Registro de la decisión en este ADR y en el índice |
| **2. Instalador construido y verificado en CI, sin unirse** | Plantilla NSIS por máquina; job `suite-windows`; manifiesto y SBOM; limpieza del ZIP y los `.exe` sueltos; las pruebas 1-10 de la decisión 8 | Verde en runner (**instalar, actualizar conservando datos, desinstalar, smoke**); `verify.sh` y `manifest.py` aceptan el activo; no queda ZIP ni `.exe` suelto en `dist/` |
| **3. `atenea network` en Windows (ADR-017 fases 1-2)** | Servicio por red, adaptador por red, metadatos públicos, guarda de DNS, comprobación de confianza del binario; el instalador deja de registrar `AteneaAgent` (decisión 1) | Aceptación de dos redes a la vez en runner, como exige ADR-017; el instalador de la fase 2 sigue verde |
| **4. El asistente de la aplicación une desde Windows (ADR-017 fase 3)** | Rama Windows en la aplicación (`clientFor`, `List`, `Legacy`, `Elevate`, `ElevationCommand`), canal de resultado A1, pruebas con ejecutor simulado | Pruebas unitarias; **unir una red desde el asistente en un Windows real** (prueba de la fase 5 solapada) |
| **5. Prueba en un Windows 11 real del responsable** | Instalar con SmartScreen y UAC reales, abrir la aplicación sin elevar, unir con el asistente (UAC por operación), reiniciar y comprobar que la red vuelve, actualizar el instalador conservando datos, desinstalar | Resultado observado y registrado; hasta entonces **ninguna afirmación** de que «funciona» en Windows |

## Cuándo «ya funciona de verdad»

- **Fase 2 no basta:** instala y se verifica, pero el asistente de la aplicación sigue diciendo que Windows no está soportado y la unión es manual.
- **No antes de la fase 4**: el responsable puede instalar, abrir la aplicación y unirse a una red desde el asistente.
- **Comprobado, solo tras la fase 5**: la fase 4 con pruebas en el runner no demuestra UAC, SmartScreen ni equipos reales (ADR-017 línea 94 y ADR-008 línea 50). Esa afirmación solo se hace cuando consta la prueba observada en un Windows real.

## Riesgos y lo que no se sabe

- **Wintun y varios adaptadores:** no está comprobado que el controlador admita varios adaptadores simultáneos con GUID propios (ADR-017, «Límites de esta evidencia»). Condiciona ADR-017, no el instalador, pero la fase 3 puede fallar.
- **Licencia y excepción:** incrustar es redistribuir; la lectura de la licencia es de 2026-10-25 y no se ha repetido. Si el responsable no renueva la excepción, queda B (descargar), con sus dependencias de red.
- **SmartScreen, Smart App Control y antivirus:** el comportamiento con un instalador sin firma que registra un servicio **no se ha probado**. Puede ser bloqueo sin excepción por aplicación, falsos positivos o ambos. Mitigación real: firmar (decisión 6); mientras tanto, solo se documenta.
- **Lógica de actualización propia:** sin la transacción de MSI, un fallo a mitad de la copia puede dejar una instalación mixta. Se mitiga con parada previa, comprobaciones antes de copiar, conservar datos y pruebas 7-8, pero **no con reversión automática** *(INFERIDO: NSIS no ofrece una nativa)*.
- **Asistente de PowerShell elevado:** ejecutar un script como administrador desde una carpeta protegida reduce el riesgo, pero el diseño (extraer en la carpeta ya bloqueada) debe validarse en la fase 2; la alternativa es escribir las comprobaciones en NSIS con `icacls`, con más código que probar.
- **Ruta nativa:** NSIS de 32 bits y `Program Files (x86)`; si falla la comprobación de la decisión 8, ADR-017 §7 rechaza el servicio.
- **Canal de resultado (decisión 5):** A1 depende de que una carpeta con lectura para `Users` dentro de un árbol privado se pueda recorrer y leer sin elevar; ADR-017 lo asume y **no está probado**. El comportamiento de `Start-Process -Verb RunAs -Wait -PassThru` (código de salida) tampoco.
- **ARM64:** no hay compilación, runner ni prueba; solo se publica amd64 hasta tenerlos.
- **Instalaciones heredadas y copia por usuario de la aplicación:** no se sabe cuántas hay. El instalador no las adopta ni las ve.
- **Aplicación abierta o servicio que no para:** la actualización aborta; no hay evidencia de cuánto tardan en parar los servicios con redes reales.
- **WebView2:** el instalador no lo trae; si falta, se detiene. No se ha medido qué porcentaje de equipos lo carece (Windows 11 y Windows 10 al día lo traen con Edge, `instalacion-escritorio.md` líneas 44-46).
- **Cortafuegos de Windows:** sin reglas automáticas (ADR-008, ADR-017 §4); la conectividad directa se apoya en el seguimiento de UDP saliente. No comprobado en equipos reales.
- **Coste de CI:** probar el instalador real obliga a construir también la aplicación en el job de Windows; sin medir.
- **Todo lo anterior se leyó en el código y en la documentación; no se ha ejecutado nada en Windows** para este ADR.

## Pruebas de salida por fase

1. **Fase 1:** el responsable registra la decisión de cada punto (sección siguiente); no hay código.
2. **Fase 2:** las pruebas 1-10 de la decisión 8 en runner; `check-headers`, `check-docs` y el escaneo de datos reales sin incidencias; `manifest.py create/verify` con el activo nuevo y sus dos SBOM; ningún ZIP, `install.ps1`, `get-wintun.ps1` ni `.exe` suelto en `dist/`.
3. **Fase 3:** las de ADR-017 fase 2 (dos redes simultáneas en el runner) más el instalador sin servicio heredado.
4. **Fase 4:** pruebas unitarias de la aplicación con ejecutor simulado para Windows; la prueba de superficie sigue pasando; el asistente **no** llama a `network_apply` sin pulsación (ADR-021).
5. **Fase 5:** lista de comprobación en el Windows 11 real: aviso de SmartScreen observado, UAC mostrado, instalación sin red, aplicación abierta sin elevar, unión con el asistente, reinicio, actualización con datos conservados, desinstalación con y sin `/PURGE`. Cada punto marcado con lo observado, no con lo esperado.

## Decisiones del responsable

El responsable **aceptó el 2026-11-06 las ocho recomendaciones** tal como se enuncian a continuación. Cada decisión admite la opción alternativa de su sección si más adelante se revisa.

1. **Qué instala (decisión 1):** todo menos unirse; servicio heredado `AteneaAgent` solo en las fases 2-3 (opción B), nada de servicio desde la fase 3 (opción A).
2. **Tecnología (decisión 2):** NSIS; WiX/MSI solo si aparece un despliegue por GPO con exigencia de MSI.
3. **Wintun (decisión 3):** incrustar la DLL oficial con hash y firma comprobados al construir; **requiere renovar la excepción del 2026-10-25** y releer la licencia.
4. **Alcance (decisión 4):** por máquina con una UAC, aplicación en `%ProgramFiles%`; conservar datos por defecto y purgar solo con `/PURGE`; no adoptar instalaciones heredadas.
5. **Unirse (decisión 5):** el asistente de la aplicación con UAC por operación y canal de resultado en `%ProgramData%\Atenea\ui-results` (ADR-017 §8), **no** una carpeta del usuario; manual como respaldo hasta la fase 4.
6. **Firma (decisión 6):** seguir sin firma por ahora y documentarlo sin pedir desactivar protecciones; firmar antes de ofrecerlo como instalador de uso general.
7. **Publicación (decisión 7):** `atenea-one-<v>-windows-amd64.exe` único; retirar ZIP, scripts, `.exe` sueltos y, preferiblemente, el `.exe` de la aplicación por usuario; solo amd64.
8. **CI (decisión 8):** probar el instalador real con las diez comprobaciones y reutilizar `smoke.py`; aceptar el coste de construir también la aplicación; UAC, SmartScreen y equipos reales solo en la fase 5.
