# ADR-013: Varias redes por dispositivo y unión por código (estilo ZeroTier)

- **Estado:** **Aceptado** por el responsable el 2026-10-28 con las decisiones de §6. Fases 1, 2 y 3 implementadas (Linux y macOS); la aceptación privilegiada de macOS pasó en la CI desechable (commit `b81af18`). La aplicación (fase 4) está implementada con los límites de §9; Windows sigue pendiente.
- **Relacionados:** ADR-005 (raíz y emisores), ADR-007 (rutas y DNS), ADR-008 (clientes), ADR-010 (L2), ADR-011 (aplicación de escritorio, solo lectura), ADR-012 (nombres).

## 1. Objetivo

Desde la aplicación de escritorio, pegar un **código de red**, pulsar «Unirse a una red» y quedar conectado a esa red del servidor; poder estar en **varias redes a la vez** y conectar o desconectar cada una, como en ZeroTier. Un mismo servidor (el VPS) aloja varias redes aisladas entre sí.

## 2. Qué existe hoy (comprobado en el código)

| Pieza | Estado |
| --- | --- |
| Servidor con varias redes | **Sí.** El control plane ya modela organizaciones → redes (`networks.id`, CIDR propios, `root_anchor` propio). El coordinator y el relay separan por `network_id` (nonces, cupos, `authorized_pair`, tokens de relay). |
| Unirse con un código | **Parcial.** El panel emite un `join` `{url, token, network_id, root_anchor}` por red (`POST /networks/{net}/enrollment-tokens`), pero el token es de **un solo uso** y el cliente lo consume con `atenea enroll` + `agent-config`; no hay un «código» único que pegar. |
| Unirse solo con el identificador de red | **No.** No existe ninguna petición de unión sin token. |
| Varias redes en un dispositivo | **No.** El agente es de **una red por instancia** (ADR-010 §2.4): un `agent.toml`, una interfaz, un directorio de estado, un par de sockets, un servicio `launchd` (`one.atenea.agent`) y una limpieza (`cleanup`) que asume una sola instancia de sistema. |
| Interfaz que cambia el sistema | **No.** La aplicación es de solo lectura por decisión (ADR-011): no puede crear interfaces, rutas ni claves. |

Conclusión: el servidor ya está casi listo; **lo que falta es el cliente** (varias instancias, un gestor con privilegios y la pantalla de unión) y una **forma de unión mejor que el token de un uso**.

## 3. Propuesta

### 3.1 Código de red

Un texto único y copiable que contiene todo lo que el cliente necesita: dirección del servidor, identificador de red y **huella de la raíz de confianza** (`root_anchor`), más una suma de comprobación. Ejemplo sintético: `atenea1-AB12…-7F`. Con la huella dentro no hay confianza a ciegas en la primera conexión (TOFU): el cliente fija la raíz que le dio quien le pasó el código por otro canal. Variante opcional con token de un uso para redes cerradas. Si falta el servidor o la huella, el cliente rechaza el código; no se completa nada de memoria.

### 3.2 Política de unión de cada red (campo nuevo en el servidor)

| Política | Efecto | Riesgo |
| --- | --- | --- |
| `cerrada` (por defecto) | Solo con un token emitido por un administrador (hoy). | Ninguno nuevo. |
| `con_solicitud` | Cualquiera con el código envía una solicitud: el dispositivo queda `pending` y un administrador la aprueba comparando la **huella del dispositivo** (como ahora). | Spam de solicitudes: tope de pendientes por red, límites por IP (ya existe `enroll_per_minute`) y caducidad de pendientes. |
| `abierta` | Aprobación automática. **No se ofrece en v1.** | Cualquiera con el código entra. |

El identificador de red **no es un secreto ni una credencial**: lo que autoriza es la aprobación del administrador.

### 3.3 Varias redes en el dispositivo

Una **instancia de agente por red** (reutiliza el agente actual, sin tocar el protocolo):

