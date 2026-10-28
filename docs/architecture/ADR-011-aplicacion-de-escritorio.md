# ADR-011: Aplicación de escritorio (Tauri 2)

- **Estado:** **Aceptado** por el responsable el 2026-10-27 (elección de Tauri 2 frente a Wails v3). Implementada la lectura de estado en macOS; Windows y Linux compilados o sin comprobar según se indica en «Límites». Revisado el 2026-10-28 por ADR-013 fase 4: la app ya puede pedir operaciones sobre redes (§2).
- **Relacionados:** ADR-007 (privilegios), ADR-008 (clientes macOS/Windows).

## Contexto

El agente expone un socket de **solo lectura** (`status_socket`) con el camino de cada par, el tráfico, el lease y los errores de sincronización. Hasta ahora solo se leía con `atenea status` (JSON). Se quiere una interfaz de escritorio como las de Tailscale o ZeroTier: icono en la bandeja con el estado y una ventana con los pares. El responsable descartó un panel web servido por un puerto local.

## Decisión

### 1. Tauri 2, con interfaz estática y sin servidor local

Aplicación `apps/desktop` (Tauri 2.12). La ventana es un webview del sistema (WKWebView, WebView2, WebKitGTK) que carga ficheros estáticos **embebidos en el binario**; no hay servidor HTTP, ni puerto local, ni contenido remoto. El frontend es una SPA con **React Router 8.5 (`ssr: false`), Tailwind 4 y TypeScript**, el mismo stack y las mismas versiones fijadas que `apps/web`; se compila a `build/client` y el script `scripts/csp.mjs` (el mismo que usa el panel) saca los scripts en línea a ficheros para poder mantener una CSP sin `unsafe-inline`. Las rutas son reales (`/devices/:id`, `/diagnostics`) y la lectura del agente usa TanStack Query con un sondeo de 2 s. Se eligió frente a Wails v3 porque el repositorio es Rust: se reutiliza `crates/windows` para la canalización con nombre, el árbol entra en la misma política de `cargo-deny` y no se introduce Go ni un segundo ecosistema de suministro. Frente a SwiftUI/WinUI nativos, porque una sola base de código cubre macOS y Windows; **sigue siendo un webview**, no controles nativos.

### 2. Lectura sin privilegios y mínima superficie

> **Revisado por ADR-013 fase 4 (2026-10-28).** La versión original decía «solo lectura» y una única orden `status`. La aplicación ya **no es de solo lectura**: puede pedir cinco operaciones que cambian el sistema (unirse, conectar, desconectar, salir e importar la instalación heredada). Lo que no cambia es que **la app no tiene privilegios ni implementa esas operaciones**: pide la autorización del sistema operativo y ejecuta el CLI.

