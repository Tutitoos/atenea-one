# Aplicación de escritorio: paquetes por sistema operativo

La aplicación de escritorio ([ADR-019](../architecture/ADR-019-aplicacion-de-escritorio-wails.md)) se empaqueta con
`packaging/desktop/build.sh`, el mismo script en CI y en el release. **Los paquetes no llevan firma de desarrollador ni
están notarizados.** El `.dmg`, el `.rpm` y el `.exe` **no instalan el agente ni el CLI** (siguen en su `.deb`, `.pkg` y ZIP de
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
| Windows | `atenea-desktop-<versión>-windows-<arq>.exe` | Ejecutar; `/S` para instalación silenciosa | Instalación **por usuario** (`%LOCALAPPDATA%\Programs\Atenea One`), sin UAC; acceso directo en el menú Inicio y desinstalador. |

La verificación de un release (firma Ed25519, SHA-256, SBOM) es la de [release.md](release.md): los paquetes entran en el
mismo manifiesto, cada uno con el inventario Go de su sistema (`go-desktop-<Linux|macOS|Windows>.cdx.json`).

## Primera ejecución

La primera vez que se abre la aplicación (el fichero `first-run.json` de su carpeta de configuración marca que ya ocurrió) se
muestra la ventana con el asistente de **Primeros pasos** ([ADR-021](../architecture/ADR-021-asistente-de-primera-ejecucion.md)):

1. **Sistema y cliente.** Si falta el cliente (`atenea`), el asistente enseña la orden de instalación para copiar
   (`sudo apt install ./atenea-cli-*-linux-*.deb ./atenea-desktop-*-linux-*.deb` o `sudo installer -pkg atenea-one-*-macos-*.pkg -target /`) y «Comprobar de
   nuevo». **No lo instala ni lo descarga, y no abre enlaces.** En Windows solo **ve el estado** (ver «Windows: solo ver el estado»).
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
  está, lo dice y se detiene en lugar de descargar un binario sin verificar. Windows 11 y las versiones de Windows 10 al día
  lo traen con Microsoft Edge.
- **Linux, Ubuntu 24.04:** AppArmor restringe los espacios de nombres de usuario sin privilegios y el sandbox de WebKitGTK
  (`bwrap`) puede impedir que la ventana arranque. En los spikes de ADR-019 la aplicación se cerró al arrancar hasta poner
  `kernel.apparmor_restrict_unprivileged_userns=0`. **No se ha comprobado en un escritorio real** ni se aplica ningún
  rodeo en los paquetes: desactivar el sandbox de WebKit o relajar ese ajuste es una decisión de seguridad del usuario.
- La aplicación guarda sus preferencias en la carpeta de configuración del usuario (`one.atenea.desktop`); desinstalar el paquete
  **no** las borra.

### Windows: solo ver el estado

En Windows la aplicación **no une ni gestiona redes** (unirse, conectar, desconectar y salir no se ofrecen; el CLI de Windows aún
no admite `atenea network`, ADR-017). Lo que sí hace, sin privilegios: leer la tubería de estado del agente
(`\\.\pipe\atenea-status`, legible por el grupo Users) y mostrar **una única red**, la instalada a mano con `install.ps1`
(«Red de este equipo»). Si el servicio `AteneaAgent` no responde, el asistente pide instalarlo y arrancarlo siguiendo la
[guía de Windows](instalacion-windows.md) (la aplicación no lo instala). También lanza `ping.exe` a los pares (el resultado
se calcula con los envíos pedidos, sin depender del idioma de Windows) y abre el panel con `rundll32 url.dll,FileProtocolHandler`.
Siguen sin soporte en Windows las notificaciones del sistema y el inicio con la sesión.

**No probado en un Windows real:** solo compila (`GOOS=windows`) y pasa pruebas con salidas de muestra. La lectura real de la
tubería, el ping y abrir el panel están pendientes de comprobar en un equipo Windows. La app no verifica que el propietario de
la tubería sea Administradores (lo hace el CLI Rust; exigiría FFI, vetado en la app).

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
