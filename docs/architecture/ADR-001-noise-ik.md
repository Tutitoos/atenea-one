# ADR-001 — Patrón de handshake: Noise IK

- **Estado:** Aceptado (2026-10-24); evolución de cable v2 con MAC1/MAC2 aceptada por el responsable el 2026-10-25, pendiente de implementación y auditoría.
- **Fecha:** 2026-10-24
- **Debe resolverse antes de:** fase 2
- **Relacionados:** ADR-002 (configuración firmada), ADR-003 (transporte y antirreplay)

## Contexto

Cada dispositivo recibe una configuración firmada (ADR-002/ADR-005) que enlaza su identidad con una clave estática X25519 y lista las claves estáticas de sus peers autorizados. Por tanto, **antes de iniciar un handshake el iniciador ya conoce y ha autenticado la clave estática del respondedor**, y el respondedor puede decidir si acepta a un iniciador comparando su clave estática con la lista de peers de su configuración vigente.

Los mensajes de handshake viajan por UDP directo, por el relay UDP o por TLS/TCP; el relay los trata como opacos.

## Decisión

Usar `Noise_IK_25519_ChaChaPoly_BLAKE2s`:

```
<- s
...
-> e, es, s, ss
<- e, ee, se
```

Reglas:

1. **Prólogo** (autenticado por Noise): `"atenea-one/v2" || network_id`. Enlaza el handshake a la versión de protocolo y a la red; un peer de otra red/versión no completa el handshake. [wire-v2](../protocol/wire-v2.md) fija cabecera, MAC1/MAC2 y cookie; v1 se descarta sin fallback.
2. **Payloads**: mensaje 1 lleva `sender_index (u32)` y un `timestamp` (segundos Unix, u64). Mensaje 2 lleva `sender_index (u32)`. **No se envían datos de aplicación en los mensajes de handshake.**
3. **Autorización antes de recursos**: tras descifrar el mensaje 1, el respondedor busca la clave estática del iniciador en su configuración vigente. Si no está, o la configuración ha caducado, **descarta sin responder**. No hay mensaje de error que sirva de oráculo.
4. **Frescura**: se rechaza un mensaje 1 cuyo `timestamp` difiera del reloj local más de la tolerancia de ADR-002 (30 s) o no sea mayor que el último aceptado de ese peer (en memoria).
5. **Iniciación simultánea**: si ambos lados inician a la vez, gana como iniciador el que tenga la clave estática pública menor (comparación lexicográfica); el otro descarta su intento y responde.
6. **Cambio de clave estática** de un peer = nueva configuración firmada. Un handshake con una clave obsoleta falla en el descifrado de `es` y no revela nada; el agente lo registra como "peer con clave no vigente".
7. Sin PSK ni criptografía postcuántica; ambos se registran como fuera de alcance.

## Alternativas

| Opción | Motivo del descarte |
| --- | --- |
| **XX** | Transmite ambas claves estáticas durante el handshake y necesita un paso adicional de autenticación fuera de Noise. Aporta valor solo si las claves no se conocen de antemano; aquí la configuración firmada ya las entrega. Añade un viaje de ida y vuelta. |
| **IK + PSK (IKpsk2)** | Un PSK por peer o por red añade distribución y rotación de secretos sin resolver un problema actual. Se reevalúa si se exige resistencia postcuántica por PSK. |
| **XK** | Oculta mejor la clave del iniciador pero exige 3 mensajes; sin ventaja con claves ya autenticadas. |

## Consecuencias

- Handshake de 1 RTT sin desafío; bajo carga la cookie añade un RTT antes del procesamiento Noise. El iniciador puede enviar datos tras recibir el mensaje 2.
- El mensaje 1 no tiene confidencialidad hacia adelante completa respecto a la clave estática del respondedor (spec Noise §7.7), de ahí la regla de no enviar datos de aplicación en él.
- Antes de DH se valida MAC1 sobre la envoltura completa del mensaje 1 y, bajo carga, MAC2 con cookie de retorno; construcción, rotación, origen directo/relay, respuesta XChaCha y cuotas en [wire-v2](../protocol/wire-v2.md). Después se conservan cupos por par configurado y por IP/IPv6 /64 más el cubo global de desconocidos. MAC1 usa una clave derivada de la pública del respondedor: no autentica al iniciador. La cookie no sustituye Noise ni garantiza disponibilidad frente a un emisor que puede recibirla.
- La identidad del respondedor se revela a quien conozca su clave estática pública; no se pretende ocultar quién habla con quién frente al relay (ya documentado: E2E no oculta IPs, tamaños ni tiempos).

## Verificación

Pruebas obligatorias: clave estática del respondedor equivocada rechazada; iniciador desconocido descartado sin RESP (bajo carga puede recibir un desafío de cookie antes de conocer su identidad); mensaje 1 repetido fuera de frescura rechazado; alteración y prólogo de otra red/versión rechazados; iniciación simultánea converge. Añadir vectores y pruebas pre-DH/cookie de wire-v2 §7 y cutover sin aceptación v1.

La revisión criptográfica externa (puerta de la fase 2) incluye este ADR.

## Primitivas disponibles y entrega pendiente

El árbol actual usa `snow 0.10.0`, `Noise_IK_25519_ChaChaPoly_BLAKE2s` y `StatelessTransportState`; `blake2` y `chacha20poly1305 0.10.1` están fijados en Cargo.lock. Este último ofrece XChaCha20Poly1305 para la respuesta de cookie sin añadir otro paquete. La evolución v2 debe publicar los vectores calculables y pasar revisión externa; la disponibilidad de las primitivas no verifica el diseño.
