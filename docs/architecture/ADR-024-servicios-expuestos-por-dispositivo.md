<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-024: Servicios expuestos por dispositivo (publicarlos y abrirlos con un clic)

- **Estado:** **Aceptado** por el responsable el 2026-11-08, con las cuatro decisiones D1 a D4 tal como se redactaron (la recomendación de cada una). **Implementación por fases**; la fase 2 (detección local en el agente) está implementada en el PR #103 y se comprobó el formato de macOS con la salida real; Linux y Windows solo con muestras. Nada de lo que describe está publicado.
- **Relacionados:** [ADR-012](ADR-012-nombres-de-dispositivo.md) (el canal de metadatos de presentación que se reutiliza), [ADR-014](ADR-014-servidores-roles-y-redes-por-servidor.md) (`role` en `/device/peers`), [ADR-016](ADR-016-politicas-de-acceso-minimas.md) (políticas y ACL sin estado), [ADR-019](ADR-019-aplicacion-de-escritorio-wails.md) y [ADR-021](ADR-021-asistente-de-primera-ejecucion.md) (la aplicación solo abre el panel y expone una lista cerrada de métodos), [ADR-013](ADR-013-redes-multiples-y-union-por-codigo.md) (una instancia de agente e interfaz por red) y [ADR-017](ADR-017-redes-por-instancia-en-windows.md).

## Contexto

Hoy, para abrir un servicio de otro dispositivo (por ejemplo, un Storybook o una API de desarrollo) hay que saber su dirección dentro de la red y su puerto. Nada en Atenea sabe qué escucha cada equipo. Hechos comprobados en el código de `main` (2026-11-08):

- **No existe** ningún campo, tabla, endpoint ni lectura de puertos o servicios. El agente solo anuncia al coordinator sus candidatos de conexión (`apps/agent/src/coord.rs`).
- **Existe el canal de metadatos de presentación** (ADR-012 y ADR-014), que es el que conviene reutilizar: cada 15 s el agente construye `sync::Report` (`apps/agent/src/sync.rs:131-143`, lista blanca de campos) y lo envía firmado (AtDevice) a `POST /api/v1/device/status`; el control plane lo valida con `deny_unknown_fields`, límites de tamaño, nonce y tasa (`apps/control-plane/src/panel.rs:106-224`) y guarda el `hostname` en `devices.hostname` (migración `0006`). Los demás dispositivos lo reciben por `GET /api/v1/device/peers`, que devuelve `{device_id, name, role}` **filtrado por las mismas políticas** que la configuración firmada (`apps/control-plane/src/issue.rs:156`, `compile`). El agente lo guarda en su estado local (`Engine::set_metadata`, `engine.rs:1940`) y lo publica en el socket de estado de solo lectura (`ipc.rs:75-77`), que es lo que lee la aplicación de escritorio.
- **Los metadatos no viajan en el contrato firmado** (`SignedConfig`): son de presentación y no autorizan ni enrutan (ADR-012).
- **La aplicación de escritorio solo abre el panel** (`OpenPanel`, `apps/desktop/internal/panel/panel.go`). La superficie del servicio Wails está fijada por una prueba a **12 métodos** (`apps/desktop/main_test.go:19-20`). ADR-021 fijó «no abre URL» más allá del panel.
- **El agente lee la red del sistema con comandos externos de ruta absoluta** (`ip`, `ifconfig`/`netstat`, PowerShell) y los parsea con pruebas de muestra (`apps/agent/src/platform/{linux,macos,windows}.rs`). No lee sockets en escucha.
- **El ACL no limita puertos en la práctica** (ADR-016 y `docs/api/control-plane-v1.md`, «Bidireccionalidad efectiva»): una política A→B `tcp/22` permite también a B iniciar TCP a A en cualquier puerto. Que un servicio esté publicado **no** lo hace más ni menos alcanzable: lo que decide si se llega son las políticas y el cortafuegos de cada equipo.
- **Cada red es una interfaz y un agente propios** (ADR-013): la lista de servicios es **por red**, porque una red es un ámbito de acceso distinto.

## Decisión