- Directorio propio por red: `…/networks/<id>/` con claves, estado, configuración firmada y `agent.toml`.
- **Claves por red** (par de claves nuevo en cada unión): el servidor no puede correlacionar un mismo equipo entre redes, y comprometer una red no compromete las demás.
- Interfaz propia (`utunN` en macOS; `atenea<N>` en Linux; un adaptador por red en Windows), sockets de control y de estado propios y un servicio por red (`one.atenea.agent.<id8>`).
- **Puerto UDP por instancia** (51820, 51821…): cada red necesita su puerto alcanzable para el camino directo. Alternativa a medio plazo: un solo proceso que multiplexe varias redes sobre un socket (el prólogo Noise ya separa redes), con refactor mayor del bucle principal; **no** en la primera entrega.
- **Cambios obligatorios en lo existente** (hoy asumen una sola instancia): la limpieza de resolutores DNS de macOS marca los ficheros como «de Atenea» sin distinguir red; el directorio `/var/run/atenea` y los nombres de socket son únicos; la etiqueta de `launchd` es única. Todo ello pasa a depender del identificador de red.
- **Conflictos entre redes:** CIDR solapados y dominios DNS repetidos. El cliente debe **rechazar la unión** (con el motivo) si el CIDR de la red nueva solapa con otra red unida o con una red local, antes de tocar nada.

### 3.4 Privilegios (decisión de seguridad principal)

Unirse, salir, conectar y desconectar **cambian el sistema** (claves root, interfaz, rutas, resolutores). Hoy la aplicación no puede y por diseño es de solo lectura.

| Opción | Cómo | Pros | Contras |
| --- | --- | --- | --- |
| **A. Petición de administrador por operación** | La app lanza el CLI con elevación del sistema (diálogo de contraseña en macOS, UAC en Windows, `pkexec` en Linux) para cada unión o salida. | Sin servicio con privilegios nuevo ni API local nueva; superficie mínima. | Pide contraseña cada vez; el interruptor de conectar/desconectar sería incómodo. |
| **B. Gestor residente** | Un servicio con privilegios con un socket local (solo el grupo de administradores, comprobando la credencial Unix del cliente) y una API **mínima** (`unir`, `salir`, `listar`, `conectar`, `desconectar`) que ejecuta el CLI; **no** ejecuta órdenes arbitrarias. | Experiencia ZeroTier/Tailscale: un clic, sin contraseñas. | Componente nuevo con privilegios: modelo de amenazas propio; cualquier proceso del grupo autorizado puede unir el equipo a una red; requiere pruebas y revisión de seguridad antes de usarse. |

**Recomendación:** entregar primero **A** (y el CLI, que es la base de ambas) y pasar a **B** solo si la fricción lo justifica, con su propio ADR. Una red hostil puede hacer que el equipo acepte rutas privadas y DNS para dominios concretos (ADR-007; nunca la ruta por defecto): por eso la pantalla de unión muestra el nombre de la red, el servidor y la huella de la raíz, y pide confirmación explícita.

### 3.5 Interfaz

- **Redes:** sección nueva con la lista de redes unidas (nombre, servidor, estado, interruptor de conexión) y «Unirse a una red…».
- **Dispositivos:** un selector de red; dentro de cada red siguen «Local», «Servidor» y «Pares».
- **Bandeja:** una línea por red con su punto de estado.
- Nombre de la red: el que viaja en el código; alias local editable.
- La aplicación pasa de solo lectura a poder **pedir** operaciones: se revisa ADR-011 (la lectura de estado sigue sin privilegios y sin órdenes de control).

## 4. Plan por fases

