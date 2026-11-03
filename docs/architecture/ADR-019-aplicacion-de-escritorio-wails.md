# ADR-019: Aplicación de escritorio con Wails v3 (sustituye a Tauri 2)

- **Estado:** **Decidido por el responsable el 2026-11-03** («cambiemos Tauri por Wails»). **Sin implementar.** Los spikes S1–S6 se ejecutaron el mismo día en la rama `spike/wails` y se cumplen (ver «Resultado de los spikes»). El código Rust de `apps/desktop` no se borra hasta que haya paridad (Plan, fase 3).
- **Sustituye a:** [ADR-011](ADR-011-aplicacion-de-escritorio.md) **§1 (Tauri 2), §4 (workspace de Cargo) y §5 (licencia MPL-2.0 acotada)**. Lo demás de ADR-011 (§2 lectura sin privilegios y mínima superficie, §3 sin privilegios) **sigue vigente como lista de requisitos**: ya no describe una tecnología, describe lo que la nueva aplicación debe cumplir con las mismas pruebas.
- **Relacionados:** ADR-007 (privilegios), ADR-008 (clientes macOS/Windows), ADR-013 (redes múltiples), ADR-014 (servidores), ADR-017 (Windows por instancia), ADR-018 (unión solo con el código).

## Contexto

ADR-011 eligió Tauri 2 frente a Wails v3 el 2026-10-27. Descartó Wails por obligar a reescribir el cliente IPC en Go y a duplicar la cadena de suministro. El responsable decide ahora cambiar a Wails para tener una sola cadena de empaquetado de la aplicación (instalador NSIS, `.app`/DMG con firma y notarización, AppImage/DEB/RPM) y la posibilidad de un asistente de primera ejecución. Aceptamos el coste que ADR-011 señaló.

Lo que dice la documentación oficial de Wails (leída el 2026-11-03; **no se ha construido nada con ella**):

- Wails v3.0.0 es la versión estable para escritorio en Windows, macOS y Linux (existe también una v3.1.0). Móvil sigue siendo experimental y queda fuera.
- IPC **en memoria**, «sin puertos abiertos ni servidor localhost», con bindings Go→JS generados y tipados. Cumple el requisito de ADR-011 de no tener servidor local.
- Incluye bandeja del sistema, menús, notificaciones, inicio automático, instancia única, diálogos y un *updater* opcional.
- Empaquetado: **Windows** NSIS (`.exe`) y MSIX (**no genera `.msi`**); **macOS** `.app`, DMG, universal binary, firma y notarización; **Linux** AppImage, DEB, RPM y Arch (nfpm), con base **GTK4/WebKitGTK 6.0** (la ruta GTK3 es opcional y estaba prevista para retirarse en v3.1).
- Su política de versiones recomienda **fijar versión**.

## Decisión

