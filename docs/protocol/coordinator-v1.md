# Coordinator y conexión directa v1 (fase 4)

- **Estado:** Base aceptada e implementada (2026-10-24); evolución wire-v2/relay-v2 de §4/§10 aceptada el 2026-10-25, pendiente de implementación y verificación. Los [resultados históricos](../operations/e2e-fase4.md) no verifican esa evolución. Pendiente la revisión criptográfica externa.
- **Fecha:** 2026-10-24
- **Relacionados:** ADR-001 (Noise IK), ADR-003 (sesión y roaming), ADR-004 (puertos), `docs/protocol/signed-config-v1.md`, `docs/api/control-plane-v1.md` §3.3 (autenticación de dispositivo), modelo de amenazas T3, T9 y T10.

## 1. Principios

- **Un endpoint anunciado no es una ruta autenticada.** Los candidatos de STUN/coordinator/configuración son pistas. En el establecimiento inicial, la ruta se confirma solo con respuesta de handshake o datos Noise autenticados, nunca con INIT ni respuesta de cookie. El roaming de una sesión establecida sigue actualizando el endpoint con datos autenticados sin desafío de retorno adicional (riesgo residual aceptado, ADR-003); no confundir autenticación de sesión con posesión de la nueva dirección.
- El coordinator **no es de confianza para confidencialidad ni integridad**: solo entrega candidatos y avisos. No ve claves privadas ni tráfico. Si miente, el handshake autenticado falla (mismo modelo que un relay hostil, T2/T3).
- El coordinator **no consulta la base de datos del control plane**: es sin estado duradero. Su autorización es la **configuración firmada** que el dispositivo presenta (misma cadena de confianza que el agente, sin estado anti-rollback): raíz → certificado de emisor → configuración.
- No depende de STUN público: el STUN lo sirve el propio coordinator (UDP 3478 y 3479, ADR-004).
- Un agente solo contacta, por instrucción del coordinator, con dispositivos que figuran como **pares en su propia configuración firmada**, con límites estrictos de tráfico (§6).

## 2. STUN propio

Servidor UDP en `0.0.0.0`/`::`, dos sockets (puertos 3478 y 3479, configurables). RFC 8489: solo `Binding` request → success con `XOR-MAPPED-ADDRESS`; sin autenticación ni `FINGERPRINT`. Ignora todo lo que no sea una petición Binding bien formada (sin respuesta de error: no es un reflector). Respuesta ≤ 44 bytes para una petición de 20 (factor ≤ 2,2); **límite de tasa por IP de origen** (cubo de fichas: 20/s, ráfaga 40) y tabla de orígenes acotada.

Detección de mapeo (en el agente): consulta, **desde el socket de datos**, a los puertos 3478 y 3479 del mismo servidor. Si el endpoint externo coincide: mapeo independiente del destino (hole punching viable); si difiere: mapeo dependiente (NAT simétrico: la conexión directa fallará y se informa). Con una sola IP pública no es posible clasificar el filtrado (RFC 5780 necesita dos IP).

El agente comparte el socket UDP entre STUN y Noise; discrimina con `looks_like_stun` (cookie en los bytes 4..8 y longitud exacta). Un Init de Atenea coincide por azar con probabilidad ≈ 2⁻³² (el cliente reintenta).

## 3. API HTTP del coordinator

Prefijo `/v1`, JSON, `Authorization: AtDevice v1 …` (contrato del control plane §3.3) con la clave de identidad Ed25519 del dispositivo. El coordinator obtiene esa clave de la **configuración presentada** en `announce`; el resto de peticiones se verifican contra la presencia vigente. Ventana `|now − ts| ≤ 60 s` y caché de nonces de 120 s (en memoria, acotada). Fallo de autenticación: `401 invalid_signature` uniforme.