- Órdenes que el frontend puede invocar (las de `generate_handler!`): `status(network)` (lee el socket de estado de **una red unida**, cuyo id es un UUID válido, sin enviar órdenes de control), `network_list` (`atenea network list --json`, sin privilegios), `network_legacy` (`atenea network legacy --json`, sin privilegios: `{"present": bool}`), `network_inspect` (decodifica un código sin tocar disco ni red) y `network_apply` (`join`, `up`, `down`, `leave` y `migrate`). Ningún socket de control se abre nunca.
- **Sin instancia «Predeterminada»** (decisión del responsable, 2026-10-28): una red es una instancia del modelo de redes (ADR-013). La app ya no lee el socket heredado ni existe `ATENEA_STATUS_SOCKET`; sin redes unidas no hay estado: la bandeja muestra «Sin redes» con el icono gris, y Dispositivos y Diagnóstico un estado vacío con enlace a Redes. El selector de red ofrece solo redes reales (la guardada, si no la primera). Si queda una instalación heredada, Redes la ofrece como banner («Importar red existente») con confirmación y nombre opcional.
- `network_apply` **solo** ejecuta esas cinco operaciones del CLI con argumentos validados en Rust antes de construir la orden: el código se normaliza y debe ser `ATENEA1-` + alfabeto base32 + guiones (máx. 512), el id de red es un UUID canónico en minúsculas y la ruta del CLI es fija (o `ATENEA_CLI`, solo desarrollo). En `join`, el id para el `up` posterior sale del propio código (lo decodifica el CLI), nunca del frontend. `migrate` ejecuta exactamente `<cli> network migrate-legacy --yes [--name 'N']`, donde N es opcional, de 1 a 64 caracteres de `[A-Za-z0-9._ -]`, sin espacios en los extremos ni guion inicial (alfabeto deliberadamente más estricto que el de los nombres de dispositivo: no puede cerrar las comillas simples ni lleva controles ni bidi). No hay otro texto libre del usuario en la orden elevada.
- **La autoridad es el sistema operativo:** macOS muestra su diálogo de administrador (`osascript … with administrator privileges`) y Linux `pkexec`, en **cada** operación; no hay servicio residente ni credencial guardada. Windows devuelve «no soportado». **Ninguno de los dos diálogos se ha ejecutado todavía** (ver Límites). Cualquier script de la página podría invocar `network_apply`, pero no puede saltarse ese diálogo, y la CSP solo admite scripts propios.
- Confirmación en dos pasos para unirse: «Revisar código» (solo `network_inspect`) muestra nombre, servidor, id, **huella de la raíz** y tipo de código, con el aviso de comparar por otro canal; solo «Unirme a esta red» llama a `network_apply`. Salir de una red pide una confirmación aparte.
- El token de un código con token es visible un instante en la línea de órdenes de la operación elevada (`ps`), igual que con el CLI (ver la guía de redes).
- `capabilities/default.json` no concede ningún permiso de plugin. No se incluyen `fs`, `shell`, `http`, `opener`, `updater` ni `dialog`.
- CSP: `default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src ipc: http://ipc.localhost`. Sin `unsafe-inline`, sin `eval`, sin orígenes remotos.
- `withGlobalTauri` está **desactivado**: el frontend importa `@tauri-apps/api` empaquetado y no existe `window.__TAURI__`. Aun así, cualquier script de la página podría invocar `status`; es aceptable porque la CSP solo admite scripts propios y la orden es de solo lectura.
- **Frontera tipada:** la respuesta se valida con `zod` (se toleran campos ausentes de agentes anteriores y se rechaza cualquier otra forma). La salud (`ok`/`degraded`/`down`, su texto y sus motivos) la calcula **Rust una sola vez** (`assess`) y la comparten la bandeja y la ventana: el frontend no repite el criterio.
- La respuesta del agente se valida: JSON válido, sin campo `error`, con tope de 4 MiB.
- El estado contiene IP públicas de pares y claves públicas: la ventana no las envía a ningún sitio; el webview no tiene salida de red permitida por la CSP.

### 3. Sin privilegios

Se ejecuta como usuario normal. Lee el socket de estado de cada red unida, igual que `atenea status`. Las redes unidas desde la app se crean con `--status-gid` = grupo primario del usuario (en macOS, `staff`), de modo que su socket de estado sea legible sin privilegios; una red unida desde el CLI sin esa opción aparece como «Sin acceso al estado». La aplicación **no** instala el agente ni arranca nada por sí misma: cualquier cambio pasa por la autorización del sistema.

### 4. Workspace de Cargo independiente

`apps/desktop` tiene su `[workspace]` y su `Cargo.lock`: Tauri necesita WebKitGTK en Linux y no debe entrar en `cargo ... --workspace` del laboratorio ni en el grafo del resto del producto. Lo construyen los jobs `desktop-macos` y `desktop-windows` de la CI (solo cuando se lanza la CI, que desde 2026-10-28 es solo a demanda; véase [CI](../operations/ci.md)), que reciben el frontend ya compilado como artefacto del job `panel` (no necesitan Node ni pnpm); `scripts/check.sh` solo comprueba su formato. `cargo-deny` lo recorre con el mismo `deny.toml` (job `supply-chain`).

### 5. Licencia MPL-2.0 acotada

El árbol de Tauri incluye cinco crates con **MPL-2.0** (copyleft a nivel de fichero): `cssparser`, `cssparser-macros`, `selectors` y `dtoa-short`, que entran por la macro de compilación (`tauri-codegen` → `dom_query`), y `option-ext`, que se enlaza (`dirs`). Se admiten **solo esos cinco, sin modificar**, mediante `exceptions` en `deny.toml`; MPL-2.0 no se añade a la lista global. Además se permite `Apache-2.0 WITH LLVM-exception` (Apache-2.0 con permisos adicionales; solo `target-lexicon`, árbol de Linux). La obligación asociada es conservar sus avisos y poder indicar dónde está su código fuente (crates.io, versión fijada en `apps/desktop/Cargo.lock`).