| Fase | Contenido | Tamaño | Prueba de salida |
| --- | --- | --- | --- |
| **0** | Aceptar este ADR (decisiones de §6). | S | ADR aceptado. |
| **1** | **Servidor:** código de red (generación en el panel con huella y QR), política `cerrada`/`con_solicitud`, endpoint de solicitud sin token con límites y tope de pendientes, caducidad de pendientes, auditoría, OpenAPI. Una migración. | M | Pruebas contra PostgreSQL: cada política, límites, caducidad, aislamiento entre redes y que un código manipulado se rechaza. |
| **2** | **CLI:** `atenea network join <código>`, `leave`, `list`, `up`, `down`; análisis del código con fuzz; claves por red; rechazo de CIDR solapados. Sin interfaz gráfica. | M | Laboratorio con dos redes en un mismo cliente: no se alcanzan entre sí, salir de una no toca la otra, revocar en una no afecta a la otra. |
| **3** | **Multiinstancia en el agente y empaquetado Linux/macOS:** directorios, interfaces, sockets, etiquetas y limpieza por red; `.pkg` y `.deb` (§8). **Windows aplazado** a una fase propia (servicio/adaptador por red y pruebas elevadas). | M–L | Pruebas de limpieza (un `cleanup` no borra recursos de otra red), reinicio con dos redes y desinstalación. |
| **4** | **Aplicación:** sección Redes, selector, diálogo de unión con confirmación y opción **A** de privilegios; bandeja por red. | M | Pruebas de interfaz y de accesibilidad; unión y salida reales en un Mac. |
| **5** | **Piloto:** crear una segunda red en el VPS, unir el Mac a las dos, medir el efecto en el soak (más sesiones, más puertos) y documentar. | S | Soak de 72 h con dos redes. |
| **6** (opcional) | Gestor residente (opción **B**) con ADR y revisión de seguridad propios. | L | Modelo de amenazas y pruebas negativas. |
| **7** (aplazado) | Multiplexado de varias redes en un proceso y un único puerto. | L | Comparación medida frente a una instancia por red. |

Cada fase deja `check.sh` y CI en verde y documentación con los límites reales; nada se despliega sin autorización explícita.

## 5. Riesgos y límites

- **Autoridad de la raíz por red.** Cada red necesita un certificado de emisor firmado por una raíz **fuera de línea** (ADR-005; hoy 30 días de validez). Con varias redes crece el trabajo de renovación manual. Reutilizar una misma raíz para varias redes parece posible (el control plane recibe la `root_anchor` al crear la red) pero **no está comprobado**; la renovación automática con raíz fuera de línea es un problema abierto.
- **L3 frente a L2.** Las redes virtuales de ZeroTier son de nivel 2 (Ethernet, difusión). Las de Atenea son de nivel 3. El modo L2 está diseñado (ADR-010) y **aplazado hasta el informe de la auditoría**; este plan no lo incluye.
- **Carga:** más redes por equipo implican más sesiones, más puertos y más sondeos; sin medir.
- **Un código filtrado** permite enviar solicitudes (no entrar): aprobar compara siempre la huella del dispositivo.
- **Privilegios:** la opción B añade superficie local; la opción A no la añade pero es menos cómoda.
- **Sin comprobar hoy:** coordinator y relay con dos redes reales en el mismo servidor (el código las separa, no se ha ejecutado), y el solapamiento de dos interfaces de Atenea en macOS.
- **Salida:** `leave` revoca el dispositivo antes de borrar claves (§11); solo `--offline` deja el registro remoto pendiente de revocación administrativa.

## 6. Decisiones del responsable (2026-10-28)

1. **Formato del código:** **todo en uno**, con la huella de la raíz de confianza dentro.
2. **Política de unión:** `cerrada` por defecto y `con_solicitud` disponible en v1. *Esta decisión no la dio el responsable de forma explícita: se deduce de que la fase 1 incluye `con_solicitud`; se mantiene la recomendación de `cerrada` por defecto y `abierta` no se ofrece.*
3. **Privilegios:** **opción A** (elevación por operación). La opción B queda para un ADR posterior.
4. **Una instancia de agente por red**; el multiplexado queda aplazado.
5. **Claves por red.**
6. **L2:** fuera de alcance; espera a la auditoría y a la fase 10 (ADR-010).