| Ruta | Descripción |
| --- | --- |
| `POST /v1/announce` | Cuerpo `{"config": "<base64 del SignedConfig>", "candidates": ["ip:puerto", …]}` (≤ 160 KiB). Verifica la configuración **sin estado** (`atenea_identity::inspect_config`: pasos 1–6, 8, 9, 12 y 14 del contrato; **no** aplica anti-rollback ni claves locales) contra los anclas de confianza configurados en el coordinator; exige `config.device_id == device` de la cabecera y verifica la firma de la petición con `config.identity_pubkey`. Guarda la presencia y devuelve `{"ttl_secs": 60, "observed_ip": "<ip de origen>"}`. |
| `POST /v1/rendezvous` | Cuerpo `{"peer": "<hex 16 bytes>"}`. Requiere presencia vigente (`409 not_announced` si no). Autoriza **solo si es mutuo**: el solicitante lista al par en su configuración **y** el par tiene presencia vigente en la misma red y lista al solicitante. Si no: `404 peer_unavailable` (misma respuesta para inexistente, no autorizado o desconectado). Éxito: `{"peer": "…", "candidates": [...]}` con los candidatos del par y un aviso encolado al par con los del solicitante. |
| `GET /v1/events?wait=N` | Long-poll (N ≤ 30 s, por defecto 25). `{"events": [{"kind": "connect", "peer": "<hex>", "candidates": [...], "seq": n}]}`; devuelve y vacía la cola. Los avisos caducan a los 30 s; cola máxima de 16 por dispositivo (se descarta el más antiguo). |
| `GET /healthz` | `{"status": "ok", "presence": <n>}` (número de presencias vigentes; sin datos de dispositivos). |

**Presencia.** Clave `device_id`; contiene `network_id`, pares (ids) de su configuración, candidatos saneados (`sanitize_candidates`, máximo 8) y caducidad `min(now + 60 s, config.not_after + 30 s)`. Un barrido cada 5 s elimina las caducadas: **no quedan dispositivos fantasma** pasado el TTL. Un `announce` posterior sustituye el registro. Una configuración caducada o con firma inválida se rechaza (`403 invalid_config`).

**Límites operativos.** Tamaño de cuerpo 160 KiB; hasta 4096 presencias y 256 long-polls simultáneos (configurable), con máximo 2 polls por dispositivo y ⌈total/4⌉ por red; `announce` ≤ 1 por 2 s por dispositivo; `rendezvous` ≤ 1 por segundo por par y ≤ 10 por minuto por dispositivo. Tras verificar la firma, todas las rutas firmadas comparten un cubo por dispositivo (1/s, ráfaga 16) y otro por red (8/s, ráfaga 64) antes de reservar nonces. La caché conserva cada nonce vigente durante 120 s, purga caducados y aplica cuotas por dispositivo mín(256, ⌈total/4⌉) y red mín(4096, ⌈total/2⌉), además del total configurable (65536). Los excesos de capacidad/tasa devuelven `429`, no `401`. Respuesta o cancelación liberan los tres cupos de long-poll. Las cuotas son locales al proceso; no cambian el formato firmado.

## 4. Comportamiento del agente

Configuración local nueva (TOML, sin firmar):

```toml
[coordinator]
url = "https://host:8081"         # base HTTPS (http solo en loopback o con insecure_http = true, laboratorio)
stun = ["host:3478", "host:3479"] # dos puertos del mismo servidor, en este orden
```

