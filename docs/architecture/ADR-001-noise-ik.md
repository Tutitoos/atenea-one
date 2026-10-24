# ADR-001 — Patrón de handshake: Noise IK

- **Estado:** Aceptado (2026-10-24, por el responsable del proyecto)
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

1. **Prólogo** (autenticado por Noise): `"atenea-one/v1" || network_id`. Enlaza el handshake a la versión de protocolo y a la red; un peer de otra red no completa el handshake.
2. **Payloads**: mensaje 1 lleva `sender_index (u32)` y un `timestamp` (segundos Unix, u64). Mensaje 2 lleva `sender_index (u32)`. **No se envían datos de aplicación en los mensajes de handshake.**
3. **Autorización antes de recursos**: tras descifrar el mensaje 1, el respondedor busca la clave estática del iniciador en su configuración vigente. Si no está, o la configuración ha caducado, **descarta sin responder**. No hay mensaje de error que sirva de oráculo.
4. **Frescura**: se rechaza un mensaje 1 cuyo `timestamp` difiera del reloj local más de la tolerancia de ADR-002 (30 s) o no sea mayor que el último aceptado de ese peer (en memoria).
5. **Iniciación simultánea**: si ambos lados inician a la vez, gana como iniciador el que tenga la clave estática pública menor (comparación lexicográfica); el otro descarta su intento y responde.
6. **Cambio de clave estática** de un peer = nueva configuración firmada. Un handshake con una clave obsoleta falla en el descifrado de `es` y no revela nada; el agente lo registra como "peer con clave no vigente".
7. Sin PSK en v1 y sin criptografía postcuántica; ambos se registran como fuera de alcance.

## Alternativas

| Opción | Motivo del descarte |
| --- | --- |
| **XX** | Transmite ambas claves estáticas durante el handshake y necesita un paso adicional de autenticación fuera de Noise. Aporta valor solo si las claves no se conocen de antemano; aquí la configuración firmada ya las entrega. Añade un viaje de ida y vuelta. |
| **IK + PSK (IKpsk2)** | Un PSK por peer o por red añade distribución y rotación de secretos sin resolver un problema actual. Se reevalúa si se exige resistencia postcuántica por PSK. |
| **XK** | Oculta mejor la clave del iniciador pero exige 3 mensajes; sin ventaja con claves ya autenticadas. |

## Consecuencias

- Handshake de 1 RTT; el iniciador puede enviar datos tras recibir el mensaje 2.
- El mensaje 1 no tiene confidencialidad hacia adelante completa respecto a la clave estática del respondedor (spec Noise §7.7), de ahí la regla de no enviar datos de aplicación en él.
- El respondedor hace operaciones DH por cada mensaje 1 con clave conocida: hace falta limitación de tasa por origen antes de la fase 4 (relay/coordinator exponen el puerto). El diseño de cookie/MAC anti-DoS queda como trabajo de la fase 2 si las pruebas de abuso lo exigen.
- La identidad del respondedor se revela a quien conozca su clave estática pública; no se pretende ocultar quién habla con quién frente al relay (ya documentado: E2E no oculta IPs, tamaños ni tiempos).

## Verificación

Pruebas obligatorias en fase 2: clave estática del respondedor equivocada rechazada; iniciador desconocido descartado sin respuesta; mensaje 1 repetido fuera de la ventana de frescura rechazado; mensajes alterados rechazados; prólogo distinto (otra red/versión) no completa; iniciación simultánea converge a una sola sesión.

La revisión criptográfica externa (puerta de la fase 2) incluye este ADR.

## Pendiente de verificar al fijar versiones

Que `snow 0.10.0` (última observada en crates.io) ofrezca el patrón con su resolver por defecto y la conversión `HandshakeState → StatelessTransportState` (`TryFrom`), según su documentación. No se ha compilado nada todavía.
