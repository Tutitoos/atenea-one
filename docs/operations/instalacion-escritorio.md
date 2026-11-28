# Aplicación de escritorio: paquetes por sistema operativo

La aplicación de escritorio ([ADR-019](../architecture/ADR-019-aplicacion-de-escritorio-wails.md)) se empaqueta con
`packaging/desktop/build.sh`, el mismo script en CI y en el release. **Los paquetes no llevan firma de desarrollador ni
están notarizados.** El `.dmg`, el `.rpm` y el `.exe` **no instalan el agente ni el CLI** (siguen en su `.deb`, su `.pkg` y el `.exe` único `atenea-one-*` de
[Linux](instalacion-linux.md), [macOS](instalacion-macos.md) y [Windows](instalacion-windows.md)); en macOS el `.pkg` único
`atenea-one-*` trae los dos y el `.deb` de escritorio **exige** el `.deb` del cliente de su misma versión (ADR-022). Sin
cliente, la aplicación lo dice y no ofrece unirse.

## Paquetes

| Sistema | Paquete | Cómo se instala | Qué hace |
| --- | --- | --- | --- |
| Linux (Debian 13, Ubuntu 24.04 o posterior) | `atenea-desktop-<versión>-linux-<arq>.deb` | `sudo apt install ./atenea-cli-*-linux-*.deb ./atenea-desktop-*-linux-*.deb` | `/usr/bin/atenea-desktop`, entrada de menú e icono. Depende de `libgtk-4-1`, `libwebkitgtk-6.0-4` y de `atenea (= <misma versión>)` (el cliente, ADR-022): `apt` se niega a instalarlo sin el cliente o con otra versión. |
| Linux (Fedora y derivadas) | `atenea-desktop-<versión>-linux-<arq>.rpm` | `sudo dnf install ./atenea-desktop-*-linux-*.rpm` | Lo mismo; depende de `gtk4` y `webkitgtk6.0`. |
| macOS 11 o posterior | `atenea-one-<versión>-macos-<arq>.pkg` (**recomendado**: cliente y aplicación) | `sudo installer -pkg atenea-one-*-macos-*.pkg -target /` o doble clic | Instala el cliente (`/Library/Atenea/bin`) **y** `Atenea One.app` en `/Applications` como propiedad de root (ADR-022), con una sola autorización. Vive en la barra de menús (sin icono en el Dock). |
| macOS (solo la aplicación) | `atenea-desktop-<versión>-macos-<arq>.dmg` | Abrir el DMG y arrastrar «Atenea One» a Aplicaciones | `Atenea One.app` con firma **ad hoc**; el cliente se instala aparte. |
| Windows 10/11 x64 | `atenea-one-<versión>-windows-amd64.exe` (cliente, agente y aplicación) | Ejecutar con doble clic (UAC) o `/S` desde una consola elevada | Instalación **por equipo** (`%ProgramFiles%\Atenea`), acceso directo «Atenea One» en el menú Inicio y desinstalador; **sin firma de código** (ADR-023). Ya no hay instalador por usuario de la aplicación sola. El runtime WebView2 de Microsoft debe estar presente: si falta, el instalador avisa y la ventana no abre hasta instalarlo. |

La verificación de un release (firma Ed25519, SHA-256, SBOM) es la de [release.md](release.md): los paquetes entran en el
mismo manifiesto, cada uno con el inventario Go de su sistema (`go-desktop-<Linux|macOS|Windows>.cdx.json`).

## Primera ejecución

La primera vez que se abre la aplicación (el fichero `first-run.json` de su carpeta de configuración marca que ya ocurrió) se
muestra la ventana con el asistente de **Primeros pasos** ([ADR-021](../architecture/ADR-021-asistente-de-primera-ejecucion.md)):

1. **Sistema y cliente.** Si falta el cliente (`atenea`), el asistente enseña la orden de instalación para copiar
   (`sudo apt install ./atenea-cli-*-linux-*.deb ./atenea-desktop-*-linux-*.deb` o `sudo installer -pkg atenea-one-*-macos-*.pkg -target /`) y «Comprobar de
   nuevo». **No lo instala ni lo descarga, y no abre enlaces.** En Windows une y gestiona con UAC (ver «Windows: unir y gestionar con UAC»).
2. **Instalación heredada**, si la hay: ofrece importarla antes de unirse a otra red.
3. **Unirse:** pegar el código, revisar a qué red te unes (nombre, servidor, huella de la raíz, conectividad) y confirmar; el
   sistema pide la autorización. Es la misma pantalla de «Redes».
