# Laboratorio NAT independiente

No necesita binarios, claves, servicios ni crates de Atenea. Usa `ip netns`, veth,
puentes Linux, nftables/conntrack y una sonda UDP de biblioteca estándar Python.
Las direcciones son exclusivamente de documentación; no se conecta a Internet.

## Ejecución

Desde la raíz del repositorio, con Docker Desktop disponible:

```sh
tests/network-lab/nat/run.sh
```

Construye la imagen local `atenea-lab` (incluye `nftables`, `openssl` y `haproxy`, sin arrancar servicios),
ejecuta un contenedor desechable `--privileged` con el repositorio de solo lectura,
y afirma todos los casos. Imprime JSON con endpoints, `OK`/`FAIL`, versiones del
entorno y un recuento final; cualquier fallo da salida distinta de cero. No compila
Rust ni ejecuta las otras pruebas de red. No instala nada en macOS.

Uso interactivo reutilizable:

```sh
tests/network-lab/nat/run.sh shell
# Dentro del contenedor, desde /src:
source tests/network-lab/nat/lib.sh
trap topo_teardown EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
topo_setup home
topo_describe
ip netns exec inet python3 tests/network-lab/nat/natprobe.py reflect &
ip netns exec a python3 tests/network-lab/nat/natprobe.py mapping
python3 tests/network-lab/nat/natprobe.py pair
topo_move_a_to_cafe
ip netns exec a python3 tests/network-lab/nat/natprobe.py mapping
topo_move_a_home
topo_teardown
```

`lib.sh` no cambia opciones ni traps del shell que lo importa. El llamador debe
instalar la limpieza anterior. `topo_setup` limpia la topología anterior antes de
crear otra; `topo_teardown` es idempotente y elimina procesos, namespaces, veth,
puentes y, con los namespaces, sus reglas y conntrack. El verificador comprueba
la ausencia de namespaces y puentes tras **cada** caso. Los nombres fijos son
exclusivos del laboratorio: no importarlo en una máquina con namespaces `a`, `b`,
`inet`, `nat1`, `nat2` o `nat3` de otro uso. Se reservan también `nl-*` y `tmp-nl`.

## Contrato de direcciones

En todas las topologías `inet/eth0` tiene **198.51.100.10/24** y
**2001:db8:100::10/64**. Es el host para coordinator/STUN y control plane.
Las interfaces de los clientes siempre se llaman **eth0**. Los routers usan
`wan` hacia la red pública y `lan` hacia su LAN. Todos los namespaces tienen `lo`
activa. Los puentes viven en el namespace raíz del contenedor y no tienen IP.

| Topología | a/eth0 | b/eth0 | Routers y rutas |
| --- | --- | --- | --- |
| `flat` | 198.51.100.11/24; 2001:db8:100::11/64 | 198.51.100.12/24; 2001:db8:100::12/64 | Red pública directa, sin gateway necesario para inet |
| `home` | 192.168.1.10/24, GW 192.168.1.1 | 192.168.2.10/24, GW 192.168.2.1 | nat1/wan 198.51.100.1/24; nat2/wan 198.51.100.2/24 |
| `symmetric` | Igual que home | Igual que home | nat1 simétrico sintético; nat2 doméstico |
| `double` | Igual que home | Igual que home | nat1/wan 100.64.0.2/10, GW 100.64.0.1 en nat3/lan; nat3/wan 198.51.100.3/24 |
| `samelan_hairpin_off/on` | 192.168.1.10/24 | 192.168.1.11/24 | Ambos GW 192.168.1.1; nat1/wan 198.51.100.1/24 |
| `ipv6` | 2001:db8:a::10/64, GW 2001:db8:a::1 | 2001:db8:b::10/64, GW 2001:db8:b::1 | nat1/wan 2001:db8:100::1/64; nat2/wan 2001:db8:100::2/64; rutas explícitas entre prefijos y desde inet |
| Cambio de red en `home` | Café: 192.168.3.10/24, GW 192.168.3.1 | Sin cambios | nat3/wan 198.51.100.3/24; retorno a casa restaura 192.168.1.10 |