1. **`apps/desktop` pasa a ser una aplicación Wails v3 (Go).** El frontend (`apps/desktop/app`, React Router + Tailwind + TypeScript) se conserva: las pantallas no se reescriben; solo se sustituye `@tauri-apps/api` por los bindings generados de Wails.
2. **Un único servicio vinculado** con exactamente las operaciones que hoy son comandos Tauri: `status`, `network_list`, `network_inspect`, `network_legacy`, `network_apply`, `device_ping`, `open_panel`, `notifications_get/set` y `autostart_get/set`. **Todo método exportado de un servicio vinculado es invocable desde el JavaScript de la página**, así que el servicio expone solo esos once y la lógica vive en paquetes internos sin vincular. Añadir un método al servicio es un cambio de seguridad y requiere revisión.
3. **Se mantienen, como criterios de aceptación con pruebas equivalentes, todas las reglas de ADR-011 §2 y §3:** sin privilegios; lista cerrada de operaciones; validación estricta en el backend (UUID canónico, código `ATENEA1-…`, nombre acotado, ping solo a IP de pares de esa red, URL del panel solo HTTPS) **antes** de construir cualquier orden; ruta del CLI fija; **la autoridad es el sistema operativo** (`osascript … with administrator privileges` en macOS, `pkexec` en Linux, elevación de Windows según ADR-017) en cada operación, sin servicio residente ni credencial guardada; respuesta del agente validada y con tope de 4 MiB; la salud se calcula **una sola vez** en el backend; confirmación en dos pasos para unirse; el webview no tiene salida de red.
4. **Módulo Go propio** (`go.mod`, `go.sum`, `vendor/`) dentro de `apps/desktop`, fuera del grafo del workspace de Cargo. **Se usa Wails v3 (línea 3.x estable), no v2:** el módulo es `github.com/wailsapp/wails/v3` y su CLI es `wails3`. La versión de Wails y la de `wails3` se **fijan exactamente** y solo se cambian con revisión. **Propuesta, pendiente de confirmar por el responsable: v3.0.0** (los spikes dan el mismo resultado con v3.0.0 y v3.1.0; v3.0.0 es la línea cuyo soporte GTK3 documenta Wails).
5. **Cadena de suministro (sustituye a `cargo-deny` para este módulo):** `go mod verify`, dependencias *vendored*, `govulncheck`, comprobación de licencias con lista permitida equivalente a `deny.toml`, y un SBOM CycloneDX del módulo Go que entra en el manifiesto firmado y en `scripts/release/verify.sh`. Desaparecen las cinco excepciones MPL-2.0 de Tauri; hay que revisar las licencias de las dependencias de Wails antes de aceptar la lista.
6. **Construcción:** sin instalar nada en el Mac del operador (Go y `wails3` dentro de contenedores con módulos *vendored*, sin red) y en los runners de CI de cada sistema operativo, que es donde se enlaza con las bibliotecas nativas (CGO).
7. **Distribución de la aplicación con Wails:** `.app` y DMG, instalador NSIS y AppImage/DEB/RPM. El **agente y el CLI** (servicios privilegiados) **no** salen de Wails: siguen en su `.deb`, `.pkg` y paquete de Windows propios (ADR-013, ADR-017); un instalador integrado que los junte con la aplicación es una decisión posterior y se anotará en un ADR propio. **`.msi`:** Wails no lo genera; si el responsable lo exige para el agente en Windows, se hace con WiX aparte.
8. **Asistente de primera ejecución dentro de la aplicación** (código de unión, comprobación de la huella, `join --up` elevado, inicio con la sesión, opciones avanzadas `--stun-host` y `--relay-ca`), no un instalador con pantallas propias. Su diseño de privilegios se decide en un ADR posterior.
9. **Sin *updater* de Wails** por ahora (como ADR-011): una actualización automática es un canal de ejecución de código y necesita su propio ADR y firma.
10. **Filtro de objetos del runtime en el servidor de recursos.** El runtime de Wails exporta módulos propios (`Application`, `Browser`, `Clipboard`, `Dialogs`, `Events`, `Screens`, `System`, `Window`) que la página puede usar sin que el servicio los exponga, y Wails no ofrece desactivarlos. La aplicación **debe** aplicar en su middleware del servidor de recursos la lista de permitidos demostrada en el spike S2c (solo los objetos `Call` y `CancelCall`; el resto, rechazado con 403, incluidas las subidas por trozos) junto con la CSP, y llevar pruebas que fallen si el filtro se debilita y si el servicio vinculado gana un método exportado. Sin ese filtro la superficie de la página es mayor que la de Tauri (cuyas capacidades por defecto no concedían ningún plugin).

## Condiciones: spikes obligatorios antes de borrar Tauri

| # | Spike | Qué debe demostrar |
| --- | --- | --- |
| S1 | CSP | Que se puede imponer la CSP de ADR-011 (`default-src 'none'; script-src 'self'; …`, sin `unsafe-inline` ni `eval`) en el servidor de recursos de Wails y que el webview no puede salir a la red. |
| S2 | Superficie del puente | Qué objetos globales inyecta el *runtime* en la página y que desde JS solo se pueden invocar los once métodos. |
| S3 | Bandeja | Icono por salud, menú que se reconstruye solo si cambia y acciones por red en macOS, Linux y Windows. |
| S4 | Backend | Cliente del socket de estado (Unix) con el tope de 4 MiB, ejecución del CLI con argumentos validados y la elevación por SO. |
| S5 | Empaquetado | `.app` + DMG, NSIS y AppImage/DEB/RPM reproducibles, sin red en el contenedor y con SBOM. |
| S6 | Licencias | La lista de licencias de las dependencias de Wails entra en la lista permitida sin excepciones que no podamos justificar. |

## Resultado de los spikes (2026-11-03)

Ejecutados en la rama `spike/wails` (programa mínimo en `spikes/wails/` y workflow `spike-wails.yml`; ejecución final `41680261150`, commit `063345b`) en runners de GitHub: Ubuntu 24.04 (GTK4/WebKitGTK 6.0), macOS, Windows y Ubuntu 22.04 con `-tags gtk3`, con Wails v3.0.0 y v3.1.0. **Los resultados son idénticos en las dos versiones y en todos los sistemas.** Es evidencia de CI con webviews reales, no de escritorios de usuarios reales.