**Cada agente detecta los servicios TCP que escucha el equipo y, si está activado, los publica en el canal de metadatos de presentación; los dispositivos con política hacia él los ven y los abren con un clic desde la aplicación.** Publicar no abre nada: no crea políticas ni toca cortafuegos.

1. **Qué se detecta.** Sockets TCP en escucha cuya dirección local es la comodín (`0.0.0.0`, `::`) o una dirección **de esa red** (la de su interfaz). Se excluyen loopback, direcciones de otra red y los puertos del propio agente (UDP del túnel y sockets locales). Lectura sin guardar el proceso: Linux lee `/proc/net/tcp` y `/proc/net/tcp6` (estado `0A`) sin privilegios; macOS parsea `netstat -an -p tcp` (ya está entre los binarios de la plataforma) **y necesita privilegios**: en macOS 27.0.1 sin ellos imprime la cabecera y ninguna fila TCP (comprobado), y como administrador lista `tcp4`, `tcp6` y `tcp46` con el formato esperado, que es el caso del agente del servicio; Windows usa `Get-NetTCPConnection -State Listen` con salida JSON, como `Get-NetIPAddress`. Se añade un método a `HostNetwork` y una función `parse_*` por sistema con pruebas de muestra (incluida la forma exacta de Windows: aquí ya falló un `parse` por un detalle de formato real, PR #79).
2. **Qué se publica.** Por servicio: `port` (1–65535), `proto` (`tcp`), `scheme` opcional (`http` o `https`, por defecto `http`) y `label` opcional de hasta 32 caracteres con la misma lista de caracteres que los nombres de dispositivo. **Nunca** se publican el nombre del proceso, la ruta del ejecutable, el usuario ni la dirección local. Máximo 64 servicios por dispositivo; si hay más se recorta y se avisa en el estado local.
3. **Cómo viaja.** Se añade `services` a `sync::Report` y al `deny_unknown_fields` del control plane, **solo si la publicación está activada**. La lista sustituye a la anterior en cada informe (cada 15 s), sin historial. Se guarda como `devices.services jsonb` (migración `0010`) con `services_at`. Se aplican los mismos límites de tamaño, nonce y tasa que ya tiene `/device/status`.
4. **Quién la ve.** `GET /api/v1/device/peers` devuelve `services` por par, con **la misma regla que el resto del par: solo si hay política entre ambos** (ADR-016). No se publica para toda la red. Un servicio sin informe reciente (`services_at` con más de 120 s) se omite. El administrador de la organización la ve también en el inventario del panel (`Device.services`).
5. **Cómo se abre.** La aplicación gana **un método nuevo**, `OpenService(network, deviceID, port)` (la superficie pasa de 12 a 13, con su prueba). **La interfaz nunca envía una URL:** el backend de Go la construye con la dirección IPv4 de ese par (de su estado local, IPv6 entre corchetes si no hay IPv4), el puerto **que ese par haya publicado** y el `scheme` publicado, y la abre con el navegador del sistema. No abre nada que no esté en la lista de un par de la red elegida. La interfaz muestra además un botón de copiar. Se actualiza la regla de ADR-021 («no abre URL») con esta excepción acotada.
6. **Etiquetas y esquema.** En esta versión solo se pueden fijar en la configuración del agente (`[services]`), no desde la aplicación. Eso evita un camino nuevo de escritura con privilegios.
7. **Lo que no cambia:** el contrato firmado, el protocolo del túnel, las políticas, la aprobación por huella y el filtrado del runtime de Wails. **No** se generan políticas, reglas de cortafuegos ni redirecciones a partir de lo detectado.

### Decisiones del responsable (aceptadas con la recomendación)

| # | Decisión | Aceptado | Alternativa descartada |
|---|---|---|---|
| D1 | **¿Publicación activada por defecto?** | **Apagada por defecto** (`[services] publish = false`), con un interruptor «Publicar mis servicios en esta red» en la aplicación. Una vez activo, todo lo que empiece a escuchar se detecta solo | Encendida por defecto: más cómodo, pero cada equipo que se una empieza a publicar sin haberlo decidido |
| D2 | **¿Quién ve la lista de un dispositivo?** | Solo los pares con política hacia él (como el resto de `/device/peers`) | Todos los miembros de la red: más fácil de descubrir, pero muestra servicios a quien no puede llegar a ellos |
| D3 | **¿Qué se filtra por defecto?** | Nada salvo lo indicado en la decisión 1; el usuario puede excluir puertos con `exclude = [..]` | Excluir de serie puertos de sistema (22, 25, 53, 135, 445, 5355…): menos ruido, pero oculta servicios legítimos |
| D4 | **¿`scheme` por defecto?** | `http`, con `scheme = "https"` por servicio | Detectarlo sondeando el puerto: ejecuta tráfico hacia servicios del propio equipo y se descarta |

## Amenazas y límites

| Riesgo | Tratamiento |
| --- | --- |
| La lista de puertos es un mapa de superficie de ataque | Opt-in (D1); solo la ven los pares con política (D2); sin procesos ni rutas; sin historial |
| Un dispositivo comprometido publica servicios falsos para llevar a otro a una URL maliciosa | La URL la construye el backend con la **IP del propio par** y puertos de **su** lista: no puede apuntar a otro host. `label` se pinta como texto, con lista cerrada de caracteres |
| Que un servicio figure en la lista se confunde con que esté permitido | La documentación y la interfaz lo dicen: publicar no abre nada; el acceso lo deciden las políticas y el cortafuegos. Ver «Bidireccionalidad efectiva» |
| Un agente nuevo contra un control plane antiguo | `Report` es `deny_unknown_fields`: un `services` desconocido haría fallar **todo** el informe. Por eso solo se envía con la publicación activada, y **el servidor se actualiza antes de activar nadie** (orden de despliegue en «Plan») |
| Desbordamiento o abuso | Máximo 64 servicios, longitudes acotadas, mismos límites de tasa y tamaño que `/device/status` |
| Datos obsoletos | `services_at` y omisión pasados 120 s; el estado local muestra «visto hace N s» |
| Windows con una sola red (ADR-017) | La función funciona sobre la red única; sin cambios en el modelo |

## Plan por fases

Cada fase es independiente y se puede publicar sola.

1. **ADR** (este documento) y su aceptación.
2. **Agente, detección local.** `listening_services()` en `HostNetwork` y su parser por sistema, con pruebas de muestra; los servicios aparecen en el estado local (`status`). Aquí ya existe el alcance «local» y no sale nada del equipo.
3. **Control plane y protocolo.** Migración `0010`, `services` en `Report`, `services` en `/device/peers` y en el inventario, OpenAPI y su prueba de contrato, pruebas de migración (versiones `1..10`) y de visibilidad por política. **Se despliega antes de activar la publicación en ningún agente.**
4. **Agente, publicación.** `[services]` en `agent.toml`, envío en el informe y lectura en `fetch_peer_metadata`.
5. **Aplicación de escritorio.** Lista de servicios en el detalle del par, `OpenService`, botón de copiar y el interruptor de publicación (decisión D1).
6. **Panel web.** Columna o detalle de servicios en el inventario.

## Pruebas de aceptación

- Con un servidor de prueba escuchando en `0.0.0.0:P`, el agente lo detecta; con uno en `127.0.0.1:P`, no.
- Publicación apagada: el informe no lleva `services` y un control plane antiguo lo acepta.
- Un par sin política hacia el dispositivo **no** ve sus servicios; con política, sí.
- `OpenService` rechaza un puerto que el par no publicó y un `deviceID` de otra red.
- La lista cae a vacía a los 120 s sin informe.
- Prueba de contrato OpenAPI y de migración al día; la prueba de la superficie del servicio pasa a 13 métodos.

## Qué no se hace en esta versión

Servicios UDP; sondeo de salud o de esquema; nombres DNS por servicio; edición de etiquetas desde la aplicación; generar políticas o reglas de cortafuegos automáticamente (el «alcance C» de la discusión: descartado por ahora, porque el ACL no limita puertos y automatizar el acceso sin criterio amplía la exposición).
