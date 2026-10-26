# Agente: subredes, DNS y control local

El agente aplica únicamente rutas y DNS de una configuración firmada vigente. Declarar una ruta no la autoriza: un administrador debe aprobarla y elegir explícitamente su valor de SNAT. No hay nodo de salida ni tránsito entre pares. Véanse [ADR-007](../../docs/architecture/ADR-007-subredes-dns-privilegios.md) y [configuración v2](../../docs/protocol/signed-config-v2.md).

La integración con el host se delimita en `src/platform`: `TunDevice` (IP puro), `HostNetwork` (enlace, rutas, DNS, SNAT, forwarding y candidatos), `LocalIpc` (transporte y credenciales), `PrivateFiles` (secretos y persistencia duradera) y `Lifecycle` (parada). Linux conserva su backend. macOS dispone de utun y Windows de Wintun como clientes nativos. `advertise_routes` se rechaza en plataformas sin capacidad de servir subredes; un `advertised` firmado se informa como `routes: unsupported` y `forwarding_ok: false`, sin instalar SNAT.

## Configuración local

Además de las claves habituales del TOML:

```toml
interface = "atenea0"
ipc_socket = "/run/atenea/control.sock"
status_socket = "/run/atenea/status.sock"
# Sustituir por el GID numérico del grupo local autorizado a consultar estado.
status_gid = 1000
advertise_routes = ["192.168.50.0/24"]
handshake_cookie_threshold_per_second = 100
noise_key_file = "credential:noise"
identity_key_file = "credential:identity"
```

`advertise_routes` es opcional (vacío por defecto), admite hasta ocho prefijos privados y requiere `[sync]` para declararlos al control plane. Se declara al arrancar, después de aplicar una configuración o de `reload`, y cada cinco minutos; los fallos no se registran con cuerpos ni claves y se cuentan en `sync.route_announce_errors`. Recargar una lista vacía retira una declaración anterior. `reload` vuelve a leer esta lista local y fuerza la lectura de la configuración firmada; el resto de cambios locales requiere reiniciar el servicio.

`handshake_cookie_threshold_per_second` es un entero positivo de 32 bits (por defecto 100; cero se rechaza), configurable también con `atenea agent-config --handshake-cookie-threshold-per-second N`. Cuenta INIT wire-v2 con longitud y MAC1 válidos en ventanas monotónicas de un segundo; al superar el umbral se exige MAC2 antes de DH. La cookie no autentica al par ni sustituye las cuotas DH existentes; su respuesta tampoco confirma ni marca sano un camino. Véase [wire-v2](../../docs/protocol/wire-v2.md). Cambiar el umbral requiere reiniciar.

Las respuestas de cookie reservan por par configurado 2/s y ráfaga4, compartidos entre sus endpoints/candidatos exactos (IP y puerto) y contextos relay. Desconocidos usan otros cubos: 20/s y ráfaga20 por IP/IPv6 /64, global100/s y ráfaga100, tabla≤4096. Agotar estos últimos no consume las reservas. Una coincidencia de origen no autentica al par y las reservas no garantizan disponibilidad del enlace ni de la CPU.

El objeto `counters` del estado contiene `init_challenged` (INIT que requiere cookie por carga), `tx_cookie_reply` (emitida), `cookie_reply_ratelimited` (omitida por cupo/capacidad), desglosado en `cookie_reply_ratelimited_peer` y `cookie_reply_ratelimited_unknown`, `rx_cookie_reply` (válida aceptada), `rx_cookie_reply_invalid` (descartada por índice/contexto/AEAD o estructura inválida con cabecera v2/COOKIE_REPLY) y `mac2_accepted` (MAC2 vigente/anterior válido que supera la puerta pre-DH, haya o no carga; no implica autorización Noise). Los contadores no incluyen secretos; las claves omitidas valen cero. IPC sigue sirviendo una única instantánea serializada, sin añadir cálculos por consulta.

`credential:nombre` busca el fichero dentro de `$CREDENTIALS_DIRECTORY` de systemd (`LoadCredential` o `LoadCredentialEncrypted`). El nombre no puede contener `/` ni `..`; falta de directorio, clave ilegible o permisos de grupo/otros producen un error. También se admiten los ficheros privados 0600 habituales. La interfaz admite letras ASCII, números y `_`, hasta 15 caracteres: se usa como identificador de la tabla nftables.

## Transporte, estado y recuperación

