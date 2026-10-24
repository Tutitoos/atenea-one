# Coordinator y conexión directa v1 (fase 4)

- **Estado:** Implementado y verificado en laboratorio (ver §9 y [resultados](../operations/e2e-fase4.md)); pendiente de aceptación formal y de la revisión criptográfica externa.
- **Fecha:** 2026-10-24
- **Relacionados:** ADR-001 (Noise IK), ADR-003 (sesión y roaming), ADR-004 (puertos), `docs/protocol/signed-config-v1.md`, `docs/api/control-plane-v1.md` §3.3 (autenticación de dispositivo), modelo de amenazas T3, T9 y T10.

## 1. Principios

- **Un endpoint anunciado no es una ruta autenticada.** Los candidatos que llegan de STUN, del coordinator o de la configuración son **pistas**. Una ruta directa existe solo cuando un paquete Noise **autenticado** (respuesta de handshake o datos) llega desde ese endpoint. El agente nunca envía tráfico significativo a un candidato sin validar el retorno.
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

**Límites.** Tamaño de cuerpo 160 KiB; hasta 4096 presencias y 256 long-polls simultáneos (configurable); `announce` ≤ 1 por 2 s por dispositivo; `rendezvous` ≤ 1 por segundo por par y ≤ 10 por minuto por dispositivo; sobrepasar: `429`.

## 4. Comportamiento del agente

Configuración local nueva (TOML, sin firmar):

```toml
[coordinator]
url = "http://host:8081"          # base HTTP(S) del coordinator
stun = ["host:3478", "host:3479"] # dos puertos del mismo servidor, en este orden
```

- **Candidatos propios:** direcciones de las interfaces con el puerto de escucha (excluye loopback, enlace local y no enrutables), el endpoint reflexivo de STUN y, si lo hay, los endpoints de la configuración firmada para los pares. Se recalculan cada 30 s o ante cambio de interfaz; un cambio dispara un `announce` inmediato.
- **Presencia:** `announce` cada 20 s (TTL 60 s) mientras haya configuración vigente; sin lease no se anuncia.
- **Aviso de pares:** long-poll continuo a `/v1/events`. Un evento solo se atiende si `peer` figura en la configuración vigente; si no, se ignora y se cuenta.
- **Iniciar una conexión** (hay tráfico hacia un par sin ruta directa confirmada): `rendezvous` (≤ 1 cada 5 s por par). Con los candidatos del par más los endpoints de la configuración, se inicia el sondeo.
- **Sondeo (hole punching):** se envía el **mismo mensaje 1 de Noise** a cada candidato (≤ 8 más los configurados) cada ~1 s con jitter ±25 %, durante ≤ 10 s por intento (≤ 12 envíos por candidato). Ambos lados sondean a la vez al recibir el aviso. Si ambos inician, gana la clave pública menor (ADR-001). Al fallar un intento se espera 30 s (con retardo creciente hasta 5 min) antes de otro `rendezvous`; el estado del par es `probing` y luego `none` (a la espera del relay de la fase 5).
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
| Coordinator hostil entrega candidatos de una víctima ("reflexión") | Solo para pares de la propia configuración; ≤ 8 candidatos, ≤ 12 sondeos de 118 bytes por candidato y ≤ 1 intento cada 30 s (backoff): ≈ 1 paquete/s por candidato durante 10 s. Residual aceptado: tráfico acotado hacia terceros |
| Coordinator hostil desvía a un par | Imposible sin las claves: el handshake falla (IK con la clave del par de la configuración) |
| Un tercero obtiene candidatos de un dispositivo | `rendezvous` exige firma del dispositivo y autorización mutua; respuesta uniforme `peer_unavailable` |
| Mensaje 1 repetido desde otra IP redirige el tráfico del respondedor | El endpoint solo se confirma con un paquete autenticado de la sesión (§4) |
| Amplificación/reflexión por STUN | Respuesta ≤ 2,2× la petición, límite por IP de origen |
| Agotamiento del coordinator (long-polls, presencias, anuncios con configuraciones grandes) | Límites de §3; presencia con TTL y tablas acotadas |
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
