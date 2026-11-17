---
title: Rendimiento
description: Qué se ha medido de Atenea One (túnel directo, relay, reconexión, estabilidad y escala del control plane), cómo y con qué límites.
---

Todo lo de esta página sale de `docs/operations/rendimiento-y-presupuestos.md` del repositorio. **No dimensiona un VPS.** Se midió en Docker Desktop sobre un Mac arm64, dentro de una VM Linux de 5 vCPU y unos 7,75 GiB de RAM, con namespaces, un generador Python y listeners loopback. No hay enlace WAN, proveedor ni hardware de producción. Una prueba funcional, una cuota del código o una CI verde no son medidas de capacidad.

## Cómo se mide

- Dos repeticiones por escenario: se publican ambas, la media y el peor caso. Para el caudal el peor caso es el mínimo; para coste, memoria, pérdida y latencia, el máximo.
- **TCP:** 512 MiB por transferencia, con SHA-256 en ambos extremos. La ventana incluye cliente, envío, recepción y digest, así que no mide solo el cifrado.
- **RTT:** 2000 ecos ICMP por repetición, con p50, p95 y p99 por rango más próximo. Sin túnel, el suelo orientativo es de 0,116 ms (p50), 0,219 ms (p95) y 0,406 ms (p99).
- **UDP:** 10 000 datagramas de 1200 bytes durante 5 s (19,2 Mbit/s ofrecidos). No busca saturación.
- CPU: 100 % equivale a un núcleo lógico; puede superarse con varios hilos. RSS es el muestreado, no un máximo continuo de heap.

## El túnel por camino

| Camino | TCP Mbit/s (media) | RTT p50 ms | p95 ms | p99 ms | Pérdida UDP |
| --- | ---: | ---: | ---: | ---: | ---: |
| Directo | 1009,8 | 0,552 | 0,947 | 1,530 | 0 % |
| Relay UDP | 21,649 | 0,760 | 1,565 | 3,245 | 0 % |
| Relay TLS 443 | 21,641 | 0,724 | 1,540 | 2,895 | 0 % |

En el camino directo, las dos transferencias de 512 MiB tardaron 4,285 s en el peor caso. Por relay, unos 198 s.

La meseta de unos 21,65 Mbit/s por relay es compatible con la cuota predeterminada de 24 Mbit/s de datagramas por asignación más la sobrecarga. **No es saturación de CPU ni autoriza a prometer 1 Gbit/s por una asignación de relay.**

### Recursos durante la transferencia TCP

| Camino / proceso | CPU % (peor) | Pico RSS KiB (peor) |
| --- | ---: | ---: |
| Directo / agente A | 118,9 | 6716 |
| Directo / agente B | 106,2 | 6848 |
| Relay UDP / agente A | 7,3 | 7536 |
| Relay UDP / relay | 4,8 | 4756 |
| Relay TLS / agente A | 9,9 | 7636 |
| Relay TLS / relay | 3,8 | 5600 |

## Reconexión tras reiniciar

Con 63 tiempos de primer eco en 21 experimentos (tres controles sin inundación más tres modos de abuso por tres tasas, dos repeticiones), hubo **0 fallos**.

| Par | Antes (14 controles históricos) | Después: media | Mediana | Máximo |
| --- | --- | ---: | ---: | ---: |
| a↔b | 0,05–20,58 s | 0,34 s | 0,065 s | 3,564 s |
| c↔b | 0,17–20,92 s | 0,60 s | 0,173 s | 4,675 s |
| a↔c (par nuevo) | 20,06–20,81 s | 5,13 s | 5,072 s | 5,317 s |

El residuo de unos 5 s en el par nuevo coincide con el paso de rendezvous del coordinator, un límite de diseño que no ha cambiado. **No es un A/B controlado**: las series antiguas proceden de sesiones y versiones de arnés distintas, y los tiempos incluyen convergencia y primer eco, no solo el handshake criptográfico. No son un SLA.

La parada del agente (`atenea down`) tardó menos de 3 s en 21 de 21 invocaciones (máximo 2,084 s).

## Estabilidad (soak de 12 minutos)

Un ensayo de 12 minutos con el stack completo y dos agentes, con una transferencia SHA-256 de 16 MiB por minuto:

- 12 de 12 transferencias íntegras, con una media de 24,99 Mbit/s.
- 3515 pings enviados y 3513 recibidos: pérdida del 0,057 %.
- RTT mínimo, medio y máximo de 0,233, 1,248 y 33,578 ms.
- Antes de una corrección, el relay se recreaba cada unos 300 s. Ahora el agente renueva la concesión por adelantado sin cortar la asignación: tres renovaciones correctas y ningún fallo ni estrangulamiento.

**No hay soak de 72 h.** Un ensayo de 12 minutos no lo sustituye, y tampoco descarta fugas lentas. Los extremos de memoria muestreados no crecieron más allá de los márgenes locales.

## Escala

- **Relay:** admite 4096 asignaciones simultáneas por defecto (1024 por red, 32 por origen); la 4097 se rechaza.
- **Coordinator:** hasta 10 000 presencias, con una configuración de ensayo distinta de la predeterminada de 4096.
- **Control plane:** 10 000 dispositivos repartidos en 79 redes (la API admite como máximo 256 dispositivos aprobados por red). Con 200 clientes y tras el índice `0005`, unas 788 lecturas/s (p95 ≈0,43 s) y unas 177 emisiones/s (p95 ≈1,41 s). No es un A/B aislado y solo se ha medido en el laboratorio.

## Presupuestos de regresión

Son **alarmas calibradas con lo medido**, con margen. No son promesas comerciales ni recomendaciones de CPU o RAM. Una regresión es incumplir cualquiera de las cotas de la fila.

| Caso | Presupuesto |
| --- | --- |
| Directo, TCP | ≥700 Mbit/s; 512 MiB en ≤7 s; cada agente ≤180 % CPU y ≤16 MiB RSS |
| Relay UDP, TCP | ≥18 Mbit/s; ≤240 s; agente ≤15 % CPU y relay ≤10 % |
| Relay TLS, TCP | ≥18 Mbit/s; ≤240 s; agente ≤18 % CPU y relay ≤9 % |
| Directo, 2000 ecos | p95 ≤2 ms; p99 ≤4 ms; pérdida ≤0,1 % |
| Relay (UDP y TLS), 2000 ecos | p95 ≤3 ms; p99 ≤8 ms; pérdida ≤0,1 % |
| Reconexión | par nuevo ≤11 s; pares ya conocidos ≤9 s |

## Qué falta

- Medir en hardware, proveedores y redes reales; todo lo anterior es laboratorio.
- El soak de 72 h.
- Una auditoría independiente: ninguna cifra de esta página sustituye a la revisión externa.

Para reproducir las mediciones, el repositorio incluye `tests/load/` (`run.sh`, `stack.sh`, `soak.sh`) y los presupuestos en `tests/load/budgets.json`. Ejecútalos secuencialmente y no los apuntes a producción.