`sync.url` y `coordinator.url` exigen `https`; `http` solo se admite hacia loopback o con `insecure_http = true` en esa sección (laboratorio; `atenea enroll` y `agent-config` aceptan `--insecure-http` con el mismo alcance). `sync` solo sustituye `config_file` con una configuración que verifica (firma, serial, versión y reloj) contra el estado vigente.

El estado anti-rollback (`state.bin`) se escribe en un temporal exclusivo y el directorio de estado se crea 0700; se lee sin seguir enlaces simbólicos y solo si es de root o del dueño del directorio. La hora máxima observada avanza solo hasta el fin del lease de la última configuración verificada, y el estado comprometido nunca retrocede en disco. Tras un reloj que estuvo adelantado, con el agente parado:

```sh
atenea-agent reset-clock --config /etc/atenea/agent.toml
```

Solo borra la hora máxima observada; conserva versión, serial y anclas.

Las concesiones de relay del coordinator se aceptan solo para pares de la configuración, como mucho cuatro relays por concesión y cuatro resoluciones en vuelo; cada dirección se resuelve con plazo y el cliente conecta a la IP verificada (se descartan loopback, enlace local, multicast, la red del túnel y las subredes servidas). Cada entrada lleva `relay_id` y su token de audiencia propio: no se comparte un token entre relays. Solo se admite wire-v2/relay-v2 (TLS ALPN `atenea-relay/2`), sin fallback v1. Las cookies INIT se ligan al endpoint directo o al contexto `(relay_id, red, emisor, receptor)`; UDP y TLS del mismo relay comparten ese contexto. El mensaje 1 conserva sus cupos pre-DH por par configurado y por origen desconocido.

**Renovación y presencia (aplicado el 2026-10-26).** Un relay listo ya no impide
pedir credenciales nuevas. Para tokens de 300 s, el dispositivo de ID menor
solicita la renovación cuando quedan 60–65 s; el otro toma el relevo al quedar
30–35 s si no recibe la concesión inversa. El margen nunca supera un tercio de
la vida del token. El jitter de esta elección procede del token; los reintentos
HTTP usan backoff de 5/10/20 s más 0–510 ms de jitter, con una única petición en
vuelo por par. El agente limita **todos los intentos**, incluidos los fallidos,
a dos por par y diez por dispositivo en cada ventana de 60 s, sin ampliar las
cuotas del coordinator. Un `Retry-After` entero de 0 a 60 s puede retrasar más
el intento; otros formatos se ignoran.

El token nuevo solo pasa a ser vigente tras un ALLOC_OK en la misma audiencia
y vinculación. El re-ALLOC conserva `alloc_id`, worker, contadores y camino;
no crea dos asignaciones simultáneas. Un rechazo, token inválido o respuesta
perdida no prolonga la autorización anterior: se conserva hasta su `not_after`
o el vencimiento anterior de la asignación. Las renovaciones ALLOC fallidas
se reintentan tras cinco segundos; retirar la audiencia sigue cerrándola.
No se usan los 30 s de tolerancia del servidor para prolongar el token local.
Una caída del transporte sigue causando Down; no se promete continuidad
entre relays distintos ni disponibilidad cuando se agotan las cuotas.

El objeto IPC `counters` expone cuatro contadores de la **obtención HTTP de
concesiones para una asignación ya lista**, separados de `relay_requests`
(peticiones de fallback del engine):

- `relay_renewal_attempts`: peticiones anticipadas despachadas por la agenda.
- `relay_renewal_ok`: respuestas HTTP con una concesión relay-v2 válida.
- `relay_renewal_failed`: peticiones fallidas, incluidos 429, errores de red o
  respuestas inválidas. Un reintento posterior es otro intento.
- `relay_renewal_throttled`: renovaciones pendientes aplazadas por backoff o
  presupuesto local; una vez por episodio de espera, no por tick de 250 ms.

No se cuentan las peticiones iniciales ni se multiplican los intentos por los
tokens de cada audiencia. `relay_renewal_ok` no equivale a ALLOC_OK ni acredita
tráfico E2E: el estado y `last_error` de la asignación siguen mostrando el
resultado del refresco en el relay. Los cuatro valores aparecen incluso a cero.