Cada natN IPv4 tiene `lan = 192.168.N.1/24`; nat3 en `double` tiene además
100.64.0.1/10. El tramo de doble NAT usa **100.64.0.0/10**, no 192.168.3.0/24.
IPv4 forwarding está activado en los NAT; en `ipv6` está activo IPv6 forwarding,
sin NAT. Los clientes en las topologías IPv4 no tienen IPv6 global salvo `flat`.
Las funciones de cambio de red solo admiten `home`: eliminan y sustituyen el
veth/eth0 de `a`, pero **no** recrean su namespace ni matan sus procesos. El
verificador compara la identidad del namespace antes y después, vuelve a consultar
el reflector desde el mismo puerto y comprueba también el retorno a casa.

## Sonda y afirmaciones

El reflector escucha UDP 3478 y 3479 tanto en IPv4 como IPv6. Devuelve el origen
observado como texto JSON `["ip", puerto]`; **no implementa STUN** y no debe
sustituir al coordinator al probar agentes reales. Solo responde al literal
`observe`. `mapping --ip IP --port 41000` consulta ambos puertos desde el **mismo
socket** y compara los endpoints. No clasifica el filtrado.

`pair --ip IP` arranca clientes en `a` y `b`, ligados a **42000** y **42001**.
Cada cliente obtiene su endpoint reflexivo; el arnés los intercambia por pipes
fuera de la red y ambos envían durante **cuatro segundos, cada ~1 segundo**, sin
cerrar ni cambiar el socket. Se exige recibir el marcador del **otro** cliente en ambos extremos; se
reintenta para tolerar que el primer paquete llegue antes de abrir conntrack.
No se exige que la dirección recibida sea idéntica al candidato: hairpin con SNAT
a la dirección LAN del gateway cambia ese origen. Esto prueba conectividad UDP,
no autenticación ni integridad criptográfica de Atenea.

| Topología | Mapeo medido a / b | Punch o hairpin medido | Endpoint reflexivo de a |
| --- | --- | --- | --- |
| flat | independiente / independiente; sin NAT, puerto intacto | Ambos reciben | 198.51.100.11 |
| home | independiente / independiente | Ambos reciben | 198.51.100.1 |
| symmetric | dependiente / independiente | Ninguno recibe; fallo esperado | 198.51.100.1 con puertos distintos |
| double | independiente / independiente | Ambos reciben | 198.51.100.3 |
| samelan_hairpin_off | independiente / independiente | Ninguno recibe; fallo esperado | 198.51.100.1 |
| samelan_hairpin_on | independiente / independiente | Ambos reciben | 198.51.100.1 |
| ipv6 | independiente / independiente; sin NAT | Ambos reciben | 2001:db8:a::10 (IPv6) |
| home → café → home | independiente | Reflexión responde tras cada cambio | 198.51.100.1 → .3 → .1, puerto 41000 |

La prueba IPv6 adicional abre un receptor **sin salida previa** en `b:43001`,
envía desde `inet` y afirma que no recibe. El mismo receptor tiene un control
positivo en `flat` (IPv6 sin firewall: sí recibe). Las consultas al reflector
prueban salida y respuestas permitidas; el punch posterior prueba apertura
simultánea de los cortafuegos con estado.

Entorno medido: **Linux 7.0.12-linuxkit**, nftables **1.0.6**, Python **3.11.2**,
en Docker Desktop. Validación integral final: **`Resultado: 0 fallos`, salida 0**,
40,57 segundos. La tabla anterior recoge los resultados de esa ejecución; las
dos expectativas negativas (simétrico y hairpin desactivado) fallaron como se esperaba.

