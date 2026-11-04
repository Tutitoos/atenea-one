<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-021: Asistente de primera ejecución de la aplicación de escritorio (modelo de privilegios)

- **Estado:** **Aceptado** por el responsable el 2026-11-04, con las siete decisiones de «Decisiones del responsable» tal como se redactaron. Redactado a partir de la lectura del código de `main`; **implementación en curso** (plan al final).
- **Relacionados:** [ADR-019](ADR-019-aplicacion-de-escritorio-wails.md) (decisión 8: el asistente va dentro de la aplicación y «su diseño de privilegios se decide en un ADR posterior»), [ADR-011](ADR-011-aplicacion-de-escritorio.md) §2 y §3 (reglas de privilegios, que siguen vigentes), [ADR-018](ADR-018-union-solo-con-el-codigo.md) (unirse con solo el código, `--up` y aprobación manual), [ADR-013](ADR-013-redes-multiples-y-union-por-codigo.md) §12 (`--stun-host` y `--relay-ca`), [ADR-017](ADR-017-redes-por-instancia-en-windows.md) (Windows), [ADR-007](ADR-007-subredes-dns-privilegios.md) §6.

## Contexto

Hechos comprobados en el código (`main`, 2026-11-04):

- **Unirse ya existe.** `apps/desktop/app/routes/networks.tsx` tiene el flujo en dos pasos: pegar el código y «Revisar código» (`network_inspect`, sin privilegios, sin disco ni red), y «Unirme a esta red» (`network_apply`, con el diálogo de autorización del sistema). Tras unirse enseña la huella del dispositivo que el administrador compara al aprobar. El script elevado es `atenea network join <código> --yes --status-gid <gid> && atenea network up <id>` (`internal/backend/ops.go`): **un solo diálogo** une y arranca.
- **No hay «primera vez».** Ni asistente, ni fichero, ni indicador; la única persistencia propia es `notifications.json`. La ventana está oculta al arrancar salvo con `--open`: Linux y Windows lanzan con `--open`, pero **en macOS la app es `LSUIElement` y al abrirla desde el DMG solo aparece un icono en la barra de menús**, sin ventana ni Dock.
- **La app no distingue «cliente no instalado» de «sin redes».** `backend.List()` y `Legacy()` devuelven `[]` y `{"present":false}` sin error si el CLI no existe (`networks.go`); la bandeja dice «Sin redes» en ambos casos. Si el usuario pega un código sin CLI, ve el error crudo del sistema (`fork/exec … no such file or directory`).
- **El CLI y el agente no salen de la aplicación.** Los instalan su `.pkg` (macOS, `/Library/Atenea/bin`), su `.deb` (Linux, `/usr/bin`) y el ZIP de Windows; la app de escritorio no descarga ni instala nada (comprobado por búsqueda en `main.go`, `service.go`, `internal/` y `app/`).
- **El CLI tiene opciones que la app no expone:** `--name`, `--stun-host`, `--relay-ca` (**una ruta absoluta a un fichero**, que el proceso elevado leería), `--insecure-http`, `leave --offline`. ADR-013 §12 lo dejó explícito: «la aplicación de escritorio no expone estas opciones todavía». Con ADR-018 el código ya lleva el STUN y el hash de la CA del relay.
- **El inicio con la sesión** es un fichero del propio usuario (`LaunchAgent` o `.desktop`), apagado por defecto y solo en Diagnóstico.
- **Windows no puede unirse** desde la app (ADR-017 sin implementar): `Elevate` devuelve «todavía no está soportado».
- El servicio vinculado expone **exactamente 11 métodos** y una prueba por reflexión falla si crece (ADR-019, decisión 2 y 10).

## Decisión

**El asistente es una guía sobre las operaciones que la aplicación ya tiene, no un camino nuevo hacia los privilegios.**

