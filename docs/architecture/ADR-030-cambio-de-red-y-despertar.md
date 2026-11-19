<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-030: Cambio de red y despertar del equipo

- **Estado:** **Aceptado** por el responsable el 2026-11-19 («haz lo de Background y lo del cambio de red y despertar»). **Implementado en `main`, sin publicar ni desplegar.**
- **Relacionados:** [ADR-002](ADR-002-revocacion-lease.md) (el lease caduca por reloj de pared), [relay-v2](../protocol/relay-v2.md) §7 (conmutación de caminos), [diagnóstico de reconexión](../operations/diagnostico-reconexion-y-ciclo-relay.md).

## Contexto

Hechos medidos y leídos en el código de la 0.1.34:

- El agente **no escucha** ningún cambio de red ni de suspensión: solo temporizadores. Recuperar un par tras cambiar de wifi o despertar tardaba lo que los plazos acumulados: 45 s para dar por muerta la ruta directa (`PATH_DEAD_AFTER`), configuración cada 60 s, una espera larga (de 30 a 300 s) entre sondeos tras uno fallido, y una petición de eventos al coordinator que esperaba hasta 35 s por una conexión que ya no existía. `docs/operations/e2e-fase4.md` midió 47 s para un cambio de red y ya decía que «no se detecta el cambio de interfaz».
- `Instant` no cuenta el tiempo suspendido en macOS ni en Linux. Tras un sueño largo el agente creía tener sesiones jóvenes que el par ya había dado por caducadas.
- El servicio de cada red en macOS se instalaba con `ProcessType = Background` (desde el 2026-10-28). Con el Mac cargado, macOS frena un proceso así y el agente tardaba más que sus propios plazos de red (5 s de conexión, 15 s en total) y que los de la app (5 s), y mostraba «error de red» y «tiempo de espera agotado» sin que el servidor tuviera problema. Medido: `atenea network list` tardó de 15 a 32 s con carga 280, y de 6,7 a 8 s con carga ~80. La relación causal entre la prioridad y el retraso **no está demostrada** (en una medición posterior el agente contestaba en 0,11 s mientras otra petición fallaba); la prioridad de fondo no aporta nada a un servicio de red y quitarla es de riesgo mínimo.

## Decisión

### D1. Servicio de macOS sin prioridad de fondo

El plist de cada red deja de llevar `ProcessType`. El `.pkg` quita `ProcessType = Background` de los plists de red ya instalados antes de recargar el servicio (`normal_priority`, en `atenea-network.sh`); cualquier otro valor lo puso el administrador y no se toca.

### D2. Despertar y cambio de red, por muestreo

Una vez por segundo, dentro del bucle del agente, sin tarea nueva, dependencias ni `unsafe` (`netwatch.rs`):

- **Despertar:** el reloj de pared avanzó al menos 5 s más que el monótono. Un reloj que retrocede no cuenta.
- **Cambio de red:** cambió la dirección de origen que el sistema usaría hacia `192.0.2.1` o `2001:db8::1` (documentación: `connect` en UDP solo consulta la tabla de rutas, no envía nada). Quedarse sin ninguna ruta no es un evento; volver a tenerla sí, aunque sea la misma dirección.

**Límite:** dos redes distintas que dan la misma dirección privada (dos `192.168.1.x` iguales) no se distinguen; queda el despertar y los plazos de siempre. En Windows no se ha comprobado que `Instant` excluya el tiempo suspendido: el cambio de red funciona igual, el despertar puede no detectarse.

### D3. Qué se reinicia

`Engine::on_network_change` equivale a cumplir de golpe `PATH_DEAD_AFTER` en todos los pares:

- Ruta directa confirmada, sondeos y handshakes en curso: se descartan. Retroceso de sondeo, esperas de rendezvous y de relay: a cero.
- Asignaciones de relay listas: se cierran (salen de la red anterior) y el par las pide de nuevo si tenía tráfico reciente (con las cuotas de siempre: no se resetean).
- Solo los pares con tráfico en los últimos 15 s piden pistas y relay ya; los demás lo harán con su primer paquete (17 pares no consultan al coordinator por un cambio de wifi).
- **Sesiones:** se conservan en un cambio de red (no dependen de la dirección) y se descartan tras una suspensión de 120 s o más (`REKEY_AFTER_TIME`).
- Fuera del motor: STUN y anuncio de presencia ya, configuración (y lease) ya, y la petición de eventos en curso se abandona y se abre otra desde la red nueva.

### Lo que no cambia

Ningún formato firmado, protocolo ni mensaje. Un agente 0.1.34 sigue siendo compatible con uno de esta versión en ambos sentidos. Los plazos de `PATH_DEAD_AFTER` y la sospecha de camino siguen ahí para cuando no se detecte el evento.

## Verificación

Pruebas del motor (red nueva: retroceso a cero y directo recuperado con el primer paquete; sesiones solo tras suspensión larga; relays cerrados y peticiones solo de pares activos) y de `netwatch` (despertar, reloj que retrocede, proceso con retraso, cambio por familia, sin red y vuelta). **No comprobado:** una suspensión real del equipo, un cambio de wifi real y la reconexión medida de extremo a extremo; la lógica se ha probado con relojes y rutas inyectados. Pendiente medirlo en el piloto antes de dar por buena la mejora.