**Desviación de la fase 1 respecto a §4:** el QR del panel se aplaza porque exige una dependencia nueva de generación de QR (inventario y revisión de suministro); el panel muestra el código, la huella y un botón de copiar.

## 7. Estado de la fase 2 (CLI)

**Implementada** (CLI Linux/macOS; Windows devuelve «no soportado»). `atenea network join|list|up|down|leave` con raíz configurable, consentimiento explícito, claves nuevas por red y ancla fijada desde el código; instancia, interfaz, puerto UDP y sockets propios. El objetivo `join_code_parse` tiene corpus sintético y el laboratorio `two-networks.sh` demuestra claves distintas, conectividad simultánea, aislamiento, salida de A sin afectar B, revocación de B sin afectar A, código manipulado y red solapada. La guarda `network_overlap` evalúa la primera configuración verificada (el CIDR no viaja en el código): conflicto solo con una ruta ajena igual o más específica que un prefijo de `network_prefixes`, nunca con rutas más anchas; ignora las rutas de host de las propias `tunnel_addrs` que macOS lista en `lo0`. Hay pruebas con fixtures reales anonimizados de `netstat`/`ip -j route`, y se ejecutó además su lógica contra la tabla de rutas real de un Mac (sin privilegios y sin tocar su agente): la reaplicación de la propia configuración no se marca y el mismo CIDR desde otra interfaz sí. **El rechazo por solape con interfaces reales de macOS sigue sin ejercitarse** (el escenario de la CI usa CIDR distintos). Los servicios y la limpieza DNS multiinstancia están en la fase 3 (§8); `leave` no revoca en el servidor.

Detalle operativo: [redes-multiples-cli.md](../operations/redes-multiples-cli.md). Instalación base: [Linux](../operations/instalacion-linux.md) y [macOS](../operations/instalacion-macos.md). Protocolo: [join-code-v1](../protocol/join-code-v1.md).

## 8. Estado de la fase 3 (servicios Linux y macOS)

Implementados: plantilla systemd `atenea-network@<UUID>.service` con endurecimiento igual a legacy y lanzador validado por UUID, activación persistente desde `network up`, `down` deshabilita y para, drop-in para raíces personalizadas seguras; `.deb` instala la plantilla, actualiza reiniciando solo las instancias activas y desinstala sin borrar identidades. CLI `--backend auto|process|system`, conserva el modo proceso en contenedores sin systemd. `join`/`leave` y la asignación usan bloqueo exclusivo del directorio raíz (también lo respetan `up`/`down`), con prueba concurrente.

En macOS: plist generado por el CLI como root:wheel 0644, etiqueta `one.atenea.network.<id8>`, `KeepAlive/PathState`, launcher con UUID y raíz estrictamente validados; `bootstrap/bootout` y `enable/disable` por red. Resolutores marcados por `resolver_owner` (id8), con preflight `dns_domain_conflict` antes de aplicar cambios. La marca antigua sin red pertenece únicamente a legacy. El empaquetado conserva la instancia heredada y las identidades locales al desinstalar.

