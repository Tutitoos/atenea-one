# Relay y conmutación de caminos v1 (fase 5)

- **Estado:** Aceptado (2026-10-24, por el responsable; implementado y verificado en laboratorio, fase 5)
- **Fecha:** 2026-10-24
- **Relacionados:** ADR-001/003 (Noise y sesión), ADR-004 (entrada 443, tramado TCP), `docs/protocol/coordinator-v1.md`, modelo de amenazas T2, T9, T10, T15.
- **Implementa:** `crates/relay-core` (mensajes, token, cookie, tramas), `apps/relay`, `apps/coordinator` (`/v1/relay`), `apps/agent` (`relay.rs`, gestor de caminos en `engine.rs`).

## 1. Principios

- El relay reenvía **datagramas de Atenea ya cifrados de extremo a extremo**. No tiene claves, no puede leer, alterar sin que falle la autenticación, ni inyectar tráfico válido. Ve IPs, tamaños y tiempos (T15) y puede descartar o retrasar (denegación).
- **No es un proxy abierto.** Solo reenvía entre dos **asignaciones** (`allocations`) que se corresponden: la de `X→Y` y la de `Y→X`, ambas autorizadas por un token firmado por el coordinator. Sin pareja no se reenvía nada.
- **Autenticación antes de asignar recursos:** cookie sin estado (UDP) o TLS (TCP), después token del coordinator y prueba de posesión del dispositivo.
- **Un único espacio de sesión/nonce para todos los caminos** (ADR-003): el mismo `Session` y el mismo contador cifran el tráfico enviado por directo, por relay UDP o por relay TLS/TCP; el antirreplay del receptor deduplica las copias. Cambiar de camino no cambia claves ni reinicia contadores.
- TLS en 443 no garantiza atravesar proxies HTTP ni inspección TLS corporativa (redes no soportadas). TCP sobre TCP degrada el rendimiento: el camino TLS es el último recurso.

## 2. Mensajes (`atenea-relay-core`)

Todos empiezan por `0xA1` (el primer byte de un datagrama de Atenea es `0x01`) y un byte de tipo. Big-endian.

| Tipo | Contenido | Notas |
| --- | --- | --- |
| `0x01` COOKIE_REQ | relleno hasta **64 bytes exactos** | La petición no es menor que la respuesta (T9) |
| `0x02` COOKIE | `cookie[16]` | `BLAKE2s(secreto ‖ IP ‖ puerto ‖ ⌊t/60⌋)[..16]`; válida el minuto vigente y el anterior; ligada al origen |
| `0x03` ALLOC | `cookie[16] ‖ ts u64 ‖ token_len u16 (=173) ‖ token[173] ‖ sig[64]` | `sig` = Ed25519 con la clave de identidad del dispositivo sobre `"atenea-one/relay-alloc/v1\0" ‖ cookie ‖ ts ‖ token`. En TCP la cookie va a ceros (el origen ya está validado por TCP/TLS) |
| `0x04` ALLOC_OK | `alloc_id u64 ‖ ttl_secs u16 ‖ peer_ready u8` | `alloc_id` aleatorio e impredecible |
| `0x05` ALLOC_ERR | `code u8` | 1 token inválido, 2 caducado, 3 límite de tasa, 4 lleno, 5 no encontrado, 6 firma inválida, 7 cookie inválida, 8 no autorizado |
| `0x06` DATA | `alloc_id u64 ‖ datagrama de Atenea` | Del cliente: su `alloc_id`. Del relay: el `alloc_id` de la asignación **receptora**. Datagrama ≤ 1530 bytes |
| `0x08`/`0x09` PING/PONG | `alloc_id u64 ‖ nonce u64` | Latencia y vivacidad del camino |
| `0x0A` PEER_STATE | `alloc_id u64 ‖ ready u8` | El relay avisa cuando la asignación pareja aparece o desaparece |

**Token** (173 bytes, firmado por el coordinator): `"AORT" ‖ 1 ‖ network_id ‖ src_device ‖ dst_device ‖ src_identity_pub ‖ not_before ‖ not_after ‖ token_id[8] ‖ sig[64]`, dominio `"atenea-one/relay-token/v1\0"`. Vida ≤ 600 s (el coordinator emite 300 s); tolerancia de reloj de 30 s. Autoriza **a `src` a enviar a `dst`**; el relay exige que quien presenta el token demuestre poseer `src_identity_pub`.