La presencia mantiene su refresco de 20 s, pero conserva una única actualización
pendiente de candidatos/configuración, incluso cuando la cantidad de candidatos
no cambia. Los cambios se coalescen y se publica el último snapshot al permitirlo
la cuota: separación mínima de 2 s más jitter de 0–510 ms desde la respuesta
anterior. Los fallos reintentan con backoff de 2/4/8 s más ese jitter, respetando
el `Retry-After` entero acotado anterior. Un 429 ya no obliga a esperar el tick
de 20 s. HTTP conserva su timeout de 15 s; estos plazos de agenda no son un SLA
de reconexión ni sustituyen autorización, cookie, frescura, `init_floor` o lease.

## Aplicación por enlace

Linux necesita `ip`, `nft` y, para DNS dividido, `resolvectl` con systemd-resolved activo, invocados por ruta absoluta (`/usr/sbin/ip`, `/usr/sbin/nft`, `/usr/bin/resolvectl`; sustituibles con `ATENEA_IP`, `ATENEA_NFT` y `ATENEA_RESOLVECTL`) y con entorno vacío; el ejecutable y su directorio deben ser de root o del proceso y sin escritura de grupo/otros. El servicio necesita `CAP_NET_ADMIN`; no necesita `CAP_NET_RAW`. No modifica `/etc/resolv.conf` ni sysctls globales. El administrador debe habilitar explícitamente el forwarding de las familias servidas; `forwarding_ok` lo lee de nuevo en cada consulta de estado y en cada informe, sin esperar una nueva configuración ni intentar repararlo. Es nulo cuando la configuración aplicada no anuncia subredes.

Cada ruta de un par se instala con `ip route add PREFIJO dev INTERFAZ`. Se informa `conflict` si solapa una ruta no predeterminada de otra interfaz, cubre un endpoint del plano de control o no se puede comprobar/instalar con seguridad. La ruta predeterminada nunca se modifica. Reaplicar conserva las rutas sin cambios y elimina únicamente las que el agente instaló y han desaparecido. Al cerrar la TUN, el kernel retira sus rutas.

Los endpoints y candidatos de pares, y los destinos de relay concedidos por el coordinator, dentro de cualquier ruta firmada o prefijo del túnel se descartan antes de sondear/negociar/conectar: alcanzarlos por la propia VPN crearía un bucle, pero no bloquean instalar la ruta. El router no anuncia sus direcciones dentro de `advertised` ni de los prefijos del túnel. Los endpoints del plano de control (sync, coordinator, STUN) siguen protegidos y sí producen `conflict`.

El servicio arranca aunque no pueda resolver los endpoints del control plane, coordinator o STUN. Mientras falte esa información, las rutas de subred quedan `pending` con motivo `endpoints_unresolved`; la TUN y las direcciones de la red del túnel siguen operativas. Reintenta cada diez segundos fuera del bucle de E/S, con una única resolución en vuelo (sin acumular hilos). Si tarda más de diez segundos, retira las rutas de subred hasta completar la resolución. Cambiar las IP resueltas vuelve a comprobar los conflictos.

El DNS configura solo servidores por enlace y dominios `~lab.internal`, nunca `~.`. Se desactiva el uso del enlace como ruta DNS predeterminada. Un sistema sin resolved informa `unsupported`; una configuración sin servidores o dominios informa `none`. Los cambios de resolved desaparecen con el enlace.

SNAT afecta exclusivamente al tráfico que entra por la TUN y sale hacia cada subred marcada `snat = true` en la configuración firmada. Las reglas se reemplazan en una transacción nftables en la tabla exclusiva `inet atenea_INTERFAZ`. Sin SNAT, la LAN necesita una ruta de retorno a la red del túnel. El agente elimina su tabla al arrancar y cerrar; tras una caída:

```sh
atenea-agent cleanup --config /etc/atenea/agent.toml
```

La tabla ausente es un éxito; un error de permisos o de nft es un error, no se oculta. Es adecuado para `ExecStopPost`.

La parada por `atenea down`, SIGTERM/SIGINT (systemd/launchd) o SCM de Windows
completa primero la limpieza de red, el vaciado del estado y la retirada del IPC.
Después, ambas entradas del agente limitan a dos segundos la espera de cierre del
runtime: un long-poll HTTP síncrono pendiente no retiene el proceso hasta su timeout.
Este presupuesto no interrumpe la limpieza ni modifica los plazos HTTP durante el uso normal.

## Sockets y estado