Aceptación privilegiada de macOS **superada en la CI desechable** (commit `b81af18`, nunca ejecutada en el Mac del piloto). `tests/macos/smoke.py` ejercita la instancia heredada más dos redes por `launchd` con `utun` reales: Noise y ping IPv4/IPv6 en cada una, resolutores por red, `down`/`up` independientes sin afectar a las otras, `postinstall` con una red deshabilitada (no se vuelve a conectar), desinstalación conservando identidades, y plists y resolutores ajenos intactos. **No ejerce** `network_overlap` ni `dns_domain_conflict` en macOS (cubiertos por pruebas unitarias y, el solape, por el laboratorio de Linux). La primera ejecución en CI falló tres veces: dos errores del propio escenario de prueba (unas rutas del v2 que solapaban con la red del túnel, que `atenea issue` rechazaba, y el formato `disabled` de `launchctl print-disabled`) y **un fallo del producto**: `postinstall` hacía `launchctl bootstrap` justo después de `bootout`, que es asíncrono, y fallaba con «5: Input/output error» si el proceso anterior aún no había salido; ahora espera a que el servicio quede descargado. El escenario Debian incluye legacy + dos redes, TCP real, independencia de PIDs/recursos, `dpkg -i`, `daemon-reexec` y remove/purge conservando identidades; la modalidad offline sobre `atenea-infra:local` omite explícitamente DNS dividido con systemd-resolved y shellcheck (no están en esa imagen).

**Aplazado:** Windows (el CLI sigue devolviendo «no soportado»; servicio y Wintun por red requieren fase propia con pruebas elevadas), aplicación (fase 4), despliegue y piloto. Sin dependencias nuevas. Guía: [redes múltiples](../operations/redes-multiples-cli.md).

## 9. Estado de la fase 4 (aplicación)

**Implementada**, con la opción A de privilegios. La sección «Redes» lista las redes unidas (estado, servidor, motivo de un conflicto), conecta y desconecta cada una, sale de una red con confirmación y permite **unirse pegando un código en dos pasos**: «Revisar código» muestra nombre, servidor, id, huella de la raíz y tipo de código sin cambiar nada, y solo «Unirme a esta red» pide la autorización del sistema. En la cabecera hay un selector de red (la lista de dispositivos y el diagnóstico muestran la elegida; sin elección guardada y con la predeterminada caída se enseña la primera red) y la bandeja muestra una línea por red que responde. ADR-011 §2 queda revisado: la app deja de ser de solo lectura; sus cuatro órdenes nuevas están acotadas y validadas en Rust.

Dos piezas nuevas del CLI sostienen la app: `atenea network inspect <código> [--json]` (decodifica sin root, sin disco y sin red; nunca imprime el token) y `join --status-gid <gid>` (el socket de estado de la red nueva es legible por ese grupo; sin él, `list` sin root muestra `desconocida`). La app une con el grupo primario del usuario.

**Sin comprobar:** la elevación real (diálogo de administrador en macOS, `pkexec` en Linux) y una unión completa desde la app: las pruebas llegan hasta la orden construida. Se ejercitó con el CLI y la app reales una red sintética (lista, `inspect`, estado) sin privilegios. No cubre Windows, el menú de la bandeja con varias redes pulsado con el ratón (la reconstrucción del menú al cambiar el conjunto de redes no se ha visto en pantalla) ni la accesibilidad de la pantalla de confirmación con `axe` (solo la lista). La opción B (gestor residente) sigue sin decidirse.

### Prueba real en el Mac del piloto (2026-10-28)

`v0.1.3` se publicó (commit `d0a7ce2`, `verify.sh` PASS) y se desplegó en el VPS con copia verificada previa (`ok=27 failed=0`, migraciones 1–7). Se creó una segunda red `red-prueba` (`10.88.0.0/24`) con política `con_solicitud` y el agente del Mac pasó a 0.1.3. **Unirse desde la app funcionó con el diálogo de administrador real de macOS**: solicitud con la huella del dispositivo, aprobada comparando esa huella, y la red quedó `conectada` **a la vez que la del piloto** (dos `utun`, cada uno con su dirección y su ruta; el piloto siguió con sus pares en `direct` y sin errores). Es la primera ejecución real de la guarda de solapes con dos redes en un Mac (CIDR distintos: no se ejerció un conflicto).