Durante la validación, una consulta IPv6 inicial agotó el margen de dos segundos
(otra ejecución había completado todos los casos IPv6).
Los puentes ahora inundan multicast (`mcast_snooping 0`) para no depender de
convergencia MLD al resolver ND, y el reflector se consulta con margen de cinco
segundos. No se deduce del timeout un fallo permanente de IPv6 ni se acepta un
resultado parcial: la expectativa de entrada bloqueada y punch correcto se mantiene.

## Qué se emula y qué no

- Doméstico: masquerade Linux, conservación de puerto cuando es posible, mapeo
  independiente en los flujos medidos y filtrado por flujo conntrack. No es un
  NAT full-cone ni un NAT restringido solo por dirección; no clasifica ni emula
  todos los tipos de filtrado de RFC 5780.
  El router descarta UDP WAN nuevo dirigido a sí mismo (INPUT), sin responder:
  modela el firewall habitual de un router doméstico. No basta con filtrar
  FORWARD: con INPUT abierto, un datagrama que llega antes de la salida del otro
  cliente puede confirmar conntrack local y provocar remapeo del puerto.
  La primera ejecución detectó ese defecto del laboratorio: `home` y `double`
  daban mapeo independiente pero `[false, false]` en el punch. Tras corregir
  INPUT, un diagnóstico con tcpdump observó en `home` un paquete entrante
  `.2:42001 → .1:42000` **antes** de la primera salida `.1:42000 → .2:42001`,
  conservando después los puertos y recibiendo ambos clientes. `double` también
  recibió en ambos extremos con 42000/42001 conservados. No es una desviación
  aceptada ni se rebaja la expectativa: se corrige la política del router.
- Simétrico **sintético y deliberado**: `masquerade fully-random`, con rangos
  disjuntos según destino: UDP 3478 → 40000–44999, UDP 3479 → 45000–49999,
  resto UDP → 50000–59999. Evita la coincidencia aleatoria de puertos que podría
  falsear la clasificación o dar éxito accidental al punch. No pretende copiar
  un algoritmo de asignación de un router comercial ni toda dependencia de IP
  remota. La sonda a una sola IP pública solo mide dependencia del **puerto**
  destino. El punch falla porque el endpoint anunciado al reflector no es el
  usado hacia el par y el filtrado con estado impide la entrada por aquel.
- Hairpin activado: DNAT estático **solo UDP 42000 → 192.168.1.10:42000 y
  42001 → 192.168.1.11:42001**, más masquerade LAN→LAN y permiso de forwarding
  para conexiones con DNAT. Otros puertos de aplicaciones requieren añadir sus
  reglas DNAT; no hay descubrimiento dinámico de mapeos ni UPnP/NAT-PMP/PCP.
  Hairpin desactivado no instala estas reglas; puede devolver ICMP unreachable.
- No ejecuta el relay ni los agentes de Atenea, ni simula Internet, MTU/pérdidas/latencia,
  caducidad de mapeos, agotamiento de puertos o routers físicos. No hay afirmaciones
  sobre estado `direct`, Noise, roaming o integridad cifrada: los verificadores
  independientes siguientes comprueban las herramientas del laboratorio, no el protocolo.
- Un solo kernel Linux compartido por todos los namespaces, incluso en Docker
  Desktop. La red de Docker no crea estos NAT: son reglas explícitas dentro de
  namespaces separados. Los resultados son de este kernel y estas reglas, no
  una garantía sobre cualquier NAT de producción. El verificador imprime las
  versiones para acompañar la evidencia.

## Herramientas de relay (fase 5)

Los fixtures actuales exigen wire-v2/relay-v2 sin compatibilidad v1.
`e2e-relay.sh` genera una audiencia aleatoria del SO al preparar `/tmp/lab`,
la persiste en `relay-id` y reutiliza el mismo valor al arrancar servicios:
`ATENEA_RELAY_ID` y `ATENEA_COORD_RELAYS[].relay_id` deben coincidir
(32 hex minúsculos, no cero). El montaje del panel en `tests/e2e-panel/lab.sh`
aplica el mismo contrato. Relays distintos requieren IDs distintos y un token
por audiencia; no se deriva el ID del certificado ni se renueva al reiniciar.