El socket de control tiene modo 0600 y comprueba las credenciales del proceso remoto (root o propietario del socket). Atiende `status`, `stop`, `reload` y `cleanup`. El de estado es opcional, modo 0660 y grupo `status_gid`; únicamente acepta `status`. El directorio padre también debe limitar el acceso al grupo autorizado. Un comando enviado al socket equivocado devuelve un error.

Los listeners atienden clientes fuera del bucle TUN/UDP: como máximo 16 conexiones simultáneas por socket, órdenes de hasta 32 bytes y un segundo para toda la conexión (lectura y respuesta). El estado es una instantánea serializada renovada una vez por segundo, no un cálculo por cliente. La saturación del socket de estado no ocupa las plazas del control. Los paquetes rechazados como malformados por el backend TUN se descartan y cuentan en `tx_malformed`; los errores de E/S del dispositivo siguen siendo fatales.

```sh
atenea status --socket /run/atenea/status.sock
atenea reload --socket /run/atenea/control.sock
atenea down --socket /run/atenea/control.sock
```

El estado IPC y los informes autenticados al control plane incluyen `routes` (`prefix`, `state` y motivo opcional), `dns` (`applied`, `unsupported`, `none`) y `forwarding_ok` (nulo si no sirve subredes). El estado es declarado por el agente, no una prueba de conectividad de la LAN.

## Pruebas sin privilegios

La política de `netconf` usa un backend `HostNetwork` en memoria para probar rutas, diferencias, conflictos, DNS y capacidades en cualquier plataforma, sin procesos ni privilegios. Las pruebas adicionales del backend Linux inyectan las rutas de `ip`, `nft`, `resolvectl` y del árbol de sysctls; usan ejecutables simulados y ficheros temporales y no cambian la red del anfitrión. La aceptación del laboratorio Linux, nftables y resolved reales corresponde al propietario y no queda sustituida por estas pruebas.

La regresión `udp_timeout_falls_back_to_tls_custom_ca_queue_and_renewal` usa sockets locales y respuestas TLS fragmentadas. Espera una notificación del servidor tras escribir la respuesta de renovación, independiente de los eventos RTT; mantiene el intervalo normal de heartbeat para no mezclar esa comprobación con las pruebas de caducidad aceleradas.

Las regresiones `token_refresh_keeps_allocation_and_expiry_emits_down` y
`rejected_refresh_keeps_old_deadline_and_fails_closed` adelantan el reloj Unix,
no los temporizadores de transporte: mantienen los plazos normales de retry y
heartbeat. Retienen deliberadamente ALLOC_OK/ERR con un RTT inicial ya encolado
para demostrar que ese RTT no confirma la renovación. Solo avanzan hasta el
deadline tras observar que el cliente procesó la confirmación o el rechazo,
no solo que el servidor escribió la respuesta. Exigen Rx sin Down con token
vigente y Down por `token_expired` al alcanzar el deadline efectivo.

`real_coordinator_and_relay_renew_before_expiry_without_down` usa el router real
del coordinator en HTTP local (configuraciones y peticiones firmadas), su
`with_clock`, dos clientes y la biblioteca real del relay con
`Relay::bind_with_clock` (Unix en milisegundos). Comparte reloj Unix inyectado,
mantiene presencia y ALLOC periódicos y atraviesa t=300 hasta t=340 con tráfico
bidireccional, una única petición anticipada y concesión inversa, sin Down/Ready
adicional ni cambio de `alloc_id`. Solo el destino loopback es específico del
laboratorio; no sustituye el ensayo NAT ni la comprobación de saneado de destinos.
`renewal_counters_count_attempt_results_and_deferrals_exactly_in_status` comprueba
incrementos exactos y su publicación en `counters`, sin alterar los del engine.
Coordinator, relay y axum son dependencias exclusivamente de pruebas Unix del
agente (el relay usa un socket de estado Unix); no entran en builds de producción
ni en las pruebas Windows.

La preparación admite como máximo tres aperturas iniciales normales tras
`udp_timeout` y registra `initial_open_attempts`; después del primer Ready no
tolera ninguna caída ni recreación. El relay local reserva UDP en `[::]:0` y
anuncia IPv4 loopback: evita que macOS asigne al hub dual-stack el mismo puerto
efímero que a un listener IPv4 local, sin alterar los plazos del cliente.
`initial_udp_timeout_is_retried_by_engine_and_recovers` descarta deliberadamente
los tres COOKIE_REQ iniciales, comprueba `udp_timeout` y verifica que el engine
respeta su intervalo y solicita de nuevo, hasta obtener Ready en la segunda
apertura. No depende de contención de CPU para provocar la pérdida.