1. **Ninguna operación elevada nueva y ningún argumento nuevo en la orden elevada.** Se mantiene la lista cerrada (`join`, `up`, `down`, `leave`, `migrate`) y las cadenas exactas de `Script`, con sus validaciones. El asistente llama a `network_apply` igual que la pantalla «Redes».
2. **La app nunca instala, descarga ni ejecuta el cliente.** Si falta, el asistente lo dice y enseña, como texto copiable, la orden o el paquete que corresponde a ese sistema, con un botón «Comprobar de nuevo» (sin temporizadores). **Sin enlaces ni apertura de URL**: abrir una URL exigiría un método nuevo y el webview sigue sin salida de red. Un instalador lanzado desde la app sería un canal de ejecución de código sin ADR de actualizaciones ni firma (ADR-019, decisión 9).
3. **Una sola consulta de solo lectura nueva: `ClientStatus()`.** Devuelve únicamente `{os, supported, installed}` (booleanos y el nombre del sistema), **sin rutas, versiones ni texto del sistema**, y no abre ningún socket ni ejecuta el CLI: solo comprueba que el fichero existe. El servicio pasa de 11 a **12 métodos**; la prueba de superficie se actualiza en el mismo cambio y la revisión de este ADR cubre esa ampliación. Con esto el asistente distingue «cliente no instalado», «sin redes» y «sistema no soportado».
4. **«Primera ejecución» es un fichero de usuario, escrito por Go, sin métodos nuevos.** Al arrancar, si `first-run.json` no existe en la carpeta de configuración de la app, **se abre la ventana una sola vez** (resuelve el caso de macOS, donde hoy no se vería nada) y se escribe el fichero. Después, la pantalla vacía («Sin redes») enlaza al asistente. No hay credenciales ni datos de la red en ese fichero. El asistente no depende de él para funcionar: el estado se **deriva** cada vez (cliente, redes unidas, instalación heredada) y se puede reabrir.
5. **Pasos del asistente:**
   1. *Sistema y cliente* (solo lectura, `ClientStatus`). Windows: «todavía no soportado», sin más pasos.
   2. *Instalación heredada* (si `network_legacy` dice que hay): ofrece importar (`migrate`, ya existente) antes de unirse a otra.
   3. *Unirse* (el flujo actual de «Revisar código» y «Unirme a esta red»): nombre, servidor, id y huella de la raíz, conectividad del código y el aviso de compararlos por otro canal. **Nada se ejecuta con privilegios hasta que el usuario pulsa «Unirme a esta red»**: sin avance automático, sin temporizadores y sin pulsación implícita.
   4. *Aprobación*: si el código no lleva token, muestra la huella del dispositivo para que el administrador la compare y espera con la consulta de solo lectura que ya hace la aplicación (`network_list`, cada 5 s). «Abrir en el panel» usa `OpenPanel`, ya existente y limitado a HTTPS de una red unida.
   5. *Opcional*: «Abrir al iniciar sesión» (apagado por defecto, con `AutostartGet/Set` existentes). Una nota dice dónde está el icono de la bandeja (en macOS, la barra de menús).
6. **Una autorización del sistema por operación, como hoy.** Unir y arrancar es un solo diálogo (`join … && up`). Cancelar el diálogo no cambia nada y deja el asistente en el paso anterior con «Cancelado» y la opción de reintentar. No hay servicio residente ni credencial guardada (ADR-011 §2).
7. **Las opciones avanzadas no salen en el asistente.** `--relay-ca` hace que un proceso con privilegios lea un fichero elegido por el usuario desde una interfaz web: es un delegado confuso innecesario, porque el código ya lleva el hash de la CA (ADR-018). `--stun-host`, `--name` e `--insecure-http` tampoco: el nombre del dispositivo es el del sistema (el administrador puede renombrarlo, ADR-012). Si el código **no** anuncia conectividad, el asistente lo dice y muestra la orden de terminal con `--stun-host`, como hace el panel.
8. **El diálogo de autorización nombra la red cuando es seguro hacerlo.** El texto de `join` incluye el nombre del código **solo si** cumple `[A-Za-z0-9._ -]`: el texto del diálogo no puede contener `"` ni `\` (`ElevationCommand` los rechaza en macOS) y el nombre de un código admite más caracteres (hasta 48 bytes sin controles ni marcas bidi). Si no cumple, queda el texto genérico de hoy. Cambia el texto que ve el usuario, **no la orden** que se ejecuta.
9. **Lo que no cambia:** aprobación manual por huella, el `network_id` sale del propio código y no del frontend, ruta fija del CLI (`ATENEA_CLI` solo para desarrollo), CSP y filtro del runtime, webview sin salida de red, sin *updater*.

## Amenazas

| Amenaza | Respuesta |
| --- | --- |
| Código pegado hostil o manipulado | Validación existente (`NormalizeCode`, UUID, `inspect` sin efectos) **antes** de construir la orden; sin cambios. |
| El asistente se salta el diálogo o une sin que el usuario lo pida | No existe tal camino: la autoridad es el diálogo del sistema en cada operación y el asistente solo avanza con la pulsación del usuario. Un webview comprometido seguiría sin poder saltarse el diálogo; con la decisión 8 el diálogo dice a qué red. |
| El usuario une su equipo a una red de otro por error | Confirmación con nombre, servidor, id y huella, aviso de comparar por otro canal y aprobación manual del administrador (ADR-018, §4). Sin cambios. |
| La app como vía de instalación de código | Descartada por diseño (decisión 2): no descarga, no instala, no abre URL. |
| Ruta de fichero controlada por el usuario leída por root | `--relay-ca` no se expone (decisión 7). |
| `ClientStatus` filtra información | Devuelve tres valores fijos; ninguna ruta ni versión. |
| Ventana que se abre sola | Una vez por perfil (`first-run.json`), sin privilegios; el inicio con la sesión sigue oculto. |
| Windows | El asistente no ofrece nada que no pueda hacer (ADR-017). |

## Alternativas descartadas

- **El asistente instala el cliente** (descarga y lanza el `.pkg` o el `.deb` con autorización): convierte la app en un canal de ejecución de código, sin firma de desarrollador ni ADR de actualizaciones.
- **Detectar la ausencia del cliente por el texto del error de `network_list`:** frágil y distinto en cada sistema, y mezcla error con estado.
- **Un fichero de «primera vez» escrito desde el frontend:** exige un método más; escribirlo desde Go al abrir la ventana por primera vez no.
- **Exponer `--stun-host` y `--relay-ca` en un «modo avanzado»:** amplía la orden elevada y, en `--relay-ca`, deja que root lea una ruta elegida por la interfaz; con ADR-018 no hace falta.
- **Pedir el nombre del dispositivo en el asistente:** es texto libre nuevo en la orden elevada (ADR-011 §2 lo restringe) y el nombre del sistema ya es el predeterminado.
- **Un instalador con pantallas propias** (NSIS o PKG interactivo): ya descartado por ADR-019, decisión 8.

## Pruebas exigidas (criterios de aceptación)

1. `Script` y `ParseOp` **no cambian**: las pruebas existentes de órdenes elevadas exactas siguen pasando sin tocarlas.
2. `ClientStatus`: dato por sistema (cliente presente, ausente, sistema no soportado) sin ejecutar nada; la prueba de superficie pasa a 12 métodos con ese nombre.
3. `first-run.json`: ausente → una apertura y escritura atómica; presente → no abre; ilegible → no abre y no falla.
4. Texto del diálogo de `join`: con nombre del alfabeto permitido lo incluye; con comillas, barras o caracteres fuera del alfabeto vuelve al genérico (y nunca llega a `ElevationCommand` con `"` o `\`).
5. Frontend (Vitest con el puente simulado): cada paso por separado; **ninguna llamada a `network_apply` sin la pulsación**; cancelar el diálogo deja el paso anterior; cliente ausente y Windows no ofrecen «Unirme»; accesibilidad con `vitest-axe`.
6. Humo con la aplicación empaquetada en los tres sistemas (como hoy).
7. **Sin comprobar hasta probarlo a mano:** los diálogos reales de `osascript` y `pkexec` (cancelación incluida) y la apertura de la ventana en un Mac recién instalado; la CI no tiene escritorio interactivo.

