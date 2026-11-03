# Aplicación de escritorio: paquetes por sistema operativo

La aplicación de escritorio ([ADR-019](../architecture/ADR-019-aplicacion-de-escritorio-wails.md)) se empaqueta con
`packaging/desktop/build.sh`, el mismo script en CI y en el release. **Los paquetes no llevan firma de desarrollador ni
están notarizados**, y no instalan el agente ni el CLI (siguen en su `.deb`, `.pkg` y ZIP de [Linux](instalacion-linux.md),
[macOS](instalacion-macos.md) y [Windows](instalacion-windows.md)): sin ellos la aplicación solo muestra «Sin redes».

| Sistema | Paquete | Cómo se instala | Qué hace |
| --- | --- | --- | --- |
| Linux (Debian 13, Ubuntu 24.04 o posterior) | `atenea-desktop_<versión>_<arq>.deb` | `sudo apt install ./atenea-desktop_*.deb` | `/usr/bin/atenea-desktop`, entrada de menú e icono. Depende de `libgtk-4-1` y `libwebkitgtk-6.0-4`. |
| Linux (Fedora y derivadas) | `atenea-desktop-<versión>.<arq>.rpm` | `sudo dnf install ./atenea-desktop-*.rpm` | Lo mismo; depende de `gtk4` y `webkitgtk6.0`. |
| macOS 11 o posterior | `atenea-desktop-<versión>-macOS-<arq>.dmg` | Abrir el DMG y arrastrar «Atenea One» a Aplicaciones | `Atenea One.app` con firma **ad hoc**. Vive en la barra de menús (sin icono en el Dock). |
| Windows | `atenea-desktop-<versión>-Windows-<arq>.exe` | Ejecutar; `/S` para instalación silenciosa | Instalación **por usuario** (`%LOCALAPPDATA%\Programs\Atenea One`), sin UAC; acceso directo en el menú Inicio y desinstalador. |

La verificación de un release (firma Ed25519, SHA-256, SBOM) es la de [release.md](release.md): los paquetes entran en el
mismo manifiesto, cada uno con el inventario Go de su sistema (`go-desktop-<Linux|macOS|Windows>.cdx.json`).

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