- **Candidatos propios:** direcciones de las interfaces con el puerto de escucha (excluye loopback, enlace local y no enrutables), el endpoint reflexivo de STUN y, si lo hay, los endpoints de la configuración firmada para los pares. Se recalculan cada 30 s o ante cambio de interfaz; un cambio dispara un `announce` inmediato.
- **Presencia:** `announce` cada 20 s (TTL 60 s) mientras haya configuración vigente; sin lease no se anuncia.
- **Aviso de pares:** long-poll continuo a `/v1/events`. Un evento solo se atiende si `peer` figura en la configuración vigente; si no, se ignora y se cuenta.
- **Iniciar una conexión** (hay tráfico hacia un par sin ruta directa confirmada): `rendezvous` (≤ 1 cada 5 s por par). Con los candidatos del par más los endpoints de la configuración, se inicia el sondeo.
- **Sondeo (hole punching):** se conserva el mismo mensaje 1 de Noise e índice pendiente por intento; cada datagrama [wire-v2](wire-v2.md) de **146 bytes** incluye MAC1 y el MAC2 de su propio candidato/camino (cero si no tiene cookie). Se envía cada ~1 s con jitter ±25 %, durante ≤10 s y ≤12 envíos por candidato, a ≤8 candidatos más los configurados. Una COOKIE_REPLY válida permite reemplazar MAC2 y reintentar solo en su ruta dentro del presupuesto, sin confirmar endpoint ni ampliar sondeo. Ambos lados sondean al recibir aviso; en simultáneo gana la pública menor (ADR-001). Tras fallo, retardo desde 30 s creciente hasta 5 min antes de otro rendezvous.
- Recibir más candidatos durante un intento pendiente adelanta el próximo envío, pero **no reinicia** su ventana de 10 s ni sus 12 rondas. Los avisos repetidos del coordinator no pueden mantener indefinidamente el mismo mensaje 1 en `probing`.
- **Confirmación (validar el retorno):** `peer.endpoint` solo cambia cuando **autentica** un paquete: respuesta de handshake del iniciador o datos de una sesión. El **respondedor ya no actualiza el endpoint al aceptar un mensaje 1** (un mensaje 1 repetido dentro de la ventana de 30 s ya no puede redirigirle tráfico); responde a la dirección de origen sin cambiar el endpoint confirmado, que se fija con el primer paquete autenticado de la sesión.
- **Keepalive:** con ruta directa y sesión confirmada, un paquete vacío cada 15 s de inactividad (los mapeos UDP de los NAT suelen caducar a ~30 s). Si no se recibe nada del par durante 45 s, la ruta se da por perdida: se vacía el endpoint confirmado y se vuelve a `rendezvous`.
- **Roaming / cambio de red:** si tras un cambio de red el par envía datos autenticados desde otro origen, se actualiza el endpoint (ADR-003). Si es el agente quien cambia de red (nueva IP/interfaz), recalcula candidatos, anuncia y vuelve a sondear.
- **Estado (IPC):** por par `path` (`direct` | `probing` | `none`), `endpoint` confirmado, `candidates` (número), `last_rx_secs`; global `nat` con `mapping` (`independent`/`dependent`/`unknown`), `reflexive`, `candidates`.
- **IPv6:** el socket de datos escucha en `[::]` de doble pila (las direcciones IPv4 se normalizan); sin NAT, el cortafuegos con estado exige salida simultánea: el sondeo lo cubre.

## 5. Contrato futuro

La lista de coordinators y STUN de una red se distribuirá en la configuración firmada v2 (relays y coordinators; fase 5). En la fase 4 va en el TOML local del agente; no es una decisión de seguridad (el coordinator no es de confianza).

## 6. Amenazas específicas

| Riesgo | Mitigación |
| --- | --- |
| Coordinator hostil entrega candidatos de una víctima («reflexión») | Solo para pares de la propia configuración; ≤8 candidatos, ≤12 sondeos de 146 bytes por candidato y ≤1 intento cada 30 s (backoff); las cookies no amplían el presupuesto. Residual aceptado: tráfico acotado hacia terceros |
| Coordinator hostil desvía a un par | Imposible sin las claves: el handshake falla (IK con la clave del par de la configuración) |
| Un tercero obtiene candidatos de un dispositivo | `rendezvous` exige firma del dispositivo y autorización mutua; respuesta uniforme `peer_unavailable` |
| Mensaje 1 repetido desde otra IP redirige el tráfico del respondedor | El endpoint solo se confirma con un paquete autenticado de la sesión (§4) |
| Amplificación/reflexión por STUN | Respuesta ≤ 2,2× la petición, límite por IP de origen |
| Agotamiento del coordinator (long-polls, presencias, anuncios con configuraciones grandes) | Límites de §3; presencia con TTL, nonces y concurrencia con cuotas por dispositivo/red y tablas acotadas; las pruebas `device_nonce_quota_preserves_other_network`, `network_nonce_quota_preserves_other_network`, `device_poll_quota_preserves_other_network_and_cancellation` y `network_poll_quota_preserves_other_network` verifican aislamiento y recuperación de capacidad |
| Dispositivo revocado sigue anunciándose | Su configuración caduca (≤ 300 + 30 s, ADR-002) y la presencia caduca con ella |
| Presencia/ubicación de dispositivos observable por el coordinator | No se oculta (E2E no oculta metadatos, T15) |