## Decisiones del responsable (2026-11-04)

Aceptadas por el responsable el 2026-11-04, **las siete tal como se redactaron** (la recomendación es la decisión):

1. **`ClientStatus` como método nuevo de solo lectura (12 métodos).** Recomendado. Alternativa: un error tipado en `network_list` (descartada por frágil).
2. **Abrir la ventana una sola vez en la primera ejecución** (`first-run.json`). Recomendado: sin ello, en macOS el usuario no ve nada tras abrir el DMG.
3. **Opciones avanzadas fuera del asistente**, con la orden de terminal cuando el código no lleve conectividad. Recomendado.
4. **No pedir el nombre del dispositivo.** Recomendado.
5. **«Abrir al iniciar sesión» como último paso opcional, apagado por defecto.** Recomendado. Alternativa: encendido por defecto, que escribe un fichero de arranque sin que el usuario lo haya pedido.
6. **El diálogo de autorización nombra la red** cuando el nombre cumple el alfabeto seguro. Recomendado.
7. **El cliente ausente se resuelve con texto copiable, sin enlaces ni descargas.** Recomendado.

## Fuera de alcance

Windows (ADR-017), instalar o actualizar el cliente desde la app, aprobación automática, un asistente en el panel web, i18n más allá del español actual, y la firma y notarización (ADR-019, fase 6).

## Plan

1. **Backend — hecho** (pruebas y mutaciones en `apps/desktop`): `ClientStatus`, `first-run.json` (`internal/firstrun`, reclamo exclusivo con `O_EXCL`) y texto del diálogo de `join` con el nombre de la red cuando es seguro; la prueba de superficie pasa a doce métodos. Comprobado con el binario real y un `HOME` temporal: la primera ejecución crea el fichero y la segunda no lo toca, y la prueba de humo no lo crea.
2. Frontend: ruta del asistente (pasos 1 a 5) reutilizando `Join` y los componentes existentes; enlace desde «Sin redes».
3. Documentación (`instalacion-escritorio.md`, guías de instalación) y límites en ADR-019.
4. Comprobación manual de los diálogos de `osascript` y `pkexec` y de la apertura de la ventana en macOS y Linux.
