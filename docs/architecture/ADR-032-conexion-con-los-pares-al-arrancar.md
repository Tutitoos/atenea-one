<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-032: Conexión con los pares al arrancar el agente

- **Estado:** **Aceptado** por el responsable el 2026-11-20 («haz el ADR y arregla el agente para que inicie la conexión con los pares al arrancar»). **Implementado en una rama, sin publicar ni desplegar.**
- **Relacionados:** [ADR-030](ADR-030-cambio-de-red-y-despertar.md) (reinicio de caminos y esperas), [ADR-031](ADR-031-conexiones-automaticas-por-aprobacion.md) (hasta 499 vecinos, sin un túnel activo con cada uno en reposo), [ADR-016](ADR-016-politicas-de-acceso-minimas.md) (un par solo aparece en la configuración si una regla lo relaciona con este dispositivo).

## Contexto

Hechos medidos el 2026-11-20 con un Windows (agente 0.1.35, red por cable) y un Mac, en la red `4mans-net`, reiniciando el Windows. Tiempos desde el arranque del kernel hasta que el ping Mac → Windows responde:

| Prueba | Tiempo |
| --- | ---: |
| Pings iniciados desde el Windows en cuanto tuvo adaptador | 36 s |
| Sin pings desde el Windows, intento 1 | ~152 s |
| Sin pings desde el Windows, intento 2 | 90 s |

En las tres el servicio de Windows estaba en marcha y con su primera configuración aplicada en unos 30 s (tras cambiar el servicio de «automático retrasado» a arranque por llegada de IP; con el modo retrasado eran ~142 s). El tiempo restante es el del camino con el Mac.

Leído en el código de la 0.1.35 (`apps/agent/src/engine.rs`): un par solo se activa con tráfico. Un paquete de la TUN hacia él crea el handshake (`on_tun` → `maybe_initiate`); sin tráfico reciente (`ACTIVE_WINDOW`, 15 s) el motor no inicia handshake, no pide rendezvous ni relay (comentarios y prueba `a_network_change_closes_the_relay_allocations_and_only_active_peers_ask_for_a_new_one`). Un agente recién arrancado y ocioso no inicia la conexión con nadie. El otro extremo sí lo intenta si tiene tráfico hacia él, pero tras un sondeo fallido espera de 30 a 300 s (`backoff`) y el aviso del coordinator levanta esa espera como mucho cada 30 s (`HINT_LIFT_INTERVAL`).

**Límite de la evidencia:** son tres medidas y la causa del retraso del otro extremo **no está demostrada**; la hipótesis es que el retroceso del par que sondea al apagado explica la espera. Lo que sí es cierto en el código es que el agente que arranca no inicia nada por sí mismo.

## Decisión

### D1. Calentamiento una vez por arranque

La primera vez que el motor tiene una configuración con lease vigente y pares (el primer `on_tick` que cumple ambas cosas), trata a los pares elegidos como si acabaran de recibir tráfico: marca actividad reciente, pide rendezvous si no hay ruta confirmada e inicia el handshake con `maybe_initiate`. Es el mismo camino que el primer paquete: mismos destinos directos, mismo sondeo, mismo relay y mismas cuotas. No hay mensaje, formato ni estado persistido nuevos. Ocurre **una vez por proceso**.

### D2. Tope de pares

Se calientan como máximo `WARMUP_MAX_PEERS` = 16 pares, en el orden de la configuración firmada. El resto sigue activándose con su primer paquete. El tope existe por ADR-031: con 499 vecinos, arrancar no puede abrir 499 handshakes ni pedir 499 relays. En redes con más de 16 pares la mejora es parcial: el orden de la configuración no refleja quién importa.

### D3. Un sondeo de arranque fallido no penaliza

El sondeo de arranque es especulativo. Si termina sin éxito no cuenta como fallo (`probe_failures`) ni activa el retroceso de 30 a 300 s, y el par vuelve a reposo; se anota en el contador `warm_probe_failed`. Así un par apagado al arrancar no retrasa su primer paquete real, que sigue intentando de inmediato. Una sesión establecida o el primer paquete real de la TUN quitan la marca de especulativo.

### D4. Coste

Por arranque y por par calentado: un handshake con su sondeo (≤ 10 s) y, si no hay camino directo, la petición de rendezvous y relay que ya haría el primer paquete (con sus cuotas). Si el par responde, la sesión se comporta como cualquier otra: keepalives mientras dura y reposo cuando caduca sin tráfico. En el peor caso son 16 sondeos y 16 peticiones por arranque del agente. El par destino recibe un mensaje 1 de un dispositivo autorizado y lo atiende con sus límites de siempre.

### Lo que no cambia

Ningún formato firmado, protocolo ni mensaje. Un agente 0.1.35 es compatible con uno de esta versión en ambos sentidos. El cambio de red y el despertar (ADR-030) siguen reiniciando solo los pares con tráfico reciente.

## Alternativas descartadas

- **Calentar todos los pares:** incompatible con ADR-031 (499 vecinos) y sin valor para los apagados.
- **Persistir los pares activos entre arranques y calentar solo esos:** es el criterio correcto para redes grandes, pero añade estado en disco y su caducidad. Fuera de este cambio; queda anotado como mejora para cuando existan redes con más de 16 pares.
- **Levantar el retroceso del otro extremo cuando el coordinator avisa de que el par volvió:** actúa sobre la causa sospechosa, pero depende de lo que informe el coordinator y de una hipótesis sin demostrar. No se toca.

## Verificación

Pruebas del motor con relojes y red simulados: el arranque conecta sin tráfico y entrega sesión a ambos lados, ocurre una sola vez y solo con lease vigente, respeta el tope de 16, y un sondeo de arranque fallido no retiene el primer paquete real.

**Medido en un Windows real (2026-11-20, `4mans-net`, sin tráfico iniciado a mano, mismo equipo y mismo Mac).** Segundos desde el arranque del kernel hasta que responde el ping Mac → Windows, reiniciando el Windows. «App» es el arranque automático de la aplicación de escritorio (~35 s tras el arranque):

| Agente | App | Medidas (s) |
| --- | --- | --- |
| 0.1.35 | sin arranque | 36, 36 |
| 0.1.35 | con arranque | 61, 88, y en mediciones anteriores 90 y ~152 |
| Esta ADR (`9cc3280`) | sin arranque | 36, 37 |
| Esta ADR (`9cc3280`) | con arranque | 36, 37 |
| Esta ADR (`366d865`, antes de añadir contadores) | con arranque | 35, 36, 37 |

Con el agente nuevo el contador `warm_up` vale 1 y ningún `warm_skip_*` aparece en las cuatro medidas de `9cc3280`. Con 0.1.35 sin la app ya vuelve rápido; el retraso solo aparece con la app arrancando sola, y el calentamiento lo evita. Los dos agentes difieren en este cambio (el código entre `0.1.35` y la rama solo cambia el motor del agente), así que la diferencia se atribuye al calentamiento.

**Sin explicar:** por qué la app retrasa al agente anterior (el registro del agente no muestra nada a los ~35 s) y por qué en las tres medidas de `366d865` el contador `warm_up` no aparecía y aun así la conexión fue rápida. Muestras pequeñas: dos a cuatro por condición, en un solo equipo.
