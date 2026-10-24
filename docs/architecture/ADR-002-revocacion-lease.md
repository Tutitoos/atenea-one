# ADR-002 — Revocación mediante leases de configuración

- **Estado:** Propuesto (recomendación; pendiente de aceptación)
- **Fecha:** 2026-10-24
- **Debe resolverse antes de:** fase 3
- **Relacionados:** ADR-001, ADR-005

## Contexto

MVP-0 no tiene control plane online ni lista de revocación consultable. La autorización es la configuración firmada que cada agente aplica (ADR-005). Hay que fijar cuánto tarda en cortarse un dispositivo revocado y qué hace un agente cuando no puede renovar su configuración.

## Decisión

**Revocar = dejar de emitir.** Cada configuración firmada es un lease:

| Parámetro | Valor propuesto | Notas |
| --- | --- | --- |
| Validez (`not_after − not_before`) | 300 s | Parámetro; se mide y se ratifica antes del piloto |
| Cadencia de reemisión | 60 s | Cada reemisión sube `version` (monótona) |
| Tolerancia de reloj | 30 s | Se acepta `now ≥ not_before − 30 s` y `now < not_after + 30 s` |

Comportamiento del agente:

1. **Fail-closed.** Al expirar el lease (más tolerancia) descarta todo el tráfico del túnel y destruye las sesiones. La interfaz y el proceso siguen vivos y reanudan al cargar una configuración válida (nuevo handshake).
2. **Sin configuración válida al arrancar** = tráfico denegado.
3. **Protección contra rollback:** el agente persiste (0600, directorio de estado del servicio) el mayor `version`, el mayor `issuer_serial` (ADR-005) y la mayor hora observada. Rechaza configuraciones con `version` o `issuer_serial` menores, y trata un reloj anterior a esa hora observada menos la tolerancia como anomalía: deniega hasta que se corrija.
4. **Al cargar una configuración nueva**, las sesiones con peers que ya no figuran en ella (o cuya clave cambió) se destruyen de inmediato.

### Latencia de revocación resultante

Con el emisor detenido para el dispositivo D:

- **Peers honestos de D** dejan de aceptarlo cuando cargan su siguiente configuración, sin D: **≤ 60 s** (cadencia) más el tiempo de carga, si alcanzan al emisor; si no, cuando caduque su último lease: **≤ 330 s**.
- **El agente de D** se corta solo a los **≤ 330 s** (lease + tolerancia) desde la última configuración que recibió. Esto solo se usa como garantía para un agente honesto. Un D hostil puede ignorar su propio lease; lo que lo contiene son sus peers.

Consecuencias de la disponibilidad: si el emisor está caído más de ~5 min, **todos los túneles caen**. Es el precio de revocar sin canal en vivo. Para el piloto se puede subir la validez (p. ej. 1 h) con revocación peor (hasta 1 h 30 s a peers aislados del emisor); se decide con las mediciones.

## Alternativas

| Opción | Motivo |
| --- | --- |
| Lista de revocación en vivo consultada por agentes/relay/coordinator | Añade dependencia de un servicio online alcanzable por todos y obliga a decidir fail-open o fail-closed si no responde, que acaba siendo el mismo lease. Se reevalúa en la fase 3B. |
| Leases largos (24 h) | Revocación inaceptable para el objetivo de "revocación verificable". |
| Avisos de revocación firmados empujados por coordinator | Compatible: reduce la latencia; el lease sigue siendo el respaldo. Candidato de fase 3B/4. |

## Consecuencias

- La revocación nunca es inmediata; la garantía es "≤ cadencia para peers que alcanzan al emisor, ≤ lease + tolerancia en el peor caso".
- El emisor es crítico para disponibilidad. En MVP-0 es un temporizador de laboratorio; en 3B es el control plane.
- El reloj de los dispositivos es parte de la seguridad: se documenta y se exige NTP; la anomalía de reloj se trata como fallo cerrado.

## Verificación (fase 3)

Medir y registrar con y sin conectividad al emisor: tiempo de corte de D visto por sus peers y por D; configuración caducada rechazada; reloj atrasado/adelantado fuera de tolerancia; rollback de `version` rechazado; reinicio del agente con estado persistido conserva las protecciones; emisor caído 6 min tumba los túneles y se recuperan al volver.
