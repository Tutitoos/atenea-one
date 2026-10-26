# ADR-010: L2 Ethernet opt-in por red (fase 10)

- **Estado:** **Propuesto** (2026-10-26). Sin decisión del responsable: nada de lo que depende de este ADR se implementa antes de aceptarlo (véase [§9](#9-decisiones-para-el-responsable)). Es solo diseño y viabilidad: no hay código, ni vectores, ni laboratorio ejecutado.
- **Relacionados:** ADR-001 (Noise IK), ADR-003 (DATA, MTU), ADR-004, ADR-007 (subredes, DNS), ADR-008 (clientes; excepción Wintun), [wire-v2](../protocol/wire-v2.md), [relay-v2](../protocol/relay-v2.md), [coordinator-v1](../protocol/coordinator-v1.md), [signed-config-v1](../protocol/signed-config-v1.md), [signed-config-v2](../protocol/signed-config-v2.md), [modelo de amenazas](../security/threat-model.md) (T31+ propuestas aquí), [especificación borrador l2-v1](../protocol/l2-v1.md), [plan de la fase](../operations/fase-10-plan.md).
- **Convención de evidencia:** **VERIFICADO** = leído en la fuente o salida real indicada (con fecha); **INFERIDO** = deducción razonada sin prueba; **SIN COMPROBAR** = no se ha podido verificar en este entorno. Fechas de consulta: 2026-10-26.

## Contexto

Hasta la fase 9 Atenea One es una VPN/overlay de **IP** (L3): el dispositivo expone un adaptador TUN y el agente maneja paquetes IP puros (`TunDevice`, «paquete IP completo, sin cabecera de plataforma»; `crates/tunnel`). La fase 10 del plan incorpora «las funciones Ethernet que diferencian una red virtual L2 de una VPN de IP»: broadcast, ARP/ND, MAC, multicast, DHCP/mDNS de la red virtual.

El plan impone dos guardas que este ADR respeta: **«L2 es un hito propio; no activarlo por defecto»** y **no anunciar equivalencia completa con ZeroTier por soportar TAP**. El proyecto está además en fase 11 (auditoría externa pendiente): este ADR **no cambia** comportamiento ni protocolos aceptados (wire-v2, relay-v2, ADR-001..009); solo propone una extensión opt-in.

## 1. Viabilidad por sistema operativo

### 1.1 Linux — soportado en v1 (propuesta)

- **VERIFICADO** ([docs.kernel.org/networking/tuntap.html](https://docs.kernel.org/networking/tuntap.html), 2026-10-26): `/dev/net/tun` con `ioctl(TUNSETIFF)`; «TUN works with IP frames. TAP works with Ethernet frames»; flags `IFF_TAP`, `IFF_NO_PI` (sin cabecera de 4 bytes), `IFF_MULTI_QUEUE`; «Linux and FreeBSD drivers support Ethernet bridging»; al cerrar el descriptor la interfaz y sus rutas desaparecen; crear o conectarse a dispositivos exige `CAP_NET_ADMIN`.
- **VERIFICADO en el repositorio:** el backend actual usa `IFF_TUN | IFF_NO_PI` (`crates/tunnel/src/linux.rs`); el agente ya corre con `CAP_NET_ADMIN` (ADR-007 §6). Pasar a `IFF_TAP | IFF_NO_PI` es un cambio de flags más un tipo de dispositivo nuevo (`TapDevice`), no una ampliación de `TunDevice`.
- **Coste (INFERIDO):** bajo. Reutiliza privilegios, limpieza por cierre de descriptor y la frontera `HostNetwork` (`configure_link` fija MTU y direcciones; hay que añadir fijar la MAC y rechazar que la interfaz esté esclavizada a un puente).
- **Límite deliberado:** el agente **no** crea ni gestiona puentes del kernel con interfaces físicas. Unir `atenea-tap` a un `bridge` con una LAN física es una topología que el agente no puede proteger de bucles ni de MAC spoofing (T34/T35); se detecta y se informa (`bridged = true`), y la v1 la declara **no soportada** (decisión D3).
- **Conclusión: soportado.**

### 1.2 macOS — no soportado en v1; spike experimental opcional

No existe TAP nativo: `utun` (ADR-008 §3, `com.apple.net.utun_control`) es L3 con cabecera de familia de 4 bytes. Opciones evaluadas:

| Opción | Evidencia | Límites | Estado |
| --- | --- | --- | --- |
| `feth` (fake ethernet) + `bridge`/BPF | **VERIFICADO** en este Mac (macOS 27.0.1, `ifconfig -C`, 2026-10-26): los clonadores `feth`, `bridge`, `vlan` existen y hay `/dev/bpf*`. **SIN COMPROBAR:** crear `feth`, emparejarlo y leer/inyectar tramas por BPF desde un proceso (requiere root; no se ha ejecutado nada que cree interfaces). `man ifconfig` local no documenta `feth` (salida vacía): es una interfaz **sin documentación pública verificada**. | Sin API estable documentada; depende de comportamiento de `ifconfig`/BPF que Apple puede cambiar; BPF necesita root; sin garantía de rendimiento ni de entrega de multicast. | Candidato a **spike**, no a producto. |
| `vmnet` | **VERIFICADO** ([developer.apple.com/documentation/vmnet](https://developer.apple.com/documentation/vmnet), 2026-10-26): API «para que las máquinas virtuales lean y escriban paquetes», modos host/compartido, DHCP/NAT integrados, `vmnet_read/vmnet_write` con tramas Ethernet completas, límite de 32 interfaces; un proceso con sandbox necesita el entitlement `com.apple.vm.networking`; «IPv4 traffic originating from the guest must use the private IPv4 address. The system drops packets sent from a different IPv4 address». | Es una NIC de **invitado**, no un adaptador del host con identidad propia: el filtrado de IPv4 de origen y el DHCP/NAT forzados contradicen un L2 con MAC/IP arbitrarias. **SIN COMPROBAR:** si el entitlement exige aprobación de Apple, y qué ocurre sin sandbox/como root. | **Descartado** como adaptador de host. |
| `NEEthernetTunnelProvider` (NetworkExtension) | **VERIFICADO** ([NEPacketTunnelProvider](https://developer.apple.com/documentation/networkextension/nepackettunnelprovider) y [NEEthernetTunnelProvider](https://developer.apple.com/documentation/networkextension/neethernettunnelprovider), 2026-10-26): `NEEthernetTunnelProvider` hereda de `NEPacketTunnelProvider`, disponible macOS 13+, «implementa el lado cliente de un protocolo de túnel de enlace»; usar el provider exige el entitlement `com.apple.developer.networking.networkextension` y extensión de app con Info.plist `com.apple.networkextension.packet-tunnel`. **SIN COMPROBAR:** semántica real de tramas (lectura de `packetFlow`), si un tercero puede obtener el entitlement para el tipo Ethernet, distribución. | Extensión de aplicación con App ID, perfil de aprovisionamiento y notarización: requiere el Apple Developer ID que ADR-008 aún lista como **pendiente**; implica un cliente nativo (no el demonio launchd actual); ADR-008 §3 decidió «No se usa NetworkExtension en v1». | Única vía **documentada**; requiere nueva decisión y trabajo nativo no portable desde el agente Rust. |

**Conclusión macOS: no soportado en v1.** Un spike con `feth`+BPF puede registrarse como **experimental** solo tras M1–M3 en Linux y con aviso de que no tiene contrato público. Coste estimado (INFERIDO): alto (cliente Swift + extensión + firma) para NetworkExtension; medio y de alto riesgo de mantenimiento para `feth`.

### 1.3 Windows — no soportado en v1 (salvo excepción de driver)

- **VERIFICADO** ([wintun.net](https://www.wintun.net/), 2026-10-26): Wintun es un «Layer 3 TUN Driver for Windows»; no expone Ethernet. **No sirve para L2.**
- **La excepción Wintun de ADR-008 no cubre ningún otro controlador**: «única excepción de controlador aceptada»; «no implica aceptar … otros componentes». Un adaptador L2 es una **excepción nueva** del responsable.
- **Candidato: TAP-Windows6** (OpenVPN; [README](https://github.com/OpenVPN/tap-windows6), 2026-10-26; `ComponentId` `tap0901` en su README): driver NDIS 6.20/6.30 «used by OpenVPN and other apps». **VERIFICADO:** el código fuente y objeto están bajo **GPL v2** (`COPYING`) con una declaración adicional sobre binarios compilados con el WDK; el README indica que las firmas exigidas hoy requieren envío a **Windows Dev Center para firma por atestación** y firma EV («Microsoft's driver signing requirements have tightened considerably»); el instalador usa `tapinstall`/`devcon`; sin firma, solo con Secure Boot deshabilitado y modo de prueba.
- **SIN COMPROBAR:** si se pueden redistribuir los binarios firmados por OpenVPN junto a un agente Apache-2.0 (ADR-006) y bajo qué condiciones (GPL-2.0 de un componente separado e invocado por interfaz de dispositivo; requiere revisión de licencias del responsable); versión vigente del driver, soporte en Windows ARM64 y mantenimiento; alternativas (un adaptador L2 propio con WDK, o Hyper-V/vSwitch) **no evaluadas**: coste de driver propio = firma EV + atestación + mantenimiento de kernel, desproporcionado para v1.
- **Coste:** nueva excepción de driver (D2), revisión de licencia GPL-2.0, certificado EV y proceso de firma si se empaqueta el driver propio, instalación elevada del driver, matriz Windows 10/11 x64/ARM64.
- **Conclusión: no soportado en v1.** Se reabre solo si el responsable acepta la excepción (D2).

### 1.4 Resumen

| SO | v1 | Motivo | Coste |
| --- | --- | --- | --- |
| Linux | **Soportado** | `IFF_TAP` documentado; mismos privilegios que hoy | Bajo |
| macOS | **No soportado** (spike `feth` experimental opcional tras M3) | Sin TAP nativo; `feth` sin API documentada; NetworkExtension exige extensión, entitlement y Developer ID | Alto / medio con riesgo |
| Windows | **No soportado** (reabrible con excepción D2) | Wintun es L3; TAP-Windows6 es GPL-2.0, firma por atestación y excepción nueva | Alto y de licencia/firma |

Consecuencia: la **prueba de salida «tráfico Ethernet real entre SO soportados»** del plan se cumple en v1 solo entre agentes Linux. Un cliente macOS/Windows en una red L2 **no puede** unirse: el agente lo rechaza al verificar la configuración (§2, `UnsupportedL2Mode`), sin degradar a L3 en silencio.

## 2. Modelo: L2 opt-in por red, firmado, coexistente con L3

1. **Un modo por red, inmutable.** Cada red es `l3` (comportamiento actual) o `l2`. El modo lo decide un administrador al **crear** la red; no se convierte una red existente (evita estados mixtos y sesiones con payloads de distinto tipo). Nunca por defecto: la API/CLI exige `mode = "l2"` explícito.
2. **Declarado en la configuración firmada, versión 3.** Opciones analizadas:

| Opción | A favor | En contra |
| --- | --- | --- |
| **v3 del cuerpo (recomendada)** | Un agente v1/v2 recibe `UnsupportedVersion` y **falla cerrado** (no puede ignorar un modo que cambia el significado de los payloads). Mantiene la regla de v2: el emisor emite 1/2 si no hay L2 y 3 solo si hace falta (bytes de v1/v2 **idénticos**). Reutiliza la verificación 1–14 sin cambios de firma ni envoltorio. | Un parser más (`config_parse` v3, fuzz, vectores). |
| Extensión de v2 con un bit de `flags` | Sin versión nueva. | `flags` de v1/v2 deben ser 0; un agente v2 devolvería `Malformed`, indistinguible de un fichero corrupto; mezcla semántica y complica «codificación única». |
| Campo en el control plane sin firmar | Trivial. | **Inaceptable**: el modo no estaría autenticado. |

3. **Coexistencia con L3 (ADR-007).** En una red L2 el dispositivo sigue teniendo `tunnel_addrs` (IPv4/IPv6), `network_prefixes` y DNS dividido: esas direcciones se configuran **en la interfaz TAP** (`configure_link`) y el host resuelve ARP/ND por ella. La ACL L3 (`crates/policies::Policy`) **sigue evaluándose sobre el paquete IP** contenido en cada trama (origen validado contra `addrs` del par) además de la ACL L2. **No hay puenteado a LAN:** `routes` y `advertised` (subredes servidas, ADR-007) deben estar **vacías** en una configuración L2 de v1 (`InvalidStructure`); servir una LAN como L2 es otro diseño (D3).
4. **Un dispositivo, una red, un modo.** El agente trabaja con una red por instancia (INFERIDO de la estructura actual: un `network_id` por configuración); no hay tráfico L2 y L3 mezclado en la misma sesión Noise.
5. **Compatibilidad hacia atrás:** wire-v2 y relay-v2 **no cambian** (§3). Coordinator y relay no interpretan payloads; el coordinator, que verifica configuraciones presentadas (`inspect_config`), debe poder parsear v3 antes de que se emita una (§7, T41).

## 3. Trama sobre el cable (resumen; normativo solo al aceptar, véase [l2-v1](../protocol/l2-v1.md))

- **Sin cambio de wire-v2.** Una trama Ethernet (sin FCS, tal como la entrega un TAP con `IFF_NO_PI`) es el **payload** de un DATA `type=3`. No hay byte de tipo interno: el modo de la red, firmado, dice cómo interpretar el payload. Keepalive sigue siendo payload vacío.
- **MTU.** El payload ya incluye las 14 bytes de cabecera Ethernet: `frame_max = mtu` configurada (1280..1500). La MTU IP de la interfaz TAP es `mtu − 14` (1386 por defecto con 1400). Datagrama en el cable = `30 + frame` ≤ 1530. Con `mtu` 1400 el datagrama cabe en un enlace IPv4 de 1500 (1400+30+8+20 = 1458, [mtu.md](../protocol/mtu.md)). Un Ethernet «normal» de 1500 bytes de MTU IP (trama de 1514) **no cabe** sin fragmentar: se declara limitación, no se fragmenta.
- **Sin fragmentación ni reensamblado** de tramas (prohibido); jumbo (>1500 de payload) descartado. 802.1Q: **no** en v1 (trama 0x8100 descartada); el campo VLAN de la ACL queda reservado (D6).
- **EtherTypes permitidos:** `0x0800` IPv4, `0x0806` ARP, `0x86DD` IPv6 (incluye ND). Resto descartado (LLDP `0x88CC`, STP/LLC, tramas 802.3 con longitud, MRP, etc.) con contador.
- **Inválida ⇒ descarte silencioso** y contador local; nunca error al par (coherente con wire-v2). No se cambian handshake, antirreplay ni rekey.

## 4. Plano de control L2 (resumen; reglas en l2-v1)

- **Identidad MAC firmada:** cada dispositivo recibe en la configuración su `mac` (unicast, administrada localmente, `02:xx:xx:xx:xx:xx`; derivación recomendada `BLAKE2s-256("atenea-one/l2-mac/v1" || network_id || device_id)[0..5]` con prefijo `02`, con reintento por el emisor si colisiona) y la `mac` de cada par. Unicidad verificada al cargar la configuración. El agente fija esa MAC en el TAP; cambiarla desde el host hace que sus tramas se descarten por `src` incorrecto.
- **Anti-suplantación:** una trama recibida de un par solo se acepta si su MAC origen es la del par **autenticado por Noise** (o una declarada en su `extra_macs` firmada). El destino unicast debe ser **la MAC propia**: nunca se reenvía a otro par (sin tránsito, igual que ADR-007/T26).
- **Tabla de MAC estática en v1:** se construye de la configuración firmada (`mac` y `extra_macs` de cada par) y no envejece. El **aprendizaje dinámico con envejecimiento** (tabla acotada, primer propietario gana, cupo `max_learned` ≤ 16 por par, ≤ 4096 por red y dispositivo, 300 s) se **especifica pero se reserva** para un modo puente futuro (D3-B): sin él no hay forma de impedir que un par aprenda una MAC arbitraria. **Destino unicast desconocido ⇒ descarte** (no hay inundación de unicast desconocido). Esto significa, con honestidad, que v1 **no es un conmutador de aprendizaje**: es Ethernet de dispositivo con una tabla firmada.
- **ARP/ND locales, sin cruzar el cable:** el agente **responde localmente** a ARP request / Neighbor Solicitation de direcciones de pares firmados (`addrs`) con la MAC firmada y descarta las solicitudes de direcciones desconocidas. ARP, ND, RA y DHCP recibidos de un par se **descartan**: así un par hostil no puede envenenar la caché del host vecino (T32).
- **Difusión y multicast:** en `flood = 0` se descarta toda trama de grupo; en `flood = 1` solo mDNS, por **replicación unicast** a los pares autorizados (cada copia es un DATA normal con su contador), con presupuesto y límites de tasa; **nunca se reenvía** lo recibido de un par (malla sin tránsito ⇒ no hay bucle posible dentro del overlay). Detección de eco/duplicado por hash para el caso de que el host una el TAP a otro dominio L2.
- **Tormentas:** cubos de fichas separados por clase (ARP/ND locales, difusión replicada, multicast, ingreso por par) y cuarentena temporal al superar el umbral sostenido.
- **Aislamiento entre redes:** una red = un `network_id` = un TAP = una sesión de pares distinta; el prólogo Noise ya liga la red. Sin tabla, caché ni cubo compartido entre redes en el mismo proceso (T39).
- **ACL L2** (`crates/policies`): reglas permitir-solo con selectores `ethertype`, `src_mac`/`dst_mac` (propia, de un par, cualquiera, o prefijo OUI) y VLAN reservada; denegación por defecto; evaluada **antes** de la ACL L3 en origen y destino. Detalle y formato firmado en l2-v1 §8–9.

## 5. Descubrimiento y DHCP (alcance mínimo propuesto)

- **DHCP:** **ninguno en v1.** Las direcciones IP vienen firmadas en `tunnel_addrs`; una red L2 sin servidor DHCP propio evita un servicio de red con estado en el agente. Las tramas DHCP (UDP 67/68) se descartan por defecto (T45 si un par intenta un DHCP falso). **Fuera de alcance:** servidor DHCP en el agente, relay DHCP, DHCPv6, RA propio. Una red que necesite DHCP para hosts detrás de un dispositivo requiere el modo puente (D3), no incluido.
- **mDNS (`224.0.0.251`, `ff02::fb`, UDP 5353):** **opt-in por red** (`flood = mdns`), por replicación unicast con el presupuesto de difusión firmado (`flood_pps`/`flood_burst`); sin reflexión entre redes ni hacia la LAN física. Es descubrimiento de nombres de **otros** dispositivos; no sustituye al DNS dividido de ADR-007 (que sigue siendo el mecanismo recomendado). SSDP, NetBIOS, LLMNR y demás difusión: fuera de v1.
- Coste de ofrecerlos (INFERIDO): mDNS bajo (reutiliza replicación); DHCP medio-alto (estado, concesiones, seguridad, T45).

## 6. Rendimiento y límites

La viabilidad de rendimiento **no está medida**. Exigencias antes de documentar números (plan fase 10: «límites de rendimiento documentados»): throughput y latencia TAP↔TAP sobre P2P y relay (iperf3 en `network-lab`), pps de tramas pequeñas, coste de CPU del filtrado por trama, coste de replicación con N pares (malla de N: N−1 copias por difusión), y comparación con el túnel L3 con el mismo hardware. Quinn/QUIC solo se evalúa si estas mediciones demuestran una necesidad; no hay compresión inicial (plan).

## 7. Amenazas nuevas (propuesta de numeración; `threat-model.md` **no** se edita todavía)

Cada fila indica la prueba que se exigirá. «Mitigación» es diseño propuesto, **no verificado**.

| ID | Amenaza | Mitigación propuesta | Prueba exigida (M = hito del plan) |
| --- | --- | --- | --- |
| T31 | **MAC spoofing**: un par usa la MAC de otro o falsifica la propia | MAC origen = la del par autenticado (o `extra_macs` firmadas); tras validar, el destino debe ser la MAC propia; en salida el origen debe ser la MAC del TAP | Par A envía con MAC de B: 0 entregas, `rx_src_mac_mismatch` crece (M1, M3) |
| T32 | **ARP/ND spoofing y envenenamiento** (GARP, NA no solicitado, RA falso, DHCP falso) | ARP/ND/RA/DHCP entrantes de pares descartados; respuestas locales desde la tabla firmada; ningún mensaje de control L2 cruza el cable | Tramas ARP/NA/RA forjadas de un par hostil: la caché ARP/neigh del host víctima no cambia (M2) |
| T33 | **Tormenta de difusión/multicast** (propia o por un par) | Sin inundación de unicast desconocido; replicación unicast acotada y por tasa; cubos por clase y por par de ingreso; cuarentena | Inyección a tasa máxima del TAP y desde un par: CPU/ancho de banda acotado, otros pares no degradados (M2) |
| T34 | **Bucle L2** (TAP unido a un puente con un camino de vuelta) | Sin tránsito entre pares; detección de eco por hash recientes; detección de `master` del TAP (puente) y de duplicados; cuarentena y alarma | Laboratorio con puente Linux deliberado: bucle detectado y cortado en < tiempo definido, sin crecimiento sostenido (M2) |
| T35 | **Puente a una LAN física** como equivalente de acceso a la LAN sin ACL | Modo no soportado en v1; `bridged=true` visible; `routes`/`advertised` vacíos | Prueba de rechazo/informe de `master` (M1) |
| T36 | **Agotamiento de la tabla de MAC / de cachés** | Tabla estática acotada por la configuración (≤ 256×9); sin aprendizaje en v1; si se habilita: ≤ `max_learned` por par, ≤ 4096 por red y dispositivo, envejecimiento | Inundación de MAC origen nuevas desde un par: memoria constante, contadores de rechazo (M1) |
| T37 | **Fuga entre redes** (tramas de la red A entregadas en B; MAC repetida entre redes) | TAP, tabla, cubos y claves por `network_id`; el prólogo Noise liga la red; la MAC solo vale dentro de su red | Dos redes L2 con la misma MAC en un host: 0 tramas cruzadas, también bajo relay (M3) |
| T38 | **Tramas jumbo / mal formadas / fragmentadas** | Longitud 14..`mtu`; sin FCS; sin 802.1Q; EtherType permitido; IP/ARP/ND coherentes con su longitud | Fuzz del parser de tramas (cabecera, ARP, ND, IP interno) y vectores negativos (M1) |
| T39 | **Agotamiento de recursos del agente** (CPU/RAM por tramas, colas del TAP, replicación N×) | Cupos por par y global, colas acotadas, descarte del nuevo, copias limitadas por `flood_pps`; sin asignación proporcional a datos no autenticados | Carga con 256 pares a tasa de difusión máxima: RSS y CPU bajo presupuesto medido (M2, fase 11) |
| T40 | **Abuso del relay** como amplificador L2 (difusión replicada × N asignaciones) | Cuotas de relay-v2 por asignación (2000 pps, 24 Mbit/s) ya acotan; el presupuesto de difusión del emisor se aplica antes; las copias son DATA normales | Malla de N con difusión mDNS sostenida: sin superar cuotas, relay estable (M5) |
| T41 | **Coordinator comprometido** sirve configuraciones v3 o manipula el modo | El modo y MAC están firmados por el emisor; el coordinator no firma; un agente rechaza versiones/modos inesperados; sin cambios en candidatos/rendezvous | Configuración v3 con modo alterado: `BadConfigSignature`; agente v2 ante v3: `UnsupportedVersion` (M1) |
| T42 | **Degradación silenciosa a L3** o confusión de modo entre pares | Modo inmutable por red; v1/v2 no parsean v3; payload nunca se «adivina» por contenido; descartes contabilizados | Par L3 recibe payload L2: descarte contado; nunca entrega a la pila IP (M1) |
| T43 | **Exposición accidental de L2** (activación por defecto o confusión con equivalencia ZeroTier) | `mode=l2` explícito; documentación sin claim de equivalencia (D7) | Revisión de documentación y de defaults de CLI/API (M3) |
| T44 | **Fuga por ACL L2 incompleta** (EtherType/MAC) | Denegación por defecto; evaluación antes de L3; sin regla ⇒ 0 tráfico | Matriz de pruebas de ACL, incluida la regla inversa (M3) |
| T45 | **DHCP/RA falso** de un par | Descartar UDP 67/68 y RA siempre en v1 | Servidor DHCP falso: 0 ofertas entregadas (M1; si se acepta DHCP en v1.1, M5) |

## 8. Criterios de aceptación de la fase 10 (del plan) y reparto

Del plan, «Verificación / salida» de la fase 10 se descompone así:

| Criterio del plan | Hito | Notas |
| --- | --- | --- |
| Tráfico Ethernet real entre SO soportados | M1 (Linux↔Linux), M4 si se aprueba otro SO | Solo Linux en v1 si D1 = recomendación |
| Descubrimiento y broadcast acotados | M2, M5 | ARP/ND locales; mDNS opt-in |
| Ataques de MAC spoofing y tormentas contenidos | M1–M3 | T31, T33, T34 |
| Redes aisladas | M3 | T37 |
| Coexistencia L3 | M1, M3 | L3 sigue funcionando en redes `l3` y ACL L3 dentro de L2 |
| Límites de rendimiento documentados | M2/M3 y cierre | §6 |
| Editor incapaz de guardar políticas inválidas o saltarse autorización | M6 | **Otro tramo** (no L2) |

El **editor Monaco de políticas**, la **topología avanzada** y la **selección regional por latencia/pérdida/carga** son trabajo de la fase 10 del plan pero **independiente** de L2; Quinn/QUIC **solo** tras mediciones que lo justifiquen. Se planifican en M6 y no bloquean M1–M5. Hitos, pruebas de salida y laboratorios: [fase-10-plan](../operations/fase-10-plan.md).

## 9. Decisiones para el responsable

Cada decisión indica opciones, coste, recomendación y consecuencia. **Marcadas como pendientes; ninguna está tomada.**

### D1. Sistemas operativos soportados en v1
- **A (recomendada): solo Linux.** Coste bajo; todos los criterios de salida verificables en CI/Docker. Consecuencia: macOS/Windows no pueden unirse a redes L2; el panel debe decirlo al crear la red.
- B: Linux + spike macOS `feth` experimental. Coste medio, sin API pública verificada; resultado «experimental».
- C: Linux + macOS (NetworkExtension) + Windows (TAP). Coste muy alto: cliente nativo, driver y firma; bloquea la fase hasta tener Developer ID, EV, hardware real (ADR-008 pendientes).

### D2. Excepción de driver en Windows
- **A (recomendada): no aceptar ninguna** en v1 (Windows sin L2).
- B: aceptar TAP-Windows6 (GPL-2.0, firma por atestación de OpenVPN, `tap0901`), con revisión de licencia previa y sin alojarlo en el repositorio, como se hizo con Wintun. Consecuencia: instalación elevada de un segundo driver; superficie de kernel adicional; revisión de auditoría ampliada.
- C: driver propio. Coste: certificado EV, firma por atestación, mantenimiento de kernel; no recomendado.

### D3. Modo de dispositivo frente a modo puente
- **A (recomendada): L2 de dispositivo.** Cada dispositivo es un nodo Ethernet con una MAC firmada y sus IPs firmadas; sin puente a LAN, sin MAC aprendidas (tabla estática + `extra_macs`). Consecuencia: no sirve para «poner una LAN/VMs detrás de un dispositivo»; eso se mantiene con subredes L3 (ADR-007).
- B: modo puente (VMs/hosts con MAC propias detrás de un dispositivo, aprendizaje bajo cupo `max_learned`). Coste alto: defensa contra bucles y suplantación mucho más difícil; posponer a un ADR propio.

### D4. Modo de configuración
- **A (recomendada): configuración firmada v3**, emitida solo cuando la red es L2 (§2).
- B: extensión de v2 por `flags`. Más barato en parser pero ambiguo (§2).
- Consecuencia de A: actualizar coordinator, control plane (`inspect_config`, API) y agentes antes de emitir ninguna v3; agentes antiguos fallan cerrado.

### D5. Política de difusión/multicast
- **A (recomendada): `proxy` por defecto** (`flood = 0`: solo ARP/ND locales; difusión/multicast descartados); `flood = 1` (`mdns`) opt-in por red para mDNS con presupuesto.
- B: replicación de toda difusión permitida. Coste: T33/T40 más difíciles de acotar.
- C: inundación estilo conmutador (unicast desconocido). **No recomendada**: contradice las defensas de tormenta y bucle.

### D6. VLAN (802.1Q)
- **A (recomendada): no en v1** (tramas etiquetadas descartadas; campo ACL reservado). Consecuencia: la parte «VLAN» de la ACL L2 del plan queda diferida a v2 de la especificación.
- B: permitir una VLAN por red. Coste: MTU −4, más superficie de parser y ACL.

### D7. DHCP, mDNS y posicionamiento
- **DHCP/mDNS en v1:** recomendado **sin DHCP** y **mDNS opt-in** (§5); alternativa: ninguno; o DHCP en el agente (coste medio-alto, T45).
- **Frase de posicionamiento permitida** (el plan prohíbe afirmar equivalencia con ZeroTier). Propuesta: «Atenea One ofrece, de forma opcional y por red, una red virtual Ethernet entre agentes Linux con un conjunto limitado de tramas (IPv4, ARP, IPv6/ND), MAC asignada y firmada y difusión acotada; no replica todas las funciones de otros productos ni es equivalente a ZeroTier». **Prohibido:** «equivalente a ZeroTier», «compatible con ZeroTier», «L2 completo», cualquier afirmación de seguridad auditada o de preparación para producción.
- **Consecuencia** de aceptar cualquiera: el panel, el README y la matriz de aceptación se actualizan con exactamente esa frase.

## 10. Consecuencias

- Aceptar el ADR habilita implementar solo M1 (Linux TAP, trama, MAC firmada) tras escribir vectores y un parser v3; nada se activa por defecto.
- Se añade superficie auditable: parser de tramas y de configuración v3, filtros ARP/ND, tablas y cubos. La auditoría externa pendiente (fase 11) no cubre L2: debe repetirse o ampliarse tras la implementación.
- Cambios en componentes existentes, solo si se acepta: `crates/identity` (v3), `crates/policies` (ACL L2), `crates/tunnel` (tipo TAP separado de TUN), `apps/agent/src/platform` (TAP Linux, MAC, detección de `master`), control plane/OpenAPI (modo de red, asignación de MAC), coordinator (parseo v3). **Ningún** cambio en wire-v2, relay-v2, STUN ni handshake.

## 11. Pendiente y sin comprobar

- Todo lo marcado **SIN COMPROBAR** en §1: viabilidad real de `feth`+BPF, entitlement de `NEEthernetTunnelProvider` y de `vmnet`, redistribución de TAP-Windows6 junto a Apache-2.0, alternativas Windows no evaluadas.
- Rendimiento (§6) y los umbrales de tasa (valores por defecto de l2-v1 son hipótesis a medir).
- Que el kernel Linux entregue/consuma tramas y MAC esperados con `IFF_TAP|IFF_NO_PI` en los runners de CI y en Docker: documentación leída, **no ejecutada**.