## Alternativas descartadas

- **Panel web local (`atenea ui`)**: descartado por el responsable; además exigía defenderse de DNS rebinding. Se ha retirado.
- **Wails v3**: viable (también trae bandeja y sin puerto), pero obliga a reescribir el cliente IPC en Go (y una dependencia para canalizaciones en Windows) y a duplicar la cadena de suministro.
- **App nativa Swift para la barra de menús**: se construyó y se retiró; solo cubría macOS.

## Límites

- **Medido (2026-10-27, macOS arm64):** árbol de producción de 213 paquetes en macOS, 254 en Linux y 222 en Windows (`cargo tree`), frente a 122 del CLI; binario *release* de 10,4 MiB; unos 98 MiB de RSS en ejecución. `unsafe_code = "forbid"` compila, incluida la macro `generate_context!`. Lectura real del socket de un agente en marcha y ventana con tráfico en vivo.
- **Sin comprobar (fase 4):** la elevación real. `network_apply` se probó solo hasta la construcción de la orden (pruebas unitarias del guion y su validación), no hasta el diálogo de administrador de macOS ni `pkexec` en Linux; la unión real desde la app está pendiente de una prueba en un Mac con autorización del propietario. Tampoco se ha probado la cancelación del diálogo (se asume el mensaje de `osascript` «User canceled» y los códigos 126/127 de polkit). La lectura (lista de redes, `inspect`, estado por red) sí se ejecutó con el CLI y la app reales contra un directorio de redes y un socket de estado sintéticos.
- **Sin comprobar:** ejecución en Windows (solo compila en CI si el job pasa; no se ha ejecutado), Linux (necesita WebKitGTK instalado y no se compila en CI), el menú de la bandeja con clic, varios monitores, macOS anterior a 27, y `cargo-deny` en local (lo ejecuta la CI).
- **Distribución:** sin firma de desarrollador ni notarización; el binario no va en el `.pkg` ni en el ZIP de Windows, no arranca solo con la sesión y no se actualiza solo (no hay *updater*).
- **Pruebas del frontend:** Vitest con Testing Library y `axe` en jsdom (con `invoke` simulado: tasas de tráfico, tolerancia del esquema, búsqueda, nombre local, error del agente, respuesta inválida, selector de red sin «Predeterminada», estado vacío sin redes, redes —conectar, salir con confirmación, unirse en dos pasos, código no válido y cancelación—, banner de instalación heredada con su confirmación y accesibilidad de las cuatro pantallas). jsdom no calcula layout ni contraste: el contraste y el aspecto solo se han visto en macOS.
- **Sin comprobar (importar la instalación heredada):** `network_legacy` y `op = "migrate"` se probaron solo con `invoke` simulado (frontend) y con pruebas unitarias del guion y del nombre en Rust; no contra `atenea network legacy` / `migrate-legacy` reales ni con el diálogo de administrador.
- **Latencia:** el agente solo mide RTT por relay; en camino directo la columna sale vacía.
- **Etiqueta «Servidor»:** ni el estado del agente ni la configuración firmada dicen qué dispositivo hace de servidor (desde un Mac, el `endpoint` del VPS puede ser una IP de Tailscale y no la del panel, de modo que reconocerlo por dirección o por nombre sería adivinar). La etiqueta es una marca **local de este equipo**, que el usuario activa en el detalle de un dispositivo y se guarda por clave pública en el almacenamiento del webview, igual que los alias. No viaja a ningún sitio y otro equipo no la ve. Hacerla común exigiría un campo de rol en el servidor (migración, API y panel) y su propio ADR.
- **Nombres:** los nombres de dispositivo no existen en la configuración firmada; se guardan en el almacenamiento local del webview. Incluirlos en la configuración firmada exigiría un nuevo formato firmado y su propio ADR.
- **Seguridad:** la aplicación no es un límite entre usuarios de la misma máquina: quien pueda leer el socket de estado ve lo mismo.