4. **Aprobación:** con un código de solicitud, enseña la huella del dispositivo para que el administrador la compare por otro
   canal y se actualiza sola hasta que lo aprueba.
5. **Listo:** opción de abrir la aplicación al iniciar sesión (apagada por defecto) y dónde queda el icono.

Se puede volver a abrir desde «Sin redes» o en `/welcome`; no guarda nada: el paso se deduce del estado del sistema.

## Avisos al primer arranque

- **macOS:** al no estar notarizada, Gatekeeper pide confirmación («Abrir igualmente» en Ajustes del Sistema → Privacidad y
  seguridad). La firma ad hoc solo evita que el bundle se modifique sin que `codesign --verify` lo note.
- **Windows:** SmartScreen avisará por la falta de firma Authenticode. El instalador **no incluye WebView2**: si el runtime no
  está, lo dice y sigue (el cliente y los servicios no lo necesitan; la ventana no abre hasta instalarlo) en lugar de descargar
  un binario sin verificar. Windows 11 y las versiones de Windows 10 al día lo traen con Microsoft Edge.
- **Linux, Ubuntu 24.04:** AppArmor restringe los espacios de nombres de usuario sin privilegios y el sandbox de WebKitGTK
  (`bwrap`) puede impedir que la ventana arranque. En los spikes de ADR-019 la aplicación se cerró al arrancar hasta poner
  `kernel.apparmor_restrict_unprivileged_userns=0`. **No se ha comprobado en un escritorio real** ni se aplica ningún
  rodeo en los paquetes: desactivar el sandbox de WebKit o relajar ese ajuste es una decisión de seguridad del usuario.
- La aplicación guarda sus preferencias en la carpeta de configuración del usuario (`one.atenea.desktop`); desinstalar el paquete
  **no** las borra.
- **Versión instalada** (ADR-025, fase 2): `atenea-desktop --version` (o `-V`, como único argumento) imprime `atenea-desktop <versión> (<commit>)` y sale **sin abrir ventana, bandeja ni servicio** y sin necesitar pantalla. Es la misma forma que `atenea --version`; el agente del equipo la ejecutará desde una ruta fija para informar qué versión de la aplicación hay instalada (fases siguientes). La versión y el commit se incrustan al compilar con `packaging/desktop/build-binary.sh`; un binario compilado a mano sin ellos imprime `atenea-desktop 0.0.0-dev (unknown)`. En Windows el binario es del subsistema gráfico: `packaging/desktop/check-version.sh` comprueba en la CI que aun así escribe en una tubería.

### Windows: unir y gestionar con UAC

En Windows la aplicación **une y gestiona redes** como en macOS y Linux (ADR-017 fase 3). Sin privilegios lee el estado: la lista sale de `atenea network list --json` y el estado de cada red, de su tubería `\\.\pipe\atenea-<id8>-status` (legible por el grupo Users). Unirse, conectar, desconectar y salir se ejecutan con el **diálogo de UAC**: la aplicación lanza `powershell.exe`, que arranca el CLI con `[Diagnostics.Process]::Start` y `Verb = runas` (sin FFI) y ejecuta una sola llamada (`atenea network join <código> --yes --up`, `up`, `down` o `leave`; no hay shell que encadene órdenes). La ventana de UAC la pone Windows y nombra el programa, no el texto de Atenea: la confirmación previa (red, servidor, huella) es la de la aplicación.

