# Fase 5 — relays y conmutación de caminos (E2E)

- **Fecha:** 2026-10-24
- **Entorno:** contenedor Linux privilegiado (Docker Desktop, kernel linuxkit, nftables, HAProxy 2.6). Topologías de `tests/network-lab/nat/lib.sh`. En el namespace `inet` corren el coordinator (con STUN y firma de tokens), el relay (UDP 443 y TLS en loopback 8443 con PROXY v2) y HAProxy (TCP 443, SNI `relay.atenea.one` + ALPN `atenea-relay/1` → relay; resto → web falso). Dos agentes reales con TUN en `a` y `b`. Certificados y claves generados en cada ejecución.
- **Reproducir:** `tests/network-lab/nat/run-e2e-relay.sh`. El comportamiento de los bloqueadores y de HAProxy, sin Atenea, se verifica aparte con `tests/network-lab/nat/run.sh relay`.

**Evidencia histórica de relay-v1/wire-v1.** El cutover actual exige [relay-v2](../protocol/relay-v2.md), IDs persistentes distintos por relay, un token de 189 bytes por audiencia y ALPN exclusivo `atenea-relay/2`. Se conserva aquí `/1` como dato de aquella ejecución, no como configuración vigente. Estos resultados no verifican v2; el laboratorio no se ha vuelto a ejecutar durante el cutover.

## Resultados (ejecución final: 138 s, `E2E RELAY: TODO OK`)

| Caso | Resultado |
| --- | --- |
| home, sin bloqueos | **direct** a los 21 s; TCP 1 MB íntegro |
| `net_block_p2p` (UDP entre clientes) | **relay-udp** a los 10 s del bloqueo (8–10 s en tres ejecuciones); TCP 1 MB íntegro; misma sesión Noise (sin nuevo handshake) |
| `net_block_udp` (todo UDP) | **relay-tls** (TCP 443 vía HAProxy) a los 3–7 s; TCP 1 MB íntegro |
| pcap en `inet` durante todo lo anterior | 0 apariciones de una marca en claro enviada por el túnel |
| `net_unblock_all` | vuelve a **direct** en 5–10 s; 2 conmutaciones en total, ninguna más en los 20 s siguientes; `rx_auth_fail = 0`; TCP 1 MB íntegro |
| NAT simétrico (sin P2P en fase 4) | conectado por **relay-udp** a los 30 s (sondeo directo de 10 s + retardo + petición de relay); TCP 1 MB íntegro |

Regresiones y fuzz: ver «Regresiones» al final.

## Qué demuestra

- Un par pasa de directo a relay UDP y de ahí a relay TLS/TCP 443 sin renegociar la sesión Noise: un único espacio de contadores y antirreplay para los tres caminos.
- Un camino solo cuenta como sano con paquetes autenticados recibidos por él. Volver a un camino mejor exige 5 s de salud continua, y no hubo oscilación.
- El relay solo ve datagramas cifrados: la marca enviada por el túnel no aparece en el pcap del servidor. El estado del relay cuenta paquetes UDP y TCP reenviados.
- HAProxy enruta por SNI + ALPN sin descifrar; el relay recibe la IP real de cliente por PROXY v2.
- El NAT simétrico, que no tenía conectividad en la fase 4, conecta por relay.

## Qué no demuestra

- La recuperación al bloquear el P2P queda en **8–10 s**, en el límite del objetivo (≤ 10 s). Depende de la ventana de salud (9 s) y de que se pida el relay al sospechar (4 s).
- El simétrico tarda ≈ 30 s en conectar: el sondeo directo falla a los 10 s y el relay se pide a los 4 s en paralelo; se puede acortar abriendo el relay antes del primer sondeo.
- NAT y bloqueos son emulaciones nftables en un solo kernel. El recorrido real de UDP 443 y TCP 443 por el firewall del proveedor sigue **sin comprobar**. No hay mediciones de rendimiento ni de pérdida de paquetes.
- Una sola instancia de relay, sin alta disponibilidad. Los certificados son efímeros de laboratorio, no de producción.
- El E2E no está en CI (necesita contenedor privilegiado). El código sí se prueba en CI con `scripts/check.sh`, que incluye las pruebas del relay, del cliente de relay y del gestor de caminos.
- Sin revisión criptográfica externa de los mensajes del relay ni de las firmas de token.

## Cambios que salieron del E2E

- Durante el retardo posterior a un sondeo fallido, el agente no intentaba el handshake aunque ya hubiera un relay listo.
- El estado informaba `none` con un único camino y keepalive de 15 s (ventana de salud de 9 s); ahora informa el camino activo mientras esté vivo.
- Al bloquear todo UDP el propio tráfico UDP del túnel falla localmente (EPERM): la prueba de texto en claro se hace con el relay UDP.

## Regresiones

`tests/network-lab/run.sh` (fases 2 y 3), `tests/network-lab/run-e2e.sh` (control plane con Dex y PostgreSQL) y `tests/network-lab/nat/run-e2e.sh` (fase 4) siguen en verde con el agente nuevo; `tests/network-lab/nat/run.sh verify` y `relay` pasan con 0 fallos. Fuzz `relay_parse`: 60 s, 14,6 M ejecuciones, sin fallos.