```sh
tests/network-lab/nat/run.sh relay
```

Ejecuta `verify-relay-lab.sh` en el contenedor privilegiado, sin Rust ni servicios
externos. Cubre **todas** las topologías, dos ciclos por topología y los dos clientes,
con sockets UDP y conexiones TCP persistentes durante las transiciones. Comprueba
UDP 443/3478/8081 y TCP 8081, candidatos reflexivos y candidatos locales en LAN
compartida, contadores de descarte P2P, restauración y ausencia de residuos.
El simétrico mantiene su fallo directo esperado: el contador del filtro demuestra
el descarte aunque el NAT ya impidiese el punch. `run.sh verify` conserva la
verificación original de mapeos y conectividad, separada del modo `relay`.

Después de `source tests/network-lab/nat/lib.sh` y `topo_setup <topología>`:

| Función | Argumentos y efecto |
| --- | --- |
| `net_block_p2p` | Ninguno. Descarta UDP entre las redes de clientes/candidatos del laboratorio, incluyendo locales, reflexivos, doble NAT e IPv6; exceptúa todo UDP con origen o destino en las direcciones de `inet`. TCP no cambia. |
| `net_block_udp` | Ninguno. Descarta UDP de los clientes en cualquier dirección, incluido `inet`. TCP no cambia. |
| `net_block_relay_udp` | Ninguno. Descarta solo UDP cuyo destino sea `198.51.100.10:443` o `[2001:db8:100::10]:443`. |
| `net_unblock_all` | Ninguno. Elimina únicamente las tablas `inet relay_lab`; idempotente. Conserva las reglas de NAT/firewall y conntrack originales. |
| `start_haproxy` | `<puerto-relay-local> <puerto-web-local>`, ambos 1–65535. Inicia HAProxy daemonizado dentro de `inet`; devuelve error si no puede arrancarlo. |

Los bloqueadores **sustituyen** al modo anterior; invocarlos repetidamente no
acumula reglas. Usan una tabla propia en `nat1/nat2/nat3` y en `a/b` (necesario
para `flat` y tráfico LAN que no pasa por routers). Los hooks prerouting/output,
prioridad **−310**, preceden conntrack, DNAT y los permisos `established`: afectan
también a sockets y flujos ya abiertos, sin vaciar sus mapeos. Solo se inspecciona
UDP; el ICMP/ND necesario y TCP se mantienen. Los prefijos de documentación y
privados usados por estas reglas son exclusivos de este laboratorio: no es una
política de firewall para hosts externos. `topo_teardown` también mata HAProxy y
los backends al destruir sus namespaces.

### Certificados efímeros

Dentro del contenedor:

```sh
tests/network-lab/nat/gen-certs.sh /tmp/relay-certs
```

Genera `/tmp/relay-certs/ca.pem`, `/tmp/relay-certs/relay.pem` y
`/tmp/relay-certs/relay.key`, todos PEM y **0600**. Rechaza directorios dentro
del repositorio y sobrescrituras. CA y servidor duran dos días; el certificado
de servidor incluye SAN DNS `relay.atenea.one` e IP `198.51.100.10` y
`2001:db8:100::10`, uso `serverAuth`. La clave de CA y CSR solo existen en un
directorio temporal eliminado al terminar. No son certificados de producción;
el llamador elimina los tres resultados cuando deja de necesitarlos.

### Entrada TCP 443 compartida (ADR-004)

```sh
# Con topología ya creada; backends dentro del mismo namespace:
ip netns exec inet python3 tests/network-lab/nat/fake-web.py 8081 &
# Arrancar aquí el relay TLS en 127.0.0.1:8443, con PROXY v2 activado.
start_haproxy 8443 8081
```