| Spike | Veredicto |
| --- | --- |
| S1 CSP | Cumplido |
| S2 Puente | Cumplido **con mitigación obligatoria** (decisión 10) |
| S3 Bandeja | Cumplido a nivel de API; la visibilidad real no se comprobó |
| S4 Backend | Cumplido |
| S5 Empaquetado | Cumplido, con límites |
| S6 Licencias | Cumplido |

- **S1.** Con `default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'` (la de ADR-011 con `connect-src 'self'` en lugar de `ipc:`), puesta como cabecera por un middleware del servidor de recursos: las seis sondas (`fetch`, WebSocket, imagen y script remotos, script en línea y `eval`) quedan bloqueadas con su evento de violación de CSP, el origen propio funciona y el puente de Wails funciona bajo esa CSP sin violaciones propias. El origen es `wails://localhost/` en macOS y Linux y `http://wails.localhost/` en Windows (host virtual de WebView2). **Sin sockets de red:** con la aplicación viva (comprobada con `pgrep` o PID), ningún socket TCP o UDP del proceso en los tres sistemas.
- **S2.** Globales inyectados: `wails`, `_wails` y un global `w` suelto, más los puentes nativos del webview (`webkit.messageHandlers` o `chrome.webview`); no existe `window.__TAURI__`. **Todo método exportado de un servicio registrado es invocable por nombre desde la página** (un método `NotIntended` respondió); los no exportados y los de tipos no registrados no existen para ella. Una prueba compara por reflexión la lista de métodos con la permitida y falla si se añade uno, incluso por incrustación. **El runtime exporta además los módulos de la decisión 10 y funcionaron desde la página:** leer y escribir el portapapeles, cambiar el título, leer sistema operativo, arquitectura y versión, y las pantallas; `Browser.OpenURL` solo bloquea `javascript:`, `data:`, `file:` y `ftp:`. En el código de Wails v3.0.0 el despacho de esos objetos no tiene lista de permitidos. **S2c (mitigación):** un filtro en el middleware, que ve el `POST /wails/runtime` antes que el transporte, deja pasar solo `Call` (0) y `CancelCall` (10) y devuelve 403 al resto; con él los métodos del servicio siguen funcionando y lo anterior queda denegado en los tres sistemas y las dos versiones (más cinco pruebas unitarias del filtro). **Residual sin probar con el filtro:** los mensajes nativos crudos (arrastrar y redimensionar la ventana, «runtime listo» y emitir eventos simples, este último desactivado salvo `AllowSimpleEventEmit`), la navegación de la página y los eventos Go→JS.
- **S3.** La bandeja se crea (icono, etiqueta, descripción y menú) y se actualiza con la aplicación en marcha (otro icono y otro menú) sin error en macOS, Windows y Linux (GTK4 y GTK3; allí sin anfitrión de bandeja, solo xvfb y dbus). **No demuestra que sea visible**, ni los clics, ni el menú por red.
- **S4.** Port a Go de `networks.rs` y del cliente del socket de estado con **los mismos casos de prueba** (16: UUID, código, nombre, órdenes elevadas exactas, operaciones, orden de elevación por sistema, socket con tope de 4 MiB y plazo, ejecución del CLI y cancelación): pasan en Linux, macOS y Windows. La elevación (`osascript`, `pkexec`) solo se construyó y probó como argumentos: **no se ejecutó ningún diálogo**. El estado por tubería de Windows (ADR-017) no se cubre.
- **S5.** Con la plantilla oficial: Linux AppImage (78 MB, incluye WebKit), DEB (4,6 MB), RPM (4,8 MB) y paquete Arch; macOS `.app` (firma ad hoc) y DMG (5,5 MB); Windows instalador NSIS (6,3 MB, con el arranque de WebView2), **sin `.msi`**. El binario del spike pesa 13–15 MB. **Compilación reproducible:** dos compilaciones con `-trimpath` dan el mismo SHA-256 en los tres sistemas. `go mod vendor`, `go mod verify` y la compilación con `-mod=vendor` correctos; SBOM CycloneDX 1.6 con `cyclonedx-gomod`; `govulncheck` sin vulnerabilidades. **No comprobado:** compilación sin red en contenedor, firma y notarización, MSIX, ARM y binario universal.
- **S6.** v3.0.0 trae 8 módulos (MIT, ISC, BSD-2-Clause y BSD-3-Clause) y v3.1.0 trae 7 (añade Apache-2.0): todos dentro de la lista de `deny.toml`, sin MPL-2.0. La clasificación es heurística (palabras clave del fichero de licencia) y solo cubre lo que importa un programa mínimo.
- **Linux.** (a) La ruta GTK3/WebKit2GTK 4.1 compila y funciona en Ubuntu 22.04 con las dos versiones; la documentación decía que se retiraba en la 3.1, **todavía no**. (b) En **Ubuntu 24.04 la aplicación se cerró al arrancar** (`bwrap: setting up uid map: Permission denied`: AppArmor restringe los espacios de nombres de usuario del sandbox de WebKit) hasta poner `kernel.apparmor_restrict_unprivileged_userns=0` en el runner. En un escritorio real no se ha comprobado y afecta a la guía de instalación y al AppImage.
- **Avisos del enlazador en macOS:** versión mínima 11 frente al SDK 26; informativos, se arregla fijando `MACOSX_DEPLOYMENT_TARGET`.