**Tramado TCP:** `longitud u16 ‖ cuerpo` (ADR-004); longitud 0 o > 2048 cierra la conexión; el cuerpo es uno de los mensajes anteriores (sin `COOKIE_REQ`/`COOKIE`). Lecturas parciales soportadas; memoria acotada.

## 3. Comportamiento del relay (`apps/relay`)

- **Escucha:** UDP (`443` en producción) y TCP+TLS (`443` detrás de HAProxy, ADR-004; en el laboratorio puede escuchar directamente). TLS 1.3 mínimo, ALPN `atenea-relay/1`, sin 0-RTT, certificado/clave PEM configurados. Detrás de HAProxy acepta PROXY protocol v2 **solo desde loopback** para conocer la IP real del cliente.
- **Configuración** (Figment, `ATENEA_RELAY_*`): direcciones UDP/TCP, `COORDINATOR_KEYS` (claves públicas hex que pueden firmar tokens), cert/clave TLS, `PROXY_PROTOCOL`, `STATUS_SOCKET` y los límites siguientes.
- **Asignación** (UDP: `COOKIE_REQ → COOKIE → ALLOC`; TCP: `ALLOC` en los primeros 5 s de la conexión). Validaciones en este orden, barato primero: cookie (UDP), límites de tasa por IP, estructura del token, firma del token con alguna clave de coordinator, ventana de tiempo del token (`not_after` + 30 s) y `ts` (±30 s), prueba de posesión (`sig`), y límites de capacidad. Éxito: se crea o se **refresca** la asignación `(network, src, dst)` y se **vincula** a la dirección UDP de origen o a la conexión TCP (la más reciente gana); `ALLOC_OK`.
- **Vida:** `min(token.not_after + 30 s − ahora, 120 s)`; se renueva con un nuevo `ALLOC` (misma clave; el agente lo repite cada 60 s con un token vigente). Sin tráfico ni renovación caduca; el barrido corre cada 5 s y libera memoria.
- **Reenvío:** un `DATA` de la asignación `A = (net, X, Y)` solo se acepta si llega desde la vinculación de `A` (misma dirección UDP de origen o misma conexión). La pareja es `B = (net, Y, X)`. Si `B` existe y está vinculada, se envía el datagrama como `DATA` con el `alloc_id` de `B` a su vinculación; si no, se descarta y se cuenta. El relay **nunca** envía a una dirección elegida por el cliente.
- **PEER_STATE:** al crear/borrar/refrescar `B`, avisa a `A` (y viceversa) para que el agente sepa si el par ya está presente.
- **Límites** (configurables; valores por defecto): 4096 asignaciones; 1024 por red; 32 por IP de origen; 2000 paquetes/s y 24 Mbit/s por asignación (cubo de fichas); `COOKIE_REQ` ≤ 20/s por IP; `ALLOC` ≤ 10/s por IP; datagrama ≤ 1530; TCP: cola de escritura de 256 datagramas por conexión (se descarta el nuevo si está llena: sin crecimiento sin límite), inactividad 90 s, 8 conexiones por IP, 2048 totales, plazo de 10 s para el handshake TLS y 5 s para `ALLOC`. Tablas acotadas; tras exceder, `ALLOC_ERR(Full/RateLimited)`.
- **Observabilidad:** socket Unix de estado (JSON: asignaciones, vinculadas, bytes/paquetes por dirección, descartes por motivo, rechazos por código). Sin tokens, firmas, cookies ni cargas útiles en los registros.
- **Seguridad:** todo el parseo con límites; sin `unsafe`; los mensajes no válidos no reciben respuesta salvo `ALLOC_ERR` a una `ALLOC` con cookie válida (o TCP); un origen que no posee cookie solo obtiene `COOKIE` (más pequeña que su petición).

## 4. Coordinator (`/v1/relay`)