`haproxy.cfg.tpl` escucha IPv4 e IPv6 en TCP 443, `mode tcp`,
`tcp-request inspect-delay 5s`. Solo **SNI `relay.atenea.one` Y ALPN
`atenea-relay/2`** van al relay por loopback con `send-proxy-v2`; cualquier otra
combinación, incluido SNI ausente, va al backend web **sin** PROXY protocol.
No termina TLS. `fake-web.py <puerto>` escucha loopback y contesta `WEB` al
aceptar la conexión; es TCP plano deliberadamente, no un servidor HTTP/TLS.

`haproxy-check.sh` es autónomo (recrea la topología `flat`) y forma parte del
modo `relay`. Genera sus certificados temporales y usa un relay TLS de eco
independiente (`haproxy-probe.py`) que parsea PROXY v2 y devuelve la IP observada.
Verifica cadena, SAN y permisos; TLS 1.3, ALPN y origen IPv4/IPv6 reales;
SNI distinto, SNI ausente y ALPN ausente hacia WEB; y un ClientHello fragmentado
con pausas de 0,7 s mientras otra conexión acaba en menos de 2 s, antes del
cliente lento. Los clientes son Python `ssl`/MemoryBIO; no se acepta un fallo
de handshake como prueba de enrutamiento web: se exige el banner.

Limitaciones: un kernel Linux compartido, NAT simétrico sintético y ausencia de
proxies HTTP/inspección TLS reales. No mide la conmutación de caminos del agente.
El verificador imprime versiones, contadores y tiempos de cada ejecución; no
necesita conntrack-tools ni borrar conntrack para imponer los bloqueos.

En Linux `7.0.12-linuxkit`, un `drop` UDP en OUTPUT devuelve **EPERM** a
`sendto`, en lugar de esperar un timeout. La sonda lo contabiliza como no entrega;
los controles positivos siguen exigiendo respuesta (por tanto EPERM los hace
fallar), y cualquier otro error de envío se propaga. La primera aceptación
detectó esta diferencia de API del kernel, antes de adaptar la sonda; no se
cambió el firewall ni se rebajaron las expectativas de conectividad.

### Resultado medido de fase 5

**Evidencia histórica anterior al cutover.** Las mediciones siguientes usaban
ALPN `/1`; no verifican `/2`, IDs ni tokens por audiencia. Los fixtures
actualizados no se han vuelto a ejecutar durante este cambio.


Validación final en Linux `7.0.12-linuxkit`, nftables `1.0.6`, Python `3.11.2`
y HAProxy `2.6.12-1+deb12u4`:

- `run.sh relay`: **0 fallos**, siete topologías, dos ciclos por topología;
  **113 s** dentro del verificador (**115,04 s** con construcción/arranque).
- `run.sh verify`: **0 fallos**, las siete topologías originales, **79,10 s**
  incluyendo construcción/arranque; sin reducir a home/symmetric.
- Durante el ClientHello lento, el segundo cliente TLS completó en **0,003 s**
  tanto por IPv4 como IPv6. PROXY v2 conservó `198.51.100.11` y
  `2001:db8:100::11`; SNI desconocido/ausente y ALPN ausente devolvieron WEB.
- Cadena/SAN DNS y ambas IP, permisos 0600 y ausencia de errores en los backends
  comprobados; sin namespaces, puentes ni tablas de bloqueo residuales.

La aceptación completa exitosa se ejecutó una vez al final. Hubo cuatro intentos
previos fallidos mientras se corregía el arnés: EPERM por OUTPUT; datagramas P2P
pendientes interpretados como JSON (ahora se filtra por origen); margen inicial
TCP IPv6 de 2 s; y margen UDP de 0,45 s. Los márgenes finales son 10 s para la
primera conexión TCP y 5 s para respuestas UDP, sin reintentar ni cambiar las
expectativas. EPERM es un descarte inmediato; el resto exige la respuesta.