## Consecuencias y riesgos

- **Reescritura:** unas 2 000 líneas de Rust (`main.rs`, `networks.rs`, `notify.rs`, `autostart.rs`, `ping.rs`, `panel.rs`, `tray_actions.rs`) y sus pruebas pasan a Go. Se portan con las **mismas tablas de casos** para demostrar paridad; el frontend conserva sus 73 pruebas. Se borra el Rust cuando haya paridad (sin dejar dos aplicaciones).
- **Código no gestionado:** Wails usa CGO (C y Objective-C para GTK, WebKit y Cocoa). `unsafe_code = forbid` no existe en Go; el riesgo es de la misma naturaleza que el de las dependencias nativas de Tauri, pero no desaparece.
- **Linux:** la base por defecto es GTK4/WebKitGTK 6.0 (Debian 13 la tiene; Debian 12 y Ubuntu 22.04 no). La ruta GTK3 funciona con v3.0.0 y v3.1.0, pero Wails la da por en retirada. Riesgo de arranque en Ubuntu 24.04 por el sandbox de WebKit (ver «Resultado de los spikes»).
- **Windows sigue aparcado** (ADR-017 sin implementar): el instalador NSIS de la aplicación no arregla que el agente de Windows no esté terminado.
- **Firma:** macOS necesita Developer ID y notarización, y Windows un certificado Authenticode. Siguen siendo del responsable; hasta entonces los instaladores van sin firmar.
- **Dependencia más joven:** Wails v3 es estable, pero reciente. Se fija la versión y se vigila su *changelog* en cada actualización.

## Alternativas descartadas

- **Mantener Tauri 2:** descartada por el responsable.
- **Un instalador Wails aparte (`atenea-setup`) que llame al CLI:** duplicaría el diálogo de unión de la aplicación y añadiría otro binario que firmar.
- **Electron:** binarios y memoria muy mayores y un navegador empaquetado; no cumple el espíritu de ADR-011.

## Plan por fases

1. **ADR-019 aceptado** y los spikes S1–S6 (sin tocar `apps/desktop`): **hechos el 2026-11-03** (ver «Resultado de los spikes»).
2. **Esqueleto Wails** con el servicio de once métodos, el frontend conectado y la bandeja; pruebas de paridad del backend, el **filtro de objetos del runtime** (decisión 10) con su prueba y la **prueba de superficie** del servicio; CSP aplicada por el middleware del servidor de recursos.
3. **Paridad completa** (notificaciones, inicio con la sesión, ping, abrir panel, operaciones elevadas) y **retirada de Tauri** (código Rust, `Cargo.lock`, `deny.toml`, jobs de CI).
4. **Empaquetado** por sistema operativo y entrada en el manifiesto firmado.
5. **Asistente de primera ejecución** (ADR propio de privilegios) y guías «Empezar» por sistema.
6. **Firma y notarización** cuando existan los certificados; **Windows** cuando se desaparque.

Cada publicación sigue siendo una entrega separada con autorización expresa.

## Límites

Los spikes son evidencia de CI en runners de GitHub, con un programa mínimo y la plantilla oficial de Wails; **no** hay una aplicación implementada. Sin comprobar: bandeja visible y con clics en escritorios reales, el arranque en un Ubuntu 24.04 de escritorio, diálogos de elevación reales, el estado por tubería de Windows, compilación sin red en contenedor, firma y notarización, memoria y rendimiento en uso prolongado, y los residuales de S2 (mensajes nativos crudos, navegación y eventos Go→JS con el filtro activo).