- **El CLI que se eleva es siempre `%ProgramFiles%\Atenea\atenea.exe`**, la carpeta que protege el instalador. La ruta sale de la carpeta conocida del sistema, no de una variable de entorno; `ATENEA_CLI` no sirve para una operación elevada, y un enlace se rechaza. **No lee la DACL del fichero** (exigiría FFI de seguridad en la aplicación): confía en la que fija el instalador, que comprueba la CI; si alguien relaja esa carpeta, la elevación lo hereda.
- **El resultado viaja por un fichero, no por la salida.** La salida de una orden elevada con UAC no se puede capturar y el proceso elevado nunca escribe en una ruta que controle el usuario. La aplicación elige un identificador de 128 bits aleatorios (`--result <nonce>`) y el CLI elevado crea `%ProgramData%\Atenea\ui-results\<nonce>\result.json` (escritura solo para SYSTEM y Administradores, lectura para `Users`; hasta 4 KiB, sin caracteres de control) con `{"ok":…,"message":…}`. Solo aporta el mensaje de un fallo: el estado se vuelve a leer con `network list`, que es la fuente de verdad. El CLI retira los resultados de hace más de un día.
- **Cancelar el diálogo no cambia nada:** `powershell` sale con 126 (error 1223 de Windows) y la aplicación muestra «Cancelado».
- **No hay migración de la instalación heredada** (`AteneaAgent`, ADR-017 §10): la aplicación no la ofrece; hay que desinstalarla, reinstalar y unirse con un código. Tampoco se pueden publicar servicios de este equipo todavía (ADR-024 en Windows).
- También lanza `ping.exe` a los pares (el resultado se calcula con los envíos pedidos, sin depender del idioma de Windows) y abre el panel con `rundll32 url.dll,FileProtocolHandler`.
- «Abrir al iniciar sesión» (Diagnóstico y último paso del asistente) funciona con un valor bajo la clave `Run` del propio usuario (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, valor `Atenea One` con la ruta del ejecutable entre comillas y sin argumentos): sin privilegios y sin servicio residente, y la aplicación arranca oculta, en la bandeja. Desactivarlo borra el valor. Siguen sin soporte en Windows las notificaciones del sistema.

**Lo que prueba la CI** (`tests/windows/network-checks.ps1`, runner elevado desechable): el canal `result.json` del CLI (éxito, fallo con su mensaje, nonce inválido, DACL), y la ruta REAL de la aplicación (`TestWindowsElevationAcceptance`: comprobación del CLI de Archivos de programa, `powershell` con `Process.Start` y `Verb = runas`, `up`, `down` y `leave` de una red unida contra un control plane falso, y el mensaje de un fallo). En el runner UAC no pide confirmación (la cuenta ya es administrador), así que **el diálogo y su cancelación no se ejercitan**; las pruebas unitarias cubren la orden que se construye (sin comillas ni metacaracteres que escapen de la cita de PowerShell) y el canal de resultado con un ejecutor simulado. La cancelación se comprobó a mano en un Windows real (ver `docs/operations/v0.1.39.md`).

**Sin comprobar en un Windows real:** una cuenta estándar que lea la lista y las tuberías (solo se comprueban sus DACL), unirse contra un control plane real, el ping a un par, abrir el panel con `rundll32`, el resto de pantallas y otras versiones o idiomas de Windows. El diálogo de UAC y su cancelación se comprobaron a mano en un solo equipo (0.1.39). La aplicación no verifica que el propietario de la tubería sea Administradores (lo hace el CLI Rust; exigiría FFI): un usuario local que creara esa tubería antes que el servicio podría mostrar un estado falso y que «Abrir el panel» abriera una URL suya (siempre `https`, sin credenciales).

## Servicios de los dispositivos

Fase 5 de [ADR-024](../architecture/ADR-024-servicios-expuestos-por-dispositivo.md). Implementada con pruebas unitarias; **no se ha
visto la interfaz pintada ni se ha ejecutado el diálogo de privilegios real**.

- **Qué se ve.** El detalle de un par tiene una sección **Servicios** con los puertos TCP que ese par publica en la red elegida
  (puerto, etiqueta, esquema), con **Abrir** y **Copiar dirección**; la lista de dispositivos indica cuántos publica cada uno.
  Un par solo aparece con servicios si los publica y si hay una política hacia él. **Publicar un servicio no lo hace alcanzable**:
  lo deciden las políticas y el cortafuegos de cada equipo.
- **Abrir.** La interfaz nunca envía una URL: manda la red, el id del par y el puerto (`OpenService`). El backend comprueba que ese
  puerto figura en la lista publicada por ese par en el estado de esa red y construye `http(s)://<IPv4 del par>:<puerto>/` (IPv6
  entre corchetes si no hay IPv4) con el esquema publicado. Se abre con el navegador del sistema (`open`, `xdg-open` o `rundll32`).
  El estado local del agente no expone los prefijos de la red: la dirección sale de las del propio par, que el agente solo acepta
  dentro de ellos, y se descartan bucle local, multicast y enlace local.
- **Publicar los propios.** En **Diagnóstico → Servicios**, el interruptor «Publicar mis servicios en esta red» (por red, apagado por
  defecto) ejecuta `atenea services publish on|off --network <id>` con el diálogo de administrador del sistema (`osascript` en
  macOS, `pkexec` en Linux); cancelar no cambia nada. Debajo se listan, en solo lectura, los servicios que detecta el equipo.
