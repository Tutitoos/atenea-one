# ADR-003 — Transporte cifrado con snow, antirreplay y rekey propios

- **Estado:** Aceptado (2026-10-24, por el responsable del proyecto). Los límites de rekey y MTU siguen siendo parámetros a medir en la fase 2.
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

### Formato de datagrama v1 (orden de red, big-endian)

| Tipo | Campos |
| --- | --- |
| Handshake init (1) | `version(1) type(1) sender_index(4) noise_msg1` |
| Handshake resp (2) | `version(1) type(1) sender_index(4) receiver_index(4) noise_msg2` |
| Datos (3) | `version(1) type(1) receiver_index(4) counter(8) ciphertext+tag(16)` |

- Keepalive = datos con payload vacío. Sobrecarga de datos: 30 bytes.
- Versión o tipo desconocidos: descartar sin responder.
- Tamaño máximo de datagrama = MTU del túnel + 30. MTU por defecto del túnel: 1400 (conservador, 1500 − UDP/IPv6 − cabecera − margen); se mide en la fase 2. Mayor = descartar.
- Sin compresión y sin relleno en v1 (el relleno no se promete; los tamaños son visibles).
- Como snow no autentica cabecera: `receiver_index` selecciona la clave (alterarlo da fallo de autenticación) y `counter` es el nonce AEAD (alterarlo también). `version` y `type` solo sirven para despachar. Esto es un punto de revisión explícito para la puerta criptográfica.

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
- **Roaming:** el endpoint de un peer solo se actualiza tras un paquete que **autentica**. La validación de retorno y los límites de amplificación se detallan en la fase 4.

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

Duplicado, reordenado y fuera de ventana descartados; paquete alterado o con `receiver_index` falso no mueve la ventana; tras reinicio no se reutiliza (clave, nonce); cambio de camino sin romper la sesión ni aceptar replays; envío bloqueado en el límite duro; fuzzing del parser de datagramas y de la máquina de estados del handshake; tests de propiedades sobre la ventana.