Los fixtures del motor fijan solo la fuente del jitter de sondeo a 1000 ms;
claves, índices y efímeras conservan su RNG criptográfico. El motor de producción
sigue usando `crypto::random_u32` para el intervalo de 750–1250 ms. Con ticks de
250 ms, intervalos de 1001–1250 ms se ejecutan a los 1250 ms: siete reenvíos a dos
candidatos antes de t=10 son catorce datagramas legítimos, no una pérdida de ráfagas.
La regresión de cadencia central exige nueve ráfagas exactas de reenvío (18
datagramas, más los dos iniciales), el mismo INIT y el backoff tras fallar.
`probe_jitter_extremes_preserve_exact_tick_cadence_window_and_budget` comprueba
también los extremos y la cuantización, sin ampliar la ventana de 10 s ni el
presupuesto de doce envíos por candidato.

## macOS

Solo cliente: utun normaliza la cabecera de familia a IP puro; `ifconfig`, `route` y `netstat` aplican direcciones/MTU/rutas con la política común de conflictos. `interface = "utun"` elige un índice libre, `utunN` solicita uno concreto; no usar `atenea0`. IPv6 desactiva DAD exclusivamente en la utun. El descubrimiento de candidatos parsea `ifconfig`, sin libc en el agente.

DNS dividido usa `/etc/resolver/<dominio>`, con marca de propiedad, sin dominio raíz ni cambios al DNS global. Solo retira ficheros propios al parar, arrancar o ejecutar `cleanup`. IPC, permisos privados y señales reutilizan los backends Unix. Las órdenes nativas y el directorio de resolutores son inyectables en pruebas sin privilegios.

El paquete sin firmar de `packaging/macos/build-pkg.sh` instala un demonio launchd root, pero no lo habilita sin configuración. Las rutas orientativas son `/Library/Application Support/Atenea` y `/var/run/atenea`; no cambian los valores del código. Véase [instalación macOS](../../docs/operations/instalacion-macos.md) para configuración, arranque, desinstalación, firma/notarización pendiente y alcance de CI. La prueba privilegiada solo se ejecuta en la VM desechable de GitHub, nunca en el Mac del propietario.

## Windows

Solo cliente: adaptador Wintun (tipo `Atenea`, alias `interface`, por ejemplo `atenea0`), direcciones/MTU/rutas mediante los cmdlets nativos NetTCPIP y DNS dividido mediante NRPT por dominio. No instala NAT ni forwarding, no sirve subredes, no modifica rutas predeterminadas ni DNS global. Wintun es la única excepción de controlador aceptada por el responsable el 2026-10-25; no se usa el protocolo WireGuard.

El ZIP sin firmar de `packaging/windows/build-zip.ps1` registra `AteneaAgent` como LocalSystem con arranque manual; no lo arranca hasta configurar. Los binarios y `wintun.dll` están en `%ProgramFiles%\Atenea`, los datos privados en `%ProgramData%\Atenea`. El instalador descarga Wintun 0.14.1 del sitio oficial con SHA256 fijado y conserva su licencia: no hay DLL versionada ni incluida en el ZIP. El servicio usa `atenea-agent run --service --config RUTA_ABSOLUTA`; `cleanup --config RUTA_ABSOLUTA` limpia residuos propios tras una caída.

Configurar `ipc_socket = '\\.\pipe\atenea-control'` y `status_socket = '\\.\pipe\atenea-status'` (literales TOML). Control: DACL SYSTEM/Administradores; estado: solo consultas, con `status_gid = 545` para BUILTIN\Users o `544` para Administradores; omitido limita a SYSTEM/Administradores. No son grupos Unix ni SIDs de grupos locales personalizados. Se rechazan clientes remotos y los ficheros privados usan DACL protegida, no permisos Unix emulados.

Véase [instalación Windows](../../docs/operations/instalacion-windows.md) para enrolamiento, configuración con rutas absolutas, arranque/parada SCM, limpieza, desinstalación, procedencia/hash/licencia de Wintun y aceptación real de CI. Pendientes: certificado de firma de código de Atenea y máquinas reales para la matriz de interoperabilidad; el trabajo definido en CI no se declara ejecutado desde este entorno.