Defectos que la prueba sacó y que **`v0.1.3` contiene**: (1) en macOS `network join` asignaba `atenea0` y el agente solo acepta `utun`/`utunN` (el servicio se reiniciaba sin parar); (2) `postinstall` dejaba `/Library/Application Support/Atenea` en `0700` y `network list` sin root fallaba, de modo que la app no veía ninguna red. Corregidos en `cc5c860` (aún sin publicar: requiere una versión nueva); la prueba usó el CLI compilado de ese commit junto al agente firmado de 0.1.3. La CI de macOS no los detectó porque su escenario escribe sus propias configuraciones y no pasa por `join` ni por una lectura sin root.

**Sin comprobar:** el flujo completo con los binarios publicados (hace falta v0.1.4); `pkexec` en Linux; que dos redes conecten **con pares** a la vez (`red-prueba` solo tenía este dispositivo); un conflicto real de CIDR; el efecto en el soak; el menú de la bandeja con dos redes pulsado con el ratón; `leave` no revoca (hubo que revocar a mano el dispositivo de la primera unión).

## 10. Migración de la instancia heredada (decisión del responsable, 2026-10-28)

**Decisión.** La instancia heredada (un único `agent.toml`: `/etc/atenea/agent.toml` en Linux, `/Library/Application Support/Atenea/agent.toml` en macOS; servicio `atenea-agent.service` / `one.atenea.agent`; sockets `<run>/control.sock` y `<run>/status.sock`) deja de ser un caso especial: **una red = una instancia del modelo de redes**. Sin redes unidas el equipo no está conectado a nada, y la red del piloto aparece en «Redes» como cualquier otra (Conectar, Desconectar, Salir). El concepto «Predeterminada» desaparece de la aplicación.

**Mecanismo.** `sudo atenea network migrate-legacy [--name NOMBRE] [--yes]` importa la heredada: valida todo sin tocar nada, pide confirmación (red, servidor, huella de la raíz y aviso de que **se para la instancia heredada y el túnel se corta unos segundos**), la para y limpia con su gestor, **mueve** las claves, `config.bin` y `state` a `<raíz>/<uuid>/device/`, reescribe un `agent.toml` con rutas y sockets por red (`<run>/<id8>/`, `resolver_owner = <id8>` en macOS; mismo puerto UDP, interfaz y `status_gid`), escribe `network.json` (`join_status = aprobada`), deja un respaldo del `agent.toml` heredado en `<raíz>/<uuid>/legacy-agent.toml.bak` (0600) y arranca la red con `network up` (servicio por red). Las claves **no se copian**: se mueven, de modo que no quedan duplicadas fuera del directorio nuevo. `atenea network legacy [--json]` (sin root) solo informa de si hay una heredada pendiente (`{"present": bool}`); la aplicación lo usa para ofrecer la importación.

**Alternativas.**

- **A. Mostrar la heredada en «Redes» sin migrarla** — *descartada* por el responsable: mantiene dos modelos (sockets, servicios, rutas de configuración y «Predeterminada») en la CLI, la aplicación, el empaquetado y los scripts externos, y la acción «Salir» no tendría un significado único.
- **B. Copiar en lugar de mover** — descartada: duplicaría claves privadas y dejaría una identidad activa en dos sitios.
- **C. Migrar sin parar la heredada** — imposible: comparten interfaz, puerto UDP, resolutor y rutas.

**Riesgos y mitigaciones.**

- *Corte del túnel* durante la parada y el arranque (segundos): se avisa en la confirmación; es la única operación disruptiva y exige `--yes` o confirmación interactiva.
- *Fallo a mitad*: si algo falla **después** de parar la heredada, se restauran los ficheros movidos, se borra el directorio de red a medias y se reinicia la heredada; el error se informa. Si el fallo es del arranque de la red nueva, se deshace igual (el servicio por red deja de arrancar, la heredada vuelve).
- *Id corto repetido* (últimos 8 hex del UUID) con una red existente: se rechaza antes de tocar nada, porque los sockets y las etiquetas se derivan de él.
- *Idempotencia*: si ya hay una red migrada con ese id, error claro sin cambios; sin `agent.toml` heredado no hay nada que migrar.
- *Rearranque espontáneo de la heredada*: sin `agent.toml` heredado la unidad systemd (`ConditionPathExists`) y el `PathState` de launchd impiden arrancarla; además se la deshabilita (`disable`).