## 7. Verificación exigida

- **Pruebas del coordinator:** autenticación (firma, desfase, repetición), verificación de configuración (válida, caducada, de otra raíz, de otra red, `device_id` distinto), presencia con TTL y reemplazo, autorización mutua de `rendezvous` (unilateral y no autorizado devuelven lo mismo que inexistente), eventos (long-poll, caducidad, cola acotada), límites de tasa, STUN (formato, límite por origen, no responde a basura) y barrido de caducadas.
- **Laboratorio de NAT (netns + nftables):** sin NAT, NAT doméstico (mapeo independiente), doble NAT, mapeo dependiente del destino (simétrico, sin P2P), IPv6 con cortafuegos con estado, hairpin disponible y no disponible, cambio de red de un agente. Para cada caso, el estado informa cuándo hay `direct` y cuándo no, con integridad de datos; dispositivos fantasma ausentes tras el TTL.
- **Regresiones:** laboratorios de fases 2, 3 y 3B sin cambios de resultado.

## 8. Fuera de alcance de la fase 4

Relays UDP y TLS/TCP y conmutación de caminos (fase 5); distribución de coordinators por configuración firmada (v2); clasificación del filtrado (RFC 5780); ICE completo con prioridades y nominación; UPnP/NAT-PMP/PCP; alta disponibilidad del coordinator.

## 9. Implementación y precisiones (fase 4)

Implementación: `crates/nat-traversal` (STUN y candidatos), `apps/coordinator` (HTTP y STUN), `crates/identity::inspect_config` (verificación sin estado), `apps/agent` (`net.rs`, `coord.rs`, sondeo y keepalive en `engine.rs`). Resultados: [`e2e-fase4`](../operations/e2e-fase4.md).

- `POST /v1/rendezvous` sin presencia vigente devuelve `409 not_announced` solo si la cabecera es válida en formato y tiempo (sin presencia no hay clave para comprobar la firma); una cabecera inválida o una firma incorrecta con presencia devuelven `401`. El coordinator acepta cualquier red firmada por un ancla permitida; las redes distintas se separan en la autorización mutua, no en `announce`.
- `inspect_config` no compara el `device_id` de los selectores de reglas con una identidad local: una configuración con un selector hacia un dispositivo ajeno es `InvalidStructure` (paso 14), no `WrongDevice`.
- El parseo STUN valida la estructura TLV de todos los atributos, también en peticiones.
- La recuperación tras un cambio de red del propio agente tarda ≈ 45 s (`PATH_DEAD_AFTER`); detectar el cambio de interfaz para avisar al par antes queda como mejora.
- Este diseño sigue **sin** distribuir coordinators por configuración firmada (v2).

## 10. Relay (fase 5)

Contrato normativo en [relay-v2](relay-v2.md), que sustituye v1 sin compatibilidad. `/v1` HTTP y `AtDevice v1` no cambian por la versión de cable. El coordinator firma con clave propia (`RELAY_SIGNING_KEY_FILE`, semilla hex de 32 bytes, 0600; `--print-relay-key` imprime la pública).

