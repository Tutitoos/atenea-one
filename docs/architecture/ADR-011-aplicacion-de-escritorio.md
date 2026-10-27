# ADR-011: Aplicación de escritorio (Tauri 2)

- **Estado:** **Aceptado** por el responsable el 2026-10-27 (elección de Tauri 2 frente a Wails v3). Implementada la lectura de estado en macOS; Windows y Linux compilados o sin comprobar según se indica en «Límites».
- **Relacionados:** ADR-007 (privilegios), ADR-008 (clientes macOS/Windows).

## Contexto

El agente expone un socket de **solo lectura** (`status_socket`) con el camino de cada par, el tráfico, el lease y los errores de sincronización. Hasta ahora solo se leía con `atenea status` (JSON). Se quiere una interfaz de escritorio como las de Tailscale o ZeroTier: icono en la bandeja con el estado y una ventana con los pares. El responsable descartó un panel web servido por un puerto local.

## Decisión

### 1. Tauri 2, con interfaz estática y sin servidor local

Aplicación `apps/desktop` (Tauri 2.12). La ventana es un webview del sistema (WKWebView, WebView2, WebKitGTK) que carga ficheros estáticos **embebidos en el binario**; no hay servidor HTTP, ni puerto local, ni contenido remoto. Se eligió frente a Wails v3 porque el repositorio es Rust: se reutiliza `crates/windows` para la canalización con nombre, el árbol entra en la misma política de `cargo-deny` y no se introduce Go ni un segundo ecosistema de suministro. Frente a SwiftUI/WinUI nativos, porque una sola base de código cubre macOS y Windows; **sigue siendo un webview**, no controles nativos.

### 2. Solo lectura y mínima superficie

- La única orden que el frontend puede invocar es `status`, que abre el **socket de estado**, envía `status` y devuelve el JSON. No existe ninguna orden de control (`stop`, `reload`), ni el socket de control se abre nunca.
- `capabilities/default.json` no concede ningún permiso de plugin. No se incluyen `fs`, `shell`, `http`, `opener`, `updater` ni `dialog`.
- CSP: `default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src ipc: http://ipc.localhost`. Sin `unsafe-inline`, sin `eval`, sin orígenes remotos.
- `withGlobalTauri` está activo: cualquier script cargado en la página puede invocar `status`. Es aceptable porque la página solo carga `app.js` propio y la orden es de solo lectura.
- La respuesta del agente se valida: JSON válido, sin campo `error`, con tope de 4 MiB.
- El estado contiene IP públicas de pares y claves públicas: la ventana no las envía a ningún sitio; el webview no tiene salida de red permitida por la CSP.

### 3. Sin privilegios

Se ejecuta como usuario normal. Lee el mismo socket que `atenea status`: en macOS el grupo `staff`, en Windows `BUILTIN\Users` si el servicio se instaló con `--status-gid 545` (ADR-008). La aplicación **no** instala ni controla el agente.

### 4. Workspace de Cargo independiente

`apps/desktop` tiene su `[workspace]` y su `Cargo.lock`: Tauri necesita WebKitGTK en Linux y no debe entrar en `cargo ... --workspace` del laboratorio ni en el grafo del resto del producto. Lo construyen los jobs de macOS y Windows de la CI; `scripts/check.sh` solo comprueba su formato. `cargo-deny` lo recorre con el mismo `deny.toml` (job `supply-chain`).

### 5. Licencia MPL-2.0 acotada

El árbol de Tauri incluye cinco crates con **MPL-2.0** (copyleft a nivel de fichero): `cssparser`, `cssparser-macros`, `selectors` y `dtoa-short`, que entran por la macro de compilación (`tauri-codegen` → `dom_query`), y `option-ext`, que se enlaza (`dirs`). Se admiten **solo esos cinco, sin modificar**, mediante `exceptions` en `deny.toml`; MPL-2.0 no se añade a la lista global. Además se permite `Apache-2.0 WITH LLVM-exception` (Apache-2.0 con permisos adicionales; solo `target-lexicon`, árbol de Linux). La obligación asociada es conservar sus avisos y poder indicar dónde está su código fuente (crates.io, versión fijada en `apps/desktop/Cargo.lock`).

## Alternativas descartadas

- **Panel web local (`atenea ui`)**: descartado por el responsable; además exigía defenderse de DNS rebinding. Se ha retirado.
- **Wails v3**: viable (también trae bandeja y sin puerto), pero obliga a reescribir el cliente IPC en Go (y una dependencia para canalizaciones en Windows) y a duplicar la cadena de suministro.
- **App nativa Swift para la barra de menús**: se construyó y se retiró; solo cubría macOS.

## Límites

- **Medido (2026-10-27, macOS arm64):** árbol de producción de 213 paquetes en macOS, 254 en Linux y 222 en Windows (`cargo tree`), frente a 122 del CLI; binario *release* de 10,4 MiB; unos 98 MiB de RSS en ejecución. `unsafe_code = "forbid"` compila, incluida la macro `generate_context!`. Lectura real del socket de un agente en marcha y ventana con tráfico en vivo.
- **Sin comprobar:** ejecución en Windows (solo compila en CI si el job pasa; no se ha ejecutado), Linux (necesita WebKitGTK instalado y no se compila en CI), el menú de la bandeja con clic, varios monitores, macOS anterior a 27, y `cargo-deny` en local (lo ejecuta la CI).
- **Distribución:** sin firma de desarrollador ni notarización; el binario no va en el `.pkg` ni en el ZIP de Windows, no arranca solo con la sesión y no se actualiza solo (no hay *updater*).
- **Latencia:** el agente solo mide RTT por relay; en camino directo la columna sale vacía.
- **Nombres:** los nombres de dispositivo no existen en la configuración firmada; se guardan en el almacenamiento local del webview. Incluirlos en la configuración firmada exigiría un nuevo formato firmado y su propio ADR.
- **Seguridad:** la aplicación no es un límite entre usuarios de la misma máquina: quien pueda leer el socket de estado ve lo mismo.