**Reversibilidad.** Parcial y manual: el respaldo `legacy-agent.toml.bak` conserva la configuración; revertir es `network down`, mover `device/` a la ruta heredada, restaurar el `agent.toml` y habilitar el servicio heredado (procedimiento en [redes-multiples-cli.md](../operations/redes-multiples-cli.md)). No hay reversión automática tras terminar con éxito.

**Límites.** Solo Linux y macOS (Windows: «no soportado»). Herramientas externas que leían `/var/run/atenea/status.sock` (scripts de soak, `atenea status --socket`, `ATENEA_STATUS_SOCKET` de la aplicación) deben apuntar a `<run>/<id8>/status.sock`. Un host que nunca migra sigue funcionando exactamente igual. **Sin comprobar** en un Mac real (solo pruebas unitarias y el ensayo Linux con systemd real).

## 11. Salida con revocación del propio dispositivo (2026-10-28)

**Decisión.** `POST /api/v1/device/leave`, cuerpo vacío, usa exactamente `api_auth` como config/peers/status: firma Ed25519 del método, ruta y query, cuerpo, identificador, timestamp y nonce; tolerancia de reloj de 60 segundos, nonce de un solo uso y límite compartido `device_per_minute`. No acepta identificadores de destino ni red: el destino es exclusivamente el dispositivo autenticado y su red almacenada. Se limita el cuerpo a 1024 bytes y cualquier cuerpo no vacío se rechaza.

**Estados e idempotencia.** Se verifica la clave guardada antes de examinar el estado. El mismo mecanismo criptográfico admite `pending`: no se exige aprobación para retirar una solicitud propia. `approved` y `pending` pasan a `revoked`; un dispositivo ya revocado obtiene 200 sin reescribir la fecha ni duplicar auditoría, siempre con firma y nonce nuevos. Dispositivos desconocidos, claves ajenas, firmas inválidas y timestamps fuera de ventana reciben el mismo 401 `invalid_signature`; los replays y límites conservan los errores uniformes del mecanismo existente. Los errores usan el sobre JSON del API; no se informa de existencia a quien no demuestra posesión.

**Atomicidad y efectos.** La transacción bloquea primero la red y comparte la revocación administrativa: marca revocado, libera asignaciones IP y rechaza rutas (sin usuario decisor en la salida autónoma). Auditoría con actor `device`, actor y objeto iguales al dispositivo, acción `device.leave`; la revocación administrativa conserva `user`/`device.revoke`. Tras el commit se notifica al panel. El dispositivo deja de recibir configuraciones; no se invalida instantáneamente una configuración ya emitida (caducidad existente).

**CLI y aplicación.** Tras confirmar, `network leave` llama al servidor con las claves de esa red antes de parar o borrar nada. Fallo de transporte o rechazo conserva todo y termina con error claro. Solo `--offline` omite el servidor, borra lo local y advierte que SIGUE registrado y que un administrador debe revocarlo por huella. Si el servidor revoca pero falla la limpieza, se informa que ya está revocado y se conservan los ficheros que no se hayan podido limpiar. La aplicación no ofrece salida offline y muestra el error del CLI.

**Amenazas.** Solo quien posee la clave privada puede salir: ya podía dejar de usarla o destruirla. Permitir su revocación añade únicamente denegación de servicio contra sí mismo, no elevación ni capacidad de afectar otra identidad o red. Firma ligada a método/ruta/cuerpo, ventana temporal y anti-replay impiden reutilizar una petición de configuración o de otra operación. No se añade ninguna primitiva ni dependencia criptográfica.