- Cada entrada de `ATENEA_COORD_RELAYS` exige `relay_id` de 16 bytes (32 hex minúsculos, no cero), `name`, `udp` y/o `tls` y `tls_name` si hay TLS. Máximo cuatro entradas, IDs/nombres únicos; datos inválidos impiden arrancar. El operador configura el mismo ID en `ATENEA_RELAY_ID` del relay. No derivarlo de un nombre o IP mutable.
- `POST /v1/relay` `{"peer":"<32 hex>"}` (firmada, presencia vigente, autorización mutua) → `{"relays":[{"relay_id":"<32 hex>","name":"…","udp":"host:443","tls":"host:443","tls_name":"…","token":"<base64 de 189 bytes>"}],"ttl_secs":300}`. `udp`/`tls` son opcionales según la entrada configurada. **No existe token raíz compartido**: cada token autoriza solicitante→par exclusivamente en el relay de su entrada.
- Token binario: `AORT[4] || version=2[1] || relay_id[16] || network_id[16] || src[16] || dst[16] || src_identity_pub[32] || not_before u64 || not_after u64 || token_id[8] || sig[64]`. Dominio de firma `"atenea-one/relay-token/v2\0"` sobre los primeros 125 bytes. Vida por defecto 300 s, máximo 600 s, tolerancia ±30 s. No cambia ningún formato de configuración firmada.
- Evento al par: `{"kind":"relay","peer":"<solicitante>","relays":[…],"ttl_secs":300,"seq":N}` con un token inverso par→solicitante por cada relay_id, en la cola existente (16 eventos, TTL30 s). `src_identity_pub` de cada dirección procede de su propia presencia vigente; token_id aleatorio independiente por token.
- Límites: una emisión por 5 s/par, dos emisiones por minuto/par y diez solicitudes por dispositivo/minuto. Una emisión produce 2×R tokens con R≤4. El antiguo máximo de cuatro tokens se interpreta ahora como cuatro **concesiones direccionales** por par/minuto (≤16 tokens de audiencia), no cuatro firmas totales ni cuatro por relay sin límite global.
- Errores uniformes: 401 invalid_signature, 409 not_announced, 404 peer_unavailable, 429 rate_limited; sin firma configurada o sin relays: 503 relay_unavailable. El cliente rechaza JSON antiguo/incoherente como error de protocolo, no usa un token en varios relays. Relay/TCP añade WrongRelay(9), UnsupportedVersion(10), Malformed(11), definidos en relay-v2; no son códigos HTTP.
- El coordinator no es confiable para integridad/confidencialidad E2E: sus tokens autorizan recursos de relay, no descifran Noise. La audiencia evita reutilización entre IDs de relay diferentes, no retransmisiones de ALLOC dentro de la misma audiencia y frescura.

### Cliente del agente: concesiones por audiencia

`coord.rs` deserializa `relay_id` como 16 bytes desde hex minúsculo no cero y
exige `token` en cada endpoint. Rechaza listas vacías, de más de cuatro entradas,
IDs repetidos, tokens que no sean v2 o cuya audiencia no coincida con su entrada;
no trunca una lista excesiva. `vet_relays` conserva el plazo DNS de tres segundos,
los filtros de destinos y la conexión posterior a una IP literal ya verificada.

`RelayClient::open(peer, grants)` no recibe token global. Antes de conectar,
comprueba red, dispositivos origen/destino, audiencia y pública de identidad
local, además de estructura y vigencia. La comprobación no autentica al
coordinator: la firma del token se sigue verificando en el servidor relay.
Cada candidato usa su propio token para UDP y TLS y para renovar; retirar su
audiencia de una concesión actualizada cierra la asignación. La cola de salida
conserva el ID para no reenviar un INIT con cookie de otra audiencia al conmutar.

Los eventos `Ready`, `Down`, `Rx` y `Rtt` entregan `relay_id: [u8;16]` junto con
par/transporte al engine; el estado IPC expone el ID en hex, sin token ni cookie.
Se mantienen tres intentos UDP de un segundo, ALPN exclusivo `/2`, renovación
cada 60 segundos y PING cada cinco segundos. Las pruebas internas cubren
incoherencias de concesión, fallback entre audiencias, COOKIE de otro ID/origen,
prueba de posesión v2, renovación y propagación del ID; su ejecución corresponde
a la integración, no se afirma verificación por esta descripción.
