# ADR-019: Aplicación de escritorio con Wails v3 (sustituye a Tauri 2)

- **Estado:** **Decidido por el responsable el 2026-11-03** («cambiemos Tauri por Wails»). **Sin implementar.** La implementación queda condicionada a los spikes de «Condiciones» y el código Rust de `apps/desktop` no se borra hasta cumplirlos.
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
4. **Módulo Go propio** (`go.mod`, `go.sum`, `vendor/`) dentro de `apps/desktop`, fuera del grafo del workspace de Cargo. La versión de Wails y de su CLI `wails3` se **fija exactamente** y solo se cambia con revisión.
5. **Cadena de suministro (sustituye a `cargo-deny` para este módulo):** `go mod verify`, dependencias *vendored*, `govulncheck`, comprobación de licencias con lista permitida equivalente a `deny.toml`, y un SBOM CycloneDX del módulo Go que entra en el manifiesto firmado y en `scripts/release/verify.sh`. Desaparecen las cinco excepciones MPL-2.0 de Tauri; hay que revisar las licencias de las dependencias de Wails antes de aceptar la lista.
6. **Construcción:** sin instalar nada en el Mac del operador (Go y `wails3` dentro de contenedores con módulos *vendored*, sin red) y en los runners de CI de cada sistema operativo, que es donde se enlaza con las bibliotecas nativas (CGO).
7. **Distribución de la aplicación con Wails:** `.app` y DMG, instalador NSIS y AppImage/DEB/RPM. El **agente y el CLI** (servicios privilegiados) **no** salen de Wails: siguen en su `.deb`, `.pkg` y paquete de Windows propios (ADR-013, ADR-017); un instalador integrado que los junte con la aplicación es una decisión posterior y se anotará en un ADR propio. **`.msi`:** Wails no lo genera; si el responsable lo exige para el agente en Windows, se hace con WiX aparte.
8. **Asistente de primera ejecución dentro de la aplicación** (código de unión, comprobación de la huella, `join --up` elevado, inicio con la sesión, opciones avanzadas `--stun-host` y `--relay-ca`), no un instalador con pantallas propias. Su diseño de privilegios se decide en un ADR posterior.
9. **Sin *updater* de Wails** por ahora (como ADR-011): una actualización automática es un canal de ejecución de código y necesita su propio ADR y firma.

## Condiciones: spikes obligatorios antes de borrar Tauri

| # | Spike | Qué debe demostrar |
| --- | --- | --- |
| S1 | CSP | Que se puede imponer la CSP de ADR-011 (`default-src 'none'; script-src 'self'; …`, sin `unsafe-inline` ni `eval`) en el servidor de recursos de Wails y que el webview no puede salir a la red. |
| S2 | Superficie del puente | Qué objetos globales inyecta el *runtime* en la página y que desde JS solo se pueden invocar los once métodos. |
| S3 | Bandeja | Icono por salud, menú que se reconstruye solo si cambia y acciones por red en macOS, Linux y Windows. |
| S4 | Backend | Cliente del socket de estado (Unix) con el tope de 4 MiB, ejecución del CLI con argumentos validados y la elevación por SO. |
| S5 | Empaquetado | `.app` + DMG, NSIS y AppImage/DEB/RPM reproducibles, sin red en el contenedor y con SBOM. |
| S6 | Licencias | La lista de licencias de las dependencias de Wails entra en la lista permitida sin excepciones que no podamos justificar. |

## Consecuencias y riesgos

- **Reescritura:** unas 2 000 líneas de Rust (`main.rs`, `networks.rs`, `notify.rs`, `autostart.rs`, `ping.rs`, `panel.rs`, `tray_actions.rs`) y sus pruebas pasan a Go. Se portan con las **mismas tablas de casos** para demostrar paridad; el frontend conserva sus 73 pruebas. Se borra el Rust cuando haya paridad (sin dejar dos aplicaciones).
- **Código no gestionado:** Wails usa CGO (C y Objective-C para GTK, WebKit y Cocoa). `unsafe_code = forbid` no existe en Go; el riesgo es de la misma naturaleza que el de las dependencias nativas de Tauri, pero no desaparece.
- **Linux:** la base pasa a GTK4/WebKitGTK 6.0. Debian 13 la tiene; Debian 12 y Ubuntu 22.04 no (la ruta GTK3 era opcional y está en retirada).
- **Windows sigue aparcado** (ADR-017 sin implementar): el instalador NSIS de la aplicación no arregla que el agente de Windows no esté terminado.
- **Firma:** macOS necesita Developer ID y notarización, y Windows un certificado Authenticode. Siguen siendo del responsable; hasta entonces los instaladores van sin firmar.
- **Dependencia más joven:** Wails v3 es estable, pero reciente. Se fija la versión y se vigila su *changelog* en cada actualización.

## Alternativas descartadas

- **Mantener Tauri 2:** descartada por el responsable.
- **Un instalador Wails aparte (`atenea-setup`) que llame al CLI:** duplicaría el diálogo de unión de la aplicación y añadiría otro binario que firmar.
- **Electron:** binarios y memoria muy mayores y un navegador empaquetado; no cumple el espíritu de ADR-011.

## Plan por fases

1. **ADR-019 aceptado** y los spikes S1–S6 (sin tocar `apps/desktop`).
2. **Esqueleto Wails** con el servicio de once métodos, el frontend conectado y la bandeja; pruebas de paridad del backend.
3. **Paridad completa** (notificaciones, inicio con la sesión, ping, abrir panel, operaciones elevadas) y **retirada de Tauri** (código Rust, `Cargo.lock`, `deny.toml`, jobs de CI).
4. **Empaquetado** por sistema operativo y entrada en el manifiesto firmado.
5. **Asistente de primera ejecución** (ADR propio de privilegios) y guías «Empezar» por sistema.
6. **Firma y notarización** cuando existan los certificados; **Windows** cuando se desaparque.

Cada publicación sigue siendo una entrega separada con autorización expresa.

## Límites

Nada de esto está implementado ni medido. Las afirmaciones sobre Wails salen de su documentación oficial y de ninguna compilación propia; los spikes S1–S6 existen para convertirlas en hechos antes de depender de ellas.