- **Límites.** En Windows no se ofrece el interruptor (como unirse, ADR-017): la publicación se configura con `[services] publish`
  en `agent.toml`. Solo TCP; las etiquetas y el esquema se fijan en `agent.toml`, no desde la aplicación.

## Construir en local

```sh
pnpm install --frozen-lockfile && pnpm --filter @atenea/desktop run build   # interfaz embebida
export ATENEA_VERSION=0.1.12 ATENEA_BUILD_COMMIT=$(git rev-parse HEAD)
packaging/desktop/build.sh dist          # paquete del sistema donde se ejecuta
packaging/desktop/check-package.sh dist  # SOLO en un equipo de pruebas: instala, arranca y desinstala
```

Linux necesita `wails3` en el PATH con la versión que fija `apps/desktop/go.mod`
(`go install github.com/wailsapp/wails/v3/cmd/wails3@<versión>`) y las bibliotecas de desarrollo de GTK 4 y WebKitGTK 6.0;
Windows necesita `makensis` (NSIS) y `pwsh`. El `.rpm` guarda en su cabecera el **nombre del equipo que lo construye**
(`BuildHost`, lo escribe nfpm): los paquetes que se publican salen de runners de CI, no de un equipo personal.

## Evidencia y límites

CI (`desktop`, Linux, macOS y Windows) construye el paquete, lo instala, arranca la aplicación instalada con la prueba de humo
y la desinstala: la interfaz embebida carga bajo la CSP y el filtro y llama a un método vinculado. Eso **no** prueba la bandeja
visible ni sus clics, que el aviso se vea, los diálogos de elevación ni el `.rpm` en una distribución Red Hat (solo se
construye y se inspecciona su cabecera). **Sin AppImage:** `wails3 generate appimage` descarga `linuxdeploy` de una etiqueta
móvil de GitHub sin comprobar ningún hash y el sandbox de Ubuntu 24.04 la afecta igual; queda pendiente de decidir cómo
fijar esa cadena. Sin firma de desarrollador, sin
notarización y sin actualizador (ADR-019, decisión 9).

**Ruta del CLI en las operaciones con privilegios.** Unirse, conectar, desconectar, salir, importar la instalación heredada y
publicar servicios ejecutan el CLI con `osascript` o `pkexec`. Antes de pedir la autorización, la aplicación exige que esa
ruta (`/Library/Atenea/bin/atenea` o `/usr/bin/atenea`) no pueda sustituirla un usuario sin privilegios: un fichero regular
de root, sin enlaces simbólicos y sin escritura de grupo ni de otros, y lo mismo para cada directorio hasta `/` (la regla de
`network_service::trusted` del CLI). Si no se cumple, la operación falla sin abrir el diálogo. `ATENEA_CLI` (desarrollo y
pruebas) sigue sustituyendo la ruta en las lecturas (`list`, `inspect`, `legacy` y el estado del cliente), pero en una
operación con privilegios solo vale si cumple esa misma regla: un CLI simulado debe instalarse como root bajo directorios de
root, no en `/tmp`, en `$HOME` ni bajo un enlace simbólico (en macOS `/tmp` y `/var` lo son). En Windows no hay operación con
privilegios (ADR-017 sin implementar).

**Límite conocido: el diálogo de autorización de macOS no enseña el texto de la aplicación.** La aplicación pasa a
`osascript` un texto propio («Atenea One quiere desconectar este equipo de una red.», «…unir este equipo a la red «nombre».»,
etc.), pero en macOS 27 (comprobado el 2026-11-04, con el sistema en español) el diálogo solo dice «¿Permitir el acceso de
administración de un script iniciado por Atenea One?», el aviso genérico de Apple y el campo de contraseña: **no aparece qué
operación se autoriza ni sobre qué red**. La orden que se ejecuta es la correcta (el registro de un CLI simulado mostró
`network join <código> --yes --status-gid <gid>` y `network up <id>` como root), pero quien autoriza no puede distinguir
«unir» de «salir de la red y borrar sus claves» solo por el diálogo. La decisión 8 de
[ADR-021](../architecture/ADR-021-asistente-de-primera-ejecucion.md) (nombrar la red) **no surte efecto en este sistema**. La
aplicación sí enseña la operación y la red **antes** de abrir el diálogo (confirmación de «Unirme a esta red», «Importar» y
«Salir de la red…»), así que conviene pulsar esos botones solo cuando se está viendo esa pantalla. Sin comprobar en otras
versiones de macOS ni con `pkexec` en Linux.