- **Configuración nueva:** `RELAY_SIGNING_KEY_FILE` (semilla hex 0600; `--print-relay-key` imprime la clave pública para el relay), `RELAYS` (lista JSON de `{ "name", "udp": "host:puerto", "tls": "host:puerto", "tls_name": "nombre" }`; `udp` o `tls` pueden faltar), `RELAY_TOKEN_TTL_SECS` (300, máximo 600).
- `POST /v1/relay` (firmada por el dispositivo, mismas reglas de autorización **mutua** que `rendezvous`). Cuerpo `{"peer": "<hex16>"}`. Respuesta `200 {"token": "<base64, 173 bytes>", "relays": [...], "ttl_secs": 300}` con el token de `solicitante → par`, y se encola al par un aviso `{"kind": "relay", "peer": "<solicitante>", "token": "<base64, token par → solicitante>", "relays": [...], "seq": n}`. `src_identity_pub` de cada token procede de la presencia vigente de cada dispositivo. Mismas respuestas uniformes (`404 peer_unavailable`, `409 not_announced`) y límites que `rendezvous` (≤ 1 por 5 s por par, ≤ 10/min por dispositivo) más un tope de 4 tokens por par y minuto. Sin `RELAY_SIGNING_KEY_FILE` o sin `RELAYS`: `503 relay_unavailable`.
- La clave de firma de relay es **distinta** de cualquier otra y solo permite autorizar asignaciones; su compromiso permite usar relays sin autorización de la red (recurso), no leer tráfico.

## 5. Agente

### 5.1 Cliente de relay (`relay.rs`)

- Tras un `/v1/relay` propio o un aviso `relay`, abre una asignación por par: **UDP primero** (socket UDP propio, `COOKIE_REQ → COOKIE → ALLOC`, reintento cada 1 s, 3 intentos); si no hay `ALLOC_OK` en ~3 s, o el relay solo ofrece TLS, **TLS/TCP** (conexión TLS 1.3, SNI y verificación del nombre `tls_name`, ALPN `atenea-relay/1`, raíces de `webpki-roots` más `relay_ca_file` opcional; `ALLOC` en la primera trama; lectura con `FrameDecoder`). Renueva con `ALLOC` cada 60 s; vuelve a pedir token al caducar el suyo.
- `PING` cada 5 s por asignación: mide latencia (media móvil) y pérdida y alimenta el estado.
- Interfaz: `open(peer, token, relays)`, `send(peer, kind, datagrama)` (UDP o cola acotada TLS; si la cola está llena se descarta), `close(peer)`; eventos `Ready{peer, kind}`, `Down{peer, kind}`, `Rx{peer, kind, datagrama}`, `Rtt{peer, kind, ms}`.

### 5.2 Gestor de caminos (`engine.rs`)

Por par, tres caminos: **directo**, **relay UDP**, **relay TLS**; preferencia `directo > relay UDP > relay TLS`.

- **Salud:** un camino está sano si recibió un paquete **autenticado** (de sesión) en los últimos **9 s**. Cada camino establecido envía un keepalive cifrado cada **4 s** mientras haya otro camino alternativo establecido, y cada 15 s si es el único (adaptativo).
- **Envío:** los datos van por el camino **activo**. Los mensajes de handshake se envían por directo y por los relays listos; la respuesta vuelve por el mismo camino de entrada.
- **Sospecha y solicitud de relay:** si se enviaron datos en los últimos 4 s y el camino activo no recibió nada autenticado en ≥ 4 s, o el sondeo directo lleva ≥ 4 s sin éxito, el par necesita relay: se pide `/v1/relay` (una vez cada 5 s) y se abre la asignación. En cuanto el relay esté listo se envía un keepalive por él (así el par aprende el camino).
- **Conmutación (con histéresis):** si el camino activo deja de estar sano y otro está sano → **cambia de inmediato** (conmutación a peor). Si hay un camino **preferido** sano de forma continua ≥ **5 s** → sube (nunca se baja voluntariamente). Por tanto, un camino que oscila no provoca conmutaciones en cadena.
- **Recuperación del directo:** mientras el activo es un relay, cada 20 s (con el retardo habitual tras fallos) se reintenta el directo con el mecanismo de la fase 4; la asignación del relay se mantiene y se libera 30 s después de que el directo lleve estable ≥ 30 s.
- **Nonces/antirreplay:** no cambian; la ventana del receptor elimina las copias que lleguen por dos caminos. Un paquete repetido por otro camino cuenta como `rx_replay`, no como fallo.
- **Estado (IPC):** por par `path` (`direct`, `relay-udp`, `relay-tls`, `probing`, `none`), `paths` con `{healthy, last_rx_secs, rtt_ms}` para cada camino, contadores `path_switches`, `relay_requests`; global `relay` con las asignaciones.
- **Configuración local:** en `[coordinator]`, `relay_ca_file` (PEM opcional con CAs extra) y `relay = "auto" | "off"` (por defecto `auto`).

