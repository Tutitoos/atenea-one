<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-017: Redes por instancia en Windows

- **Estado:** Propuesto (2026-10-29). Decisiones del responsable pendientes (§ «Decisiones que necesito del responsable»). No hay código de esta fase.
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

## Decisiones que necesito del responsable

Mis recomendaciones entre paréntesis; aceptar todas basta con decirlo.

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

## Límites de esta evidencia

Todo lo anterior se leyó en el código y la documentación; **no se ha ejecutado nada en Windows** para este ADR. En particular quedan sin comprobar: que Wintun admita varios adaptadores simultáneos con GUID propios, que la API de `windows-service` 0.8 baste, el comportamiento de UAC y de `Start-Process -Verb RunAs`, el arranque automático retrasado tras un reinicio real, ARM64 físico, suspensión/Wi-Fi y la matriz entre sistemas. La CI es solo manual y su smoke de Windows cubre una instancia.

Observaciones de documentación desfasada detectadas al investigar (sin corregir aquí): `docs/security/threat-model.md` T28 habla de «DNS global» residual cuando la implementación usa NRPT por dominio, y `ADR-011` aún dice que la elevación de macOS está pendiente mientras `ADR-013` registra después una prueba con diálogo real.
