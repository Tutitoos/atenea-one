# ADR-003 — Transporte cifrado con snow, antirreplay y rekey propios

- **Estado:** Aceptado (2026-10-24); cable v2 y relay-v2 aceptados por el responsable el 2026-10-25, pendientes de implementación. Los límites de rekey y MTU siguen siendo parámetros a medir.
- **Fecha:** 2026-10-24
- **Debe resolverse antes de:** fase 2
- **Relacionados:** ADR-001, ADR-002

## Contexto

Los datagramas llegan desordenados, duplicados o perdidos, y por varios caminos (P2P, relay UDP, TLS/TCP). `snow::StatelessTransportState` (documentación de `snow 0.10.0`) expone `write_message(nonce, payload, message)` y `read_message(nonce, payload, message)` con **nonce explícito**, además de `rekey_outgoing`, `rekey_incoming` y `rekey_manually`. No aporta política antirreplay ni gestión de sesiones, y su API **no admite datos asociados (AAD)**.

## Decisión

Usar `StatelessTransportState` tras el handshake de ADR-001 y **implementar en `crates/crypto` la política de sesión, antirreplay y rekey**. No se implementan primitivas propias.

### Sesión y época

- Cada handshake completado crea una **sesión** con `session_index (u32)` aleatorio elegido por cada lado para recibir. La **época** de un par de peers es la sesión vigente; una sesión nueva siempre parte de claves nuevas (efímeras frescas) y contador 0.
- Estado de sesión **solo en memoria**. Tras reiniciar el proceso no hay sesiones: se hace un handshake nuevo con efímeras nuevas. **No se persisten contadores** y por construcción no se reutiliza (clave, nonce).

### Formato de datagrama v2 (orden de red, big-endian)

Contrato normativo de bytes, validación pre-DH, límites, cookies y vectores en [wire-v2](../protocol/wire-v2.md); relay y sus tokens en [relay-v2](../protocol/relay-v2.md).

| Tipo | Campos | Bytes |
| --- | --- | --- |
| Handshake init (1) | `version=2(1) type(1) sender_index(4) noise_msg1(108) mac1(16) mac2(16)` | 146 |
| Handshake resp (2) | `version=2(1) type(1) sender_index(4) receiver_index(4) noise_msg2(52)` | 62 |
| Datos (3) | `version=2(1) type(1) receiver_index(4) counter(8) ciphertext+tag(16)` | payload + 30 |
| Cookie reply (4) | `version=2(1) type(1) receiver_index(4) nonce(24) encrypted_cookie+tag(32)` | 62 |

- INIT exige MAC1; bajo carga exige MAC2 o se devuelve una cookie cifrada sin DH. Una cookie no autentica ni confirma endpoint. MACs quedan fuera del transcript Noise; prólogo Noise pasa a `atenea-one/v2 || network_id`.
- Keepalive = datos con payload vacío. Sobrecarga DATA 30 bytes; MTU por defecto 1400, máximo absoluto 1530 bytes por datagrama. Sin compresión ni relleno.
- Versión o tipo desconocidos y v1: descartar sin responder. Relay/TCP informa incompatibilidad con ALPN `/2` o error definido en relay-v2 y cierre. Sin compatibilidad ni shims.
- Como snow no admite AAD: `receiver_index` selecciona la clave y `counter` es el nonce AEAD; alterarlos impide autenticar. `version` y `type` despachan, sin autenticación AEAD adicional. MAC1/MAC2 solo protegen INIT, no alteran esta limitación DATA.

### Nonce por dirección

- Las dos direcciones usan claves distintas (las dos `CipherState` de `Split()`), así que cada dirección tiene su propio espacio de nonce.
- El emisor obtiene el contador con un `AtomicU64::fetch_add`; nunca se reenvía un contador ya usado. Se rechaza enviar a partir del límite duro (abajo).
- El mismo contador y la misma sesión valen para **cualquier camino**: cambiar entre P2P, relay UDP y TLS/TCP no cambia claves ni reinicia contadores. Si se envía el mismo paquete por dos caminos, el receptor lo deduplica con la ventana.

### Ventana antirreplay

- Ventana deslizante por sesión y dirección de recepción, de 2048 contadores (bitmap).
- Orden obligatorio: (1) comprobar contador contra la ventana (solo lectura, rechazo barato), (2) **descifrar y autenticar**, (3) solo si autentica, **actualizar la ventana**. Un paquete falso nunca mueve la ventana.
- Contador más antiguo que `máximo − 2048` o ya visto: descartar. El acceso a la ventana está protegido por exclusión entre hilos.

### Rekey

- **Rekey = nuevo handshake**, no `rekey_*` de snow: da claves nuevas con confidencialidad hacia adelante y reinicia el contador sin reglas especiales. No se usa `rekey_manually`.
- Límites propuestos (parámetros de laboratorio; se miden en la fase 2):

| Límite | Valor |
| --- | --- |
| Rekey suave | 120 s o 2^32 mensajes |
| Rechazo de envío | 180 s o 2^40 mensajes (sesión inutilizable) |

- Durante la transición se conserva la sesión anterior solo para **recibir** hasta su rechazo duro. El respondedor no envía datos hasta recibir el primer paquete válido del iniciador (confirmación de sesión).
- **Roaming:** el endpoint solo se actualiza tras datos que autentican y pasan antirreplay, sin desafío adicional de retorno de la nueva dirección (comportamiento tipo WireGuard). Riesgo residual aceptado por el responsable el 2026-10-25; no se afirma validar posesión de la nueva dirección. La confirmación inicial de endpoint sigue exigiendo RESP autenticada o datos, nunca INIT ni cookie. Véase el modelo de amenazas, nota F7.

### Reinicio y clonación

- Una restauración de snapshot de VM o un fork del proceso puede duplicar el estado de sesión y reutilizar nonces. Se mitiga acotando la vida de sesión (180 s) y se documenta como riesgo para el agente; se pide revisión.

## Alternativas

| Opción | Motivo del descarte |
| --- | --- |
| `snow::TransportState` (contador implícito) | Asume entrega ordenada; no sirve para UDP con reordenación. |
| AEAD directo (`chacha20poly1305`) con claves exportadas de Noise | Obliga a sacar claves de snow y reimplementar la capa; más superficie propia sin ganancia. |
| Rekey con `rekey_outgoing/incoming` de Noise §4.2 | Cambia la clave pero no da claves nuevas con efímeras; exige sincronizar contadores por época a mano. |
| Persistir contadores | Estado que puede corromperse o retrocederse; la sesión efímera en memoria lo hace innecesario. |

## Consecuencias

- La corrección antirreplay y de nonce es **código propio** y el mayor riesgo criptográfico del producto: tests de propiedades, fuzzing del parser y revisión externa en la puerta de la fase 2.
- Cabecera sin AEAD y 30 bytes de sobrecarga son límites asumidos; se vuelven a evaluar si la revisión los rechaza.

## Verificación (fase 2)

Duplicado, reordenado y fuera de ventana descartados; paquete alterado o con `receiver_index` falso no mueve la ventana; reinicio sin reutilizar (clave, nonce); cambio de camino sin romper sesión ni aceptar replays; límites duros; fuzzing de datagramas y handshake; propiedades de ventana. Añadir tamaños/versión v2, vectores MAC/cookie, rotación, no DH antes del retorno bajo carga, no consumo de cupos DH por desafíos y no confirmación por cookie (wire-v2 §7). Son criterios exigidos, no una declaración de verificación de v2.