## 6. HAProxy (ADR-004)

`mode tcp` en `:443`, `tcp-request inspect-delay 5s`, `req.ssl_sni -i relay.<dominio>` y `req.ssl_alpn -m str atenea-relay/1` → relay en `127.0.0.1:<puerto>` con `send-proxy-v2`; el resto → Caddy/web. Verificado en el laboratorio con un backend web ficticio; la configuración de producción vive en la fase 9.

## 7. Amenazas específicas

| Riesgo | Mitigación |
| --- | --- |
| Uso del relay como proxy abierto o para reflexión | Solo reenvía entre asignaciones emparejadas autorizadas por token firmado y con prueba de posesión; sin pareja, descarta; nunca envía a direcciones elegidas por el cliente; cookie ligada al origen y `COOKIE` menor que `COOKIE_REQ` |
| Robo de token | El token solo vale con la clave de identidad del `src` (prueba de posesión); vida ≤ 5 min |
| Agotamiento de memoria/CPU/ancho de banda | Límites de §3 (tablas, por IP, por asignación, tasa, tamaño, tiempos); trabajo caro (firmas) tras cookie/TLS y límites de tasa |
| Relay hostil | No ve claves ni texto en claro; puede descartar/retrasar/duplicar: Noise autentica y el antirreplay elimina copias |
| Coordinator comprometido emite tokens | Solo permite consumir recursos del relay para dispositivos con identidad válida (no leer tráfico) |
| Inyección por falsificación de origen UDP | Cookie con origen; `DATA` solo desde la vinculación; cualquier inyección falla en AEAD en el destino |
| Conmutación oscilante o amplificada por un atacante de ruta | Histéresis (5 s), salud solo con paquetes autenticados, límites en solicitudes de relay |
| Metadatos en el relay (IPs, tiempos, tamaños) | No se ocultan (T15) |

## 8. Verificación exigida

- **relay-core:** mensajes, token, cookie, prueba de posesión, tramas (hecho; pruebas y fuzzing).
- **Relay:** pruebas de integración con sockets reales: asignación UDP y TLS, reenvío bidireccional, tokens malos (firma, caducado, otro emisor, red/dispositivos incorrectos), prueba de posesión inválida, cookie inválida/caducada/de otro origen, pareja ausente, `DATA` desde dirección no vinculada, límites (asignaciones, por IP, tasa, tamaño, tramas TCP), caducidad y barrido, PROXY v2 (aceptado solo desde loopback), TLS 1.3 y ALPN obligatorios.
- **Coordinator:** `/v1/relay` autorizado/no autorizado/unilateral, tokens verificables con `atenea-relay-core`, evento al par, límites.
- **Laboratorio de NAT con relay (agentes reales):** 1) bloquear P2P (cortafuegos entre NAT) fuerza relay UDP; 2) bloquear todo UDP fuerza TLS/TCP; 3) retirar (bloquear) el relay UDP con el camino TLS disponible se recupera por TLS; 4) recuperar el directo al desbloquear, sin oscilaciones ni replay; 5) datos íntegros por los tres caminos; 6) captura en el relay sin texto en claro; 7) tiempos de recuperación medidos (objetivo ≤ 10 s con alternativa); 8) HAProxy con SNI/ALPN hacia el relay y un backend web; 9) symmetric NAT conecta ahora por relay.
- **Regresiones:** laboratorios de fases 2, 3, 3B y 4.

## 9. Fuera de alcance de la fase 5

Selección de relay por región/latencia (fase 10), varios relays por par y balanceo, alta disponibilidad del relay (fase 9), cuotas por organización, `relay` por QUIC, TURN estándar, medición bajo carga sostenida (se registrarán latencia y pérdida básicas).
