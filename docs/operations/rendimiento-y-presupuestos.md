# Rendimiento y presupuestos — fase 11

## Alcance y lectura de la evidencia

**Este documento no dimensiona un VPS.** Docker Desktop29.7.2/arm64, VM Linux con **5 vCPU y8320163840bytes de RAM (~7,75GiB)**, sobre Mac17,4 con24GiB físicos y10 CPU lógicas. La VM, la planificación del anfitrión, los namespaces, el generador Python y los listeners loopback condicionan los resultados. No hay enlace WAN, proveedor ni hardware de producción en esta medición.

El checkout base es `d9ab7ec4b4af44dd022ab655b7747c16231a6f8b`, con cambios de trabajo sin commit. Las primeras mediciones de túnel usaron binarios del volumen local `atenea-lab-target`, siempre **solo lectura**. Tras corregir la parada se recompilan agente, CLI y herramienta sintética aparte, `--offline --locked`, con dos trabajos, en `tests/load/results/bin`; se prefieren estos binarios y se superponen de solo lectura en el runner del stack, sin escribir en la caché protegida. Coordinator/relay conservan los binarios locales identificados por hash. No se equipara un commit incrustado a un árbol de fuentes limpio.

Se distinguen **MEDIDO** (salida de una ejecución real), **INFERIDO** (cálculo a partir de ella) y **SIN COMPROBAR**. Una prueba funcional previa, una cuota del código y una CI verde no son medidas de capacidad ni aceptación de producción. Véanse [ADR-009](../architecture/ADR-009-operacion-y-releases.md), [caídas y leases](caidas-y-leases.md), [matriz de aceptación](matriz-de-aceptacion.md) y [soporte y límites](soporte-limites-y-recuperacion.md).

## Reproducción segura

```sh
bash tests/load/build-helper.sh
bash tests/load/run.sh tunnel
bash tests/load/run.sh handshake
bash tests/load/run.sh relay
bash tests/load/stack.sh control
bash tests/load/soak.sh 0.34
# Solo cuando el piloto esté autorizado: no se ha ejecutado durante esta entrega.
bash tests/load/soak.sh 72
```

Ejecutar **secuencialmente**, nunca lanzar dos trabajos a la vez. Los runners imprimen `docker ps -a` y `docker network ls`, exigen imágenes locales con `--pull=never`/`--pull never` y no instalan paquetes. El laboratorio de datos y el muestreador usan `--network none`; el stack conserva bridge como el laboratorio compartido. No se aplica un filtro nft global al runner: cortaba las redes NAT/TUN sintéticas; tampoco `internal=true` en edge, que impide alcanzar coordinator desde el origen NAT simulado. Esto **no equivale a aislamiento de egreso del runner**; la prohibición de descargas se implementa con imágenes locales, pulls deshabilitados y ausencia de instalaciones/compilación en el arnés. Cada ejecución usa exclusivamente nombres `atenea-load-*`. No hacer `docker system prune`, borrar volúmenes de caché ni sustituir tags locales. `results/` y `out/` contienen artefactos ignorados; custodiar la salida antes de limpiarlos. El bootstrap contiene claves efímeras de laboratorio; no publicar sus directorios privados.

## Método de medida

- Dos repeticiones por escenario y escala; se publican ambas, media y peor caso. Para throughput el peor caso es el **mínimo**; para coste, memoria, pérdida y latencia es el **máximo**. No se interpreta una media de dos percentiles como percentil de una población agregada.
- Reloj `time.monotonic()` de pared. `/proc/<pid>/stat`: ticks de usuario+sistema / `SC_CLK_TCK`; `/proc/<pid>/status`: `VmRSS`; `/proc/<pid>/fd`: ficheros abiertos. En transferencias se muestrea RSS cada 250 ms, además del inicio y final. El pico observado no es un máximo continuo ni RSS equivale a heap privado.
- `cpu_percent=100·CPU_s/pared_s`; 100 % equivale a un núcleo lógico, puede superarse con varios hilos. Coste normalizado = `CPU_s/Mbit entregados`, equivalente a `CPU_%/(Mbit/s)/100`. Se conserva cada PID y se identifica explícitamente si desapareció durante una renovación deliberada del iniciador. No se atribuye al agente la CPU del generador.
- TCP: **536 870 912 bytes (512 MiB)** por transferencia, bloque reutilizado de 64 KiB; SHA-256 en ambos extremos, sin escribir el payload a disco. La ventana incluye cliente, envío, recepción y digest. No mide solo cifrado.
- RTT: 2000 ecos ICMP de 10 ms nominales por repetición; la cadencia real depende de `ping`/scheduler. p50/p95/p99 por rango más próximo, pérdida explícita. El comparador sin túnel va de `a` al router público sintético del namespace `inet`: es un **suelo orientativo**, no el mismo trayecto de dos agentes ni un RTT WAN.
- UDP: 10 000 datagramas de 1200 bytes durante 5 s nominales (19,2 Mbit/s ofrecidos), secuencia para contar entregados únicos, y 3 s de drenaje. No es búsqueda de saturación; `received_mbit_s` usa la ventana ofrecida de 5 s; las muestras CPU incluyen el drenaje. Para comparaciones normalizadas usar CPU total / bits entregados, no mezclar ventanas.
- Se fuerza cada camino mediante los mismos bloqueadores nftables y se comprueba el estado IPC de ambos agentes, reutilizando `tests/network-lab/nat/lib.sh`. TLS pasa por HAProxy con SNI y ALPN v2; no se infiere del puerto.

## Handshake y protección frente a abuso

El generador separa INIT de longitud correcta con MAC1 inválido (IP suplantada), MAC1 correcto sin cookie y cookie válida. Los mensajes de abuso llevan Noise inválido: **cookie/MAC válidos no son handshakes aceptados**. Tras observar un primer timeout de 15 s, el control conserva el mismo reinicio/NAT/sesión previa **sin flood, tres repeticiones**, y compara los tres tipos de flood a **1k/10k/50k INIT/s**, dos repeticiones por combinación. Cada flood dura 20 s; los primeros ecos `a↔b`, `a↔c` y `c↔b` se buscan en paralelo hasta 60 s. Se conserva si llegaron durante o después de la inundación: una reconexión posterior no demuestra disponibilidad bajo carga. Se conservan CPU y contadores IPC antes/después, muestras de estado cada medio segundo y separaciones de DATA vacío (keepalives) capturadas en la interfaz LAN de b. El retardo del observador y los huecos entre paquetes **no son instrumentación del temporizador interno**; su comparación con el control detecta síntomas, no prueba por sí sola bloqueo del motor. Los INIT enviados no equivalen a recibidos si hay pérdida en colas del kernel.

`COOKIE_REPLY` observado debe medir 62 bytes frente a INIT de 146: factor de payload **0,4247**, no amplificación. La cota normativa es 20/s+ráfaga20 por origen y 100/s+ráfaga100 global ([wire-v2 §5](../protocol/wire-v2.md#5-política-de-carga-y-orden-de-validación)); una sola IP no demuestra el máximo global. Se publican la tasa observada y el contexto, no se confunde una cota de configuración con rendimiento medido.

## Relay: capacidad y cuotas

El ensayo de recuperación de la tabla de tasas reduce deliberadamente `MAX_RATE_ENTRIES` a 128 para saturarla con 5000 IPv4 distintas, espera reposición y exige servicio a una dirección nueva. Es una regresión F1 a menor escala, no una medición de 65 536 entradas. Una segunda instancia efímera dual-stack mide agrupación de 100 IPv6 dentro de un /64, y asignaciones firmadas v2 con las **cuotas de asignaciones por defecto**: 4096 total, 1024/red y 32/origen. Las identidades son sintéticas y cada asignación usa un /64 distinto, repartiendo redes; el intento 4097 debe devolver `Full`.

Las firmas y las cookies son reales. El agregado sintético usa 16 pares (32 asignaciones), 1000 DATA/s/asignación y payload estructural wire-v2: el relay no lo descifra. **No se anuncia como 16 túneles Noise end-to-end**; las transferencias E2E de dos agentes se miden aparte. RSS por asignación = incremento frente al proceso vacío / asignaciones simultáneas; incluye estructura y overhead del allocator, no es una constante portable.

## Coordinator y control plane

La metodología y contratos de clientes sintéticos firmados se describen en [carga de control](carga-control-metodologia.md). Una presencia deseada no se cuenta como residente sin comprobar el gauge/health; si el límite predeterminado es 4096, 5000 y 10 000 requieren configurar y declarar la capacidad del ensayo. Se mide también el rechazo por cuotas con la configuración explícita. Las solicitudes rechazadas no cuentan como throughput útil. La carga de BD usa datos sintéticos y PostgreSQL real; se distingue tamaño físico de tablas/índices del WAL y de la imagen Docker.

## Soak local y piloto

`tests/load/soak.sh <horas>` (o `bash tests/load/stack.sh soak 6m`) usa stack completo y dos agentes: calentamiento directo, relay UDP, eco continuo y transferencia SHA-256 de 16 MiB cada minuto. El bucle usa reloj monotónico y cadencia absoluta de 10 s para procesos/estados/contadores; Prometheus y transferencias tienen cadencia de 60 s. Una iteración fallida se registra como `soak-error` y no cancela las siguientes; el resumen final sigue fallando si hubo errores. Cada iteración tiene plazo de hasta 55 s (o el tiempo restante), y el trabajo completo tiene plazo duro de duración solicitada + 10 min, incluido el arranque. El `timeout` local usa solo Python estándar (macOS no incluye coreutils); envía TERM al grupo, concede 60 s a la limpieza y después KILL.

El muestreador reutiliza un único contenedor local sin red, con `/proc` de la VM, en vez de arrancar cuatro contenedores por muestra. Cuando acaba el runner se detiene el muestreador; el `trap` elimina runner, auxiliares, stack con `down -v` y runtime propio, conservando resultados. El resumen exige ≥5 muestras/min de agentes, procesos y estados, una transferencia de 16 MiB con SHA-256 por minuto, scrapes completos y ausencia de errores/reinicios/OOM. RSS >16 MiB o FD >8 son alarmas locales, **no detectores estadísticos de fugas**; la pendiente del stack descuenta hasta 5 min de calentamiento.

### Intentos históricos fallidos

| Proyecto | MEDIDO | Causa y límite de la evidencia |
| --- | --- | --- |
| `atenea-load-27313` | Alta de ambos agentes; `RuntimeError: Sin camino relay-udp`; no completó soak. | Arnés de conectividad, no medida de estabilidad del producto. |
| `atenea-load-29222` | `Sin camino direct`; estado de coordinator con `last_error: "error de red"`, sin announce exitoso. | El aislamiento `internal=true` y el filtrado nft del arnés impedían alcanzar coordinator desde las NAT simuladas. |
| `atenea-load-31018` | `Sin camino direct`; agente sin lease, versión 0 y sin pares. | Mismo recorrido de red del arnés; no aporta evidencia de carga sostenida. |
| `atenea-load-32979` (20 min pedidos) | Una transferencia de 16 MiB a **24,7306 Mbit/s**, un scrape y una muestra a **5,4702 s**. El proceso principal no acabó; timeout exterior a 3000 s y limpieza manual posterior. | La conversión `20m`→horas y el bucle original son correctos por lectura: no hay un `return` tras la primera muestra. Había operaciones sin plazo (`atenea status`, lectura de readiness TCP), ninguna recuperación por iteración ni límite global; el muestreador esperaba al runner. **INFERIDO:** bloqueo en una operación posterior; los artefactos no identifican la llamada exacta. No se afirma falsamente que el formato de duración fuera la causa. |
| Invocación preliminar de esta corrección (sin proyecto creado) | Salida 1 antes del bootstrap y sin recursos Docker nuevos. | `timeout` era un builtin del shell de herramientas, no un ejecutable disponible dentro de Bash en macOS. Corregido con timeout Python estándar local; sin instalar paquetes. |

Las pruebas sin Docker inyectan reloj/sleep y demuestran 18 iteraciones en 180 s, transferencias a 0/60/120 s y continuación tras un fallo puntual. Corrigen y cubren los riesgos del arnés, pero no reconstruyen la pila del proceso histórico.

### Ensayo corto MEDIDO: `atenea-load-42853` (6 min)

`timeout 900 bash tests/load/stack.sh soak 6m`: **421,07 s** de pared total, stack saludable en 30 s; carga **360,0084 s**, 36 muestras de agentes (**6 en cada minuto**), 39 muestras por proceso del stack y 39 por cada uno de sus ocho estados (6/6/6/6/6/7 dentro de la ventana de tráfico). Se completaron seis transferencias, una en cada minuto, con digest en ambos extremos `341aacac661ccb210720bedaa9ead5d668fe5ea41a73532fc147c71e34040df1`. Tasas por transferencia: **24,5597 / 25,4107 / 25,4206 / 25,4244 / 24,5287 / 25,1583 Mbit/s**; media 25,0837 y tasa ponderada por duración 25,0775 Mbit/s. Seis scrapes completos; errores DB = 0, sin series HTTP5xx, errores de severidad en logs = 0, sin OOM/reinicio ni proceso desaparecido.

| Proceso (PID conservado) | RSS KiB inicio→fin | Δ RSS KiB | FD inicio→fin |
| --- | ---: | ---: | ---: |
| control-plane (17478) | 10 156→10 720 | +564 | 14→15 |
| coordinator (17352) | 4 252→4 364 | +112 | 13→15 |
| relay (18205) | 4 848→4 920 | +72 | 13→13 |
| postgres principal (17054) | 28 124→28 124 | 0 | 10→10 |
| agente a (172, namespace runner) | 6 400→7 752 | +1 352 | 14→14 |
| agente b (188, namespace runner) | 7 124→7 904 | +780 | 14→14 |

Son extremos **muestreados**, no máximos de heap: ventana del stack 377 s incluyendo arranque de agentes; agentes t=5,501→350,007 s. La comparación propia de agentes desde antes de su primera transferencia hasta después de 360 s dio +1 528/+1 244 KiB y 0/0 FD. No se observó crecimiento sobre los márgenes locales; seis minutos no descartan fugas lentas.

**No se declara aceptado «contadores sin errores».** En ambos agentes `path_lost` 0→1 aparece en t=40,004 s; `probe_failed` 0→4 en t=50,005/90,006/160,007/290,003 s; `relay_down` 0→1 en t=305,374 s. `tx_bad_source` ya valía 1 en la primera muestra y quedó en 1 (delta 0). Los tiempos empiezan después de terminar `path(direct)` y `path(relay-udp)`; no hubo más llamadas de bloqueo. El sondeo del directo bloqueado y su pérdida pueden ser esperados mientras la ruta activa siga siendo relay; la caída del relay cerca de 300 s requiere correlación con rutas/logs. Este intento no guardaba estados IPC completos ni logs de agentes; se perdieron al limpiar y **no se inventa esa evidencia**. El código de salida 0 del resumen anterior solo acredita sus comprobaciones entonces implementadas; el criterio actualizado exige marca `steady_since`, estados completos y deltas estables, registrando aparte calentamiento y sondeos esperados.

La limpieza automática dejó **0 contenedores, 0 redes y 0 volúmenes propios**, eliminó el runtime y conservó los resultados. Inventarios antes/después: únicamente contenedores preexistentes ajenos a la prueba (intactos) y redes `bridge/host/none`. Descargas solicitadas por el arnés: **0**; no se instalaron paquetes ni se compiló. Hubo una invocación preliminar que terminó antes de crear recursos porque el `timeout` del shell de herramientas no existe dentro de Bash de macOS; se resolvió con el timeout local, sin instalar coreutils.

### Ensayo de correlación MEDIDO: `atenea-load-48076` (12 min, autorizado)

`timeout 1200 bash tests/load/stack.sh soak 12m`: **782,76 s** de pared total, carga **720,0092 s**, 72 muestras de agentes (**6/min en los doce minutos**), 75 muestras por proceso del stack y por estado de servicio. Doce transferencias SHA-256 íntegras, una por minuto; tasas mínimo/media/máximo **24,3489 / 24,9801 / 25,4247 Mbit/s**. Doce scrapes; errores de logs, DB, HTTP5xx, OOM y reinicios = 0. Ambos agentes siguieron vivos. Durante la ejecución, el criterio estricto inicial terminó **exit 1**, `steady_errors=4` por dos `relay_down` por agente; limpió automáticamente. Tras la decisión explícita del integrador, el **reanálisis de los mismos datos, sin otro ensayo Docker**, pasa B24 con tolerancia etiquetada **`CONOCIDO: relay-token-cycle`**. No se ha cambiado el motor ni desplazado el comienzo estable.

`steady_since=1792997655,034362` (**06:54:15,034362 UTC**), inmediatamente tras completar directo→relay UDP. Los contadores del calentamiento se conservan aparte (`tx_bad_source=1`, `relay_ready=1` y `path_switches=1` por agente; el delta estable de `tx_bad_source` es 0). Todas las muestras estables de ambos agentes informan `peers[0].path=relay-udp`; la ruta directa figura no saludable y no lista.

| Contador (ambos agentes) | Muestra, índice desde 0 | t desde steady, s | Clasificación por evidencia |
| --- | --- | --- | --- |
| `path_lost` 0→1 | 4 | 40,0154 | Esperado: pérdida del directo bloqueado; relay sigue activo antes/después. |
| `probe_failed` 0→1→2→3→4→5 | 5 / 9 / 16 / 29 / 54 | 50,0188 / 90,0110 / 160,0162 / 290,0160 / 545,5205 | Esperado: sondeos del directo bloqueado con backoff, sin cambio de la ruta relay. |
| `relay_down` 0→1 | 30 | 305,3762 | **Hallazgo estable**, primera recreación de asignación. |
| `relay_down` 1→2 | 60 | 605,4717 | **Hallazgo estable**, repetición ≈300 s después. |

### Hallazgo: ciclo de recreación del relay cada ≈300 s

Para `relay_down`, muestras anteriores/posteriores: **06:59:05,053800→06:59:20,413967 UTC** y **07:04:05,067679→07:04:20,506872 UTC**. En ambos agentes y ciclos la ruta muestreada es `relay-udp`→`relay-udp`, la entrada aparece `ready`→`ready`, se conserva `relay_id=9516a091e0fff9e07468fda68491d6d5`, pero `pings_sent` de la asignación se reinicia **60→3** y `relay_ready` avanza **1→2→3**; `path_switches` no aumenta. El agente a además pasa `relay.drops.unknown_origin_or_allocation` **1→2→3**. La repetición de recreación es **MEDIDA**; su vínculo causal con la caducidad del token de 300 s es **INFERIDO**, no demostrado por un log de renovación.

El ping continuo timestampado (`soak-ping.log`, intervalo 0,2 s) envió **3540**, recibió **3537**: **3 pérdidas, 0,0847458 %**. Pérdidas en secuencias **305, 1192 y 1484**. Alrededor del primer ciclo (±10 s respecto de t=300) hay una pérdida, secuencia 1484: respuestas vecinas a unix **1792997956,763805→1792997957,169906**, hueco **406,101 ms**. Es una cota entre respuestas, **no una duración exacta del corte**; el evento solo se observa en una muestra posterior. En torno al segundo ciclo (t=600 ±10 s) **no se perdió ningún eco**, hueco máximo **267,527 ms**. Las transferencias por minuto por sí solas ocultaban esta recreación; tampoco se afirma que cada renovación produzca una pérdida.

Los logs se copiaron **antes de limpiar**, acotados a 2000 líneas por agente y por servicio: `agent-a.log`, `agent-b.log`, `relay.log`, `coordinator.log`; los estados completos quedan en `soak.jsonl`. En las ventanas de 10 s **06:59:10–06:59:20** y **07:04:10–07:04:20**, el log del agente a solo contiene `INFO configuración aplicada` a **06:59:11,721030** / **07:04:11,721162**, y b a **06:59:11,855951** / **07:04:11,852680**. Relay/coordinator no emiten líneas en esas ventanas (solo INFO de arranque a **06:53:50,320583** / **06:53:33,110033**). Este nivel de logs no permite fechar la transición dentro del intervalo de muestreo ni probar directamente la caducidad del token; no se inventan mensajes ausentes.

| Proceso (PID) | RSS KiB inicio→fin | Δ RSS KiB | FD inicio→fin |
| --- | ---: | ---: | ---: |
| control-plane (41294) | 10 164→10 760 | +596 | 14→15 |
| coordinator (41171) | 4 268→4 408 | +140 | 13→15 |
| relay (42039) | 4 840→4 900 | +60 | 13→13 |
| postgres principal (40873) | 28 124→28 124 | 0 | 10→10 |
| agente a (172, namespace runner) | 6 388→7 656 | +1 268 | 14→14 |
| agente b (188, namespace runner) | 7 120→9 128 | +2 008 | 14→14 |

Extremos muestreados: stack 737 s; agentes primera muestra hasta t≈710 s. Sin crecimiento sobre márgenes locales; no prueba ausencia de fuga. Limpieza automática incluso con `exit 1`: **0 recursos propios**, runtime eliminado, resultados conservados; inventarios finales iguales a los iniciales. Descargas detectadas **0**, ninguna instalación/compilación.

**Impacto y decisión:** se observa recreación periódica aun manteniendo la ruta nominal; los huecos entre respuestas acotan el microcorte observado a **≈0,4 s** en este entorno (406 ms el mayor), sin romper las doce transferencias TCP con digest. No es una garantía de duración por ciclo ni de pérdida cero. Se registra como **HALLAZGO CONOCIDO de rendimiento/continuidad**, no como ausencia de defectos. Propuesta **NO implementada**: renovar **make-before-break** antes de caducar el token, sin cortar la asignación vigente; requiere una decisión de diseño de la interacción **coordinator-v1/relay-v2** antes de modificar el motor.

**Tolerancia B24 explícita:** `known_finding=relay-token-cycle`, máximo de `floor(duración_estable/300)+1` eventos `relay_down` por agente (**3** en 720 s), con `relay_ready=relay_down+1` y tantos incrementos `relay_ready` como `relay_down`. Se imprimen siempre los eventos conocidos (a=2, b=2); no se silencian. Solo se permiten `path_lost`/`probe_failed` del directo si el arnés lo declara bloqueado y la muestra confirma ruta `relay-udp`; otros incrementos inesperados, logs de error, pérdida de ping >1 %, contadores reiniciados o evidencia ausente siguen fallando. No se transforma la excepción conocida en una aceptación del soak de 72 h.

**Validación sin Docker posterior:** `bash -n` pasa; **10 pruebas unitarias** pasan (reloj inyectado, fallo puntual, timeout coordinado, dos ciclos conocidos permitidos, caída adicional sin su `ready` rechazada, exceso del máximo rechazado y pérdida >1 % rechazada). `check-budgets.py --self-check` pasa. `summarize.py soak tests/load/out/atenea-load-48076` pasa e imprime `CONOCIDO`; `check-budgets.py --budgets tests/load/out/atenea-load-48076/B24-budgets.json tests/load/out/atenea-load-48076/soak-summary.jsonl` pasa con a=2/b=2. El fichero B24 es una copia exacta del presupuesto, no un umbral rebajado para el ensayo.

El comando global pedido `python3 tests/load/check-budgets.py tests/load/out/*/*summary.jsonl` termina **exit 1**, de forma esperada: faltan muestras en **18 presupuestos de túnel/handshake/relay** (esas mediciones no están en esos directorios) y el resumen antiguo de 6 min carece de `steady_ok`/evidencia completa. No se presenta ese resultado como aceptación global ni se rellenan datos históricos ausentes.

El ensayo corto local **NO sustituye al soak de 72 h** ni cumple por sí solo la ventana local de 20–30 min. El piloto debe repetir con tráfico, retención, versiones y hardware representativos y sin rebajar cuotas para obtener una salida verde.

## Resultados MEDIDOS: túnel

Transferencias de 512 MiB íntegras; digest en ambos extremos `c047731a3c134f3d34286d608e9c173027d50f43ab9d2064f3c360939977e908`. UDP: 10 000/10 000 entregados en cada repetición siguiente; RTT: 2000/2000 ecos. La unidad de latencia es ms y la de transferencia s.

| Camino · métrica | Repetición 1 | Repetición 2 | Media | Peor |
| --- | ---: | ---: | ---: | ---: |
| Directo · TCP Mbit/s | 1002,331 | 1017,287 | 1009,809 | 1002,331 |
| Directo · TCP pared s | 4,285 | 4,222 | 4,253 | 4,285 |
| Directo · RTT p50 ms | 0,564 | 0,540 | 0,552 | 0,564 |
| Directo · RTT p95 ms | 0,972 | 0,922 | 0,947 | 0,972 |
| Directo · RTT p99 ms | 1,600 | 1,460 | 1,530 | 1,600 |
| Directo · pérdida UDP % | 0 | 0 | 0 | 0 |
| Relay UDP · TCP Mbit/s | 21,649 | 21,650 | 21,649 | 21,649 |
| Relay UDP · TCP pared s | 198,389 | 198,386 | 198,388 | 198,389 |
| Relay UDP · RTT p50 ms | 0,758 | 0,761 | 0,760 | 0,761 |
| Relay UDP · RTT p95 ms | 1,530 | 1,600 | 1,565 | 1,600 |
| Relay UDP · RTT p99 ms | 3,230 | 3,260 | 3,245 | 3,260 |
| Relay UDP · pérdida UDP % | 0 | 0 | 0 | 0 |
| Relay TLS · TCP Mbit/s | 21,636 | 21,645 | 21,641 | 21,636 |
| Relay TLS · TCP pared s | 198,508 | 198,425 | 198,466 | 198,508 |
| Relay TLS · RTT p50 ms | 0,782 | 0,665 | 0,724 | 0,782 |
| Relay TLS · RTT p95 ms | 1,550 | 1,530 | 1,540 | 1,550 |
| Relay TLS · RTT p99 ms | 3,170 | 2,620 | 2,895 | 3,170 |
| Relay TLS · pérdida UDP % | 0 | 0 | 0 | 0 |

Recursos durante **TCP**. A/B son iniciador/receptor, no suma de ambos; RSS se expresa en KiB. Coste = puntos porcentuales de un núcleo por Mbit/s entregado. Cada celda conserva las dos repeticiones; el presupuesto se aplica al peor proceso, no al promedio de peers.

| Camino / proceso | CPU % R1 / R2 | Media / peor CPU % | Pico RSS R1 / R2 | Media / peor RSS | Coste CPU R1 / R2 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Directo / agente A | 117,854 / 118,902 | 118,378 / 118,902 | 6680 / 6716 | 6698 / 6716 | 0,11758 / 0,11688 |
| Directo / agente B | 106,185 / 104,216 | 105,201 / 106,185 | 6796 / 6848 | 6822 / 6848 | 0,10594 / 0,10245 |
| Directo / relay ocioso | 0 / 0 | 0 / 0 | 4748 / 4752 | 4750 / 4752 | 0 / 0 |
| Relay UDP / agente A | 7,279 / 7,183 | 7,231 / 7,279 | 7392 / 7536 | 7464 / 7536 | 0,33621 / 0,33178 |
| Relay UDP / agente B | 5,771 / 5,610 | 5,691 / 5,771 | 7364 / 7428 | 7396 / 7428 | 0,26659 / 0,25914 |
| Relay UDP / relay | 4,804 / 4,637 | 4,721 / 4,804 | 4756 / 4756 | 4756 / 4756 | 0,22189 / 0,21420 |
| Relay TLS / agente A | 9,914 / 9,535 | 9,725 / 9,914 | 7560 / 7636 | 7598 / 7636 | 0,45821 / 0,44052 |
| Relay TLS / agente B | 7,229 / 7,227 | 7,228 / 7,229 | 7836 / 7944 | 7890 / 7944 | 0,33411 / 0,33388 |
| Relay TLS / relay | 3,803 / 3,744 | 3,774 / 3,803 | 5524 / 5600 | 5562 / 5600 | 0,17579 / 0,17299 |

**INFERIDO, no defecto:** el plateau de ≈21,65 Mbit/s TCP por relay es compatible con la cuota predeterminada de 24 Mbit/s de datagramas por asignación y la sobrecarga. No es una saturación de CPU ni autoriza prometer 1 Gbit/s a una asignación relay. Los 19,2 Mbit/s UDP ofrecidos no tuvieron pérdida en las seis repeticiones completas.

**Incidencias del arnés, no bugs de producto:** una primera ejecución se abortó por una indentación introducida en el generador mientras corría; sus dos comparadores base no se usan en las tablas de carga. Otra ejecución completó directo/relay UDP y TCP+RTT TLS antes de descubrir que `net_block_udp` del laboratorio también bloqueaba el UDP de aplicación dirigido a la TUN (`EPERM`). El arnés de carga ahora permite explícitamente `iif/oif atenea0` antes del filtro: conserva el bloqueo del UDP exterior y permite medir UDP encapsulado en relay TLS. No se modificó el bloqueador compartido ni se contabilizó ese fallo como pérdida del túnel.

### RTT base y recursos normalizados

Comparador sin túnel (`a→inet`), últimas dos repeticiones completas anteriores a las transferencias:

| Métrica | R1 | R2 | Media | Peor |
| --- | ---: | ---: | ---: | ---: |
| RTT p50 ms | 0,117 | 0,115 | 0,116 | 0,117 |
| RTT p95 ms | 0,221 | 0,217 | 0,219 | 0,221 |
| RTT p99 ms | 0,430 | 0,382 | 0,406 | 0,430 |
| Pérdida (2000 ecos/repetición) | 0 % | 0 % | 0 % | 0 % |

Normalización RSS durante TCP, **KiB/(Mbit/s entregado)**; describe el proceso completo a esa carga, no memoria que deba multiplicarse linealmente al dimensionar:

| Camino/proceso | R1 | R2 | Media | Peor |
| --- | ---: | ---: | ---: | ---: |
| Directo / A | 6,664 | 6,602 | 6,633 | 6,664 |
| Directo / B | 6,780 | 6,732 | 6,756 | 6,780 |
| Directo / relay ocioso | 4,737 | 4,671 | 4,704 | 4,737 |
| Relay UDP / A | 341,445 | 348,090 | 344,767 | 348,090 |
| Relay UDP / B | 340,152 | 343,101 | 341,626 | 343,101 |
| Relay UDP / relay | 219,685 | 219,681 | 219,683 | 219,685 |
| Relay TLS / A | 349,413 | 352,779 | 351,096 | 352,779 |
| Relay TLS / B | 362,169 | 367,008 | 364,589 | 367,008 |
| Relay TLS / relay | 255,312 | 258,717 | 257,014 | 258,717 |

Recursos **UDP**: CPU en ventana con drenaje, RSS KiB. La última columna es `100·CPU_s/Mbit entregados`, por lo que no mezcla esa ventana con los cinco segundos de oferta. La media/peor del coste se obtiene de las dos muestras publicadas; la variación TLS muestra por qué dos repeticiones no dan un intervalo de confianza.

| Camino/proceso | CPU % R1 / R2 | Media / peor CPU % | RSS R1 / R2 | Media / peor RSS | Coste CPU R1 / R2 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Directo / A | 11,548 / 11,307 | 11,427 / 11,548 | 6716 / 6764 | 6740 / 6764 | 0,9687 / 0,9479 |
| Directo / B | 6,953 / 6,461 | 6,707 / 6,953 | 6848 / 6852 | 6850 / 6852 | 0,5833 / 0,5417 |
| Directo / relay | 0 / 0 | 0 / 0 | 4752 / 4752 | 4752 / 4752 | 0 / 0 |
| Relay UDP / A | 12,049 / 11,042 | 11,545 / 12,049 | 7420 / 7536 | 7478 / 7536 | 1,0104 / 0,9271 |
| Relay UDP / B | 5,714 / 5,335 | 5,524 / 5,714 | 7268 / 7432 | 7350 / 7432 | 0,4792 / 0,4479 |
| Relay UDP / relay | 7,328 / 6,824 | 7,076 / 7,328 | 4756 / 4756 | 4756 / 4756 | 0,6146 / 0,5729 |
| Relay TLS / A | 16,180 / 5,819 | 11,000 / 16,180 | 7560 / 7636 | 7598 / 7636 | 1,3646 / 0,4896 |
| Relay TLS / B | 2,100 / 0,991 | 1,545 / 2,100 | 7840 / 7948 | 7894 / 7948 | 0,1771 / 0,0833 |
| Relay TLS / relay | 3,582 / 1,238 | 2,410 / 3,582 | 5524 / 5600 | 5562 / 5600 | 0,3021 / 0,1042 |

Para UDP, la normalización RSS se obtiene sin extrapolación dividiendo cada RSS de esta tabla por **19,2 Mbit/s**, entregados en las seis repeticiones: peores A/B/relay = directo **352,29 / 356,88 / 247,50**, relay UDP **392,50 / 387,08 / 247,71**, relay TLS **397,71 / 413,96 / 291,67 KiB/(Mbit/s)**.

### Identidad de binarios observada

Los cuatro ejecutables informaron **0.1.0 (unknown)**: hay versión, pero **no commit incrustado**. No se inventa correspondencia con HEAD; se conserva el hash como identidad del binario medido. Kernel Linux `7.0.12-linuxkit`, `aarch64`.

| Binario | SHA-256 observado |
| --- | --- |
| `atenea` | `9a8ff78b6c0a5182ab5b542b924b94e5b2cc76cbcfa8b7f0814191ff0a460d79` |
| `atenea-agent` | `4988fb8d4fdb8a7633aff8ea6443e7fb31ca14b4b03d00c55035b07c74090e64` |
| `atenea-coordinator` | `47194edcc1923469325646e7360154b0ffcc2e84956627bc1e0e3feecb222a8b` |
| `atenea-relay` | `bedb3e977e0558d12c36abdb5a5436d470737588a98dbdbc1ef76addad5cce55` |

## Panel

No se modifica ni se reconstruye el panel en esta tarea y no se ejecuta Lighthouse. Se conserva la cifra previa aportada para el bundle: **767 kB / 228 kB gzip**, **no una nueva medición de esta entrega**. No se atribuye capacidad de backend ni aceptación de producción al tamaño del JavaScript.

## Presupuestos de regresión propuestos: laboratorio Docker/Mac arm64

Son **alarmas calibradas con lo medido aquí**, con margen; no promesas comerciales ni recomendaciones de RAM/CPU para un VPS. Una regresión es incumplir **cualquier** cota de la fila, incluso si mejora la media global. Usar mismos binarios/topología/oferta y un anfitrión sin otras cargas comparables; si cambian, recalibrar con evidencia en vez de subir el límite silenciosamente.

| Nº / escala | Presupuesto propuesto | Evidencia y margen |
| --- | --- | --- |
| B01 · 2 agentes, directo TCP | ≥700 Mbit/s; 512 MiB en ≤7 s; cada agente ≤180 % CPU y ≤16 MiB RSS; coste ≤0,18 puntos CPU/(Mbit/s) | Peor 1002,33 Mbit/s / 4,285 s / 118,90 % / 6848 KiB; margen throughput ≈30 % y RSS >2× |
| B02 · 2 agentes, relay UDP TCP | ≥18 Mbit/s; ≤240 s; agente ≤15 % CPU, relay ≤10 %; cada proceso ≤16 MiB RSS; coste agente ≤0,55 y relay ≤0,40 | Peor 21,649 Mbit/s / 198,389 s; agentes ≤7,279 %, relay4,804 %; margen throughput ≈17 % |
| B03 · 2 agentes, relay TLS TCP | ≥18 Mbit/s; ≤240 s; agente ≤18 % CPU, relay ≤9 %; RSS ≤16 MiB; coste agente ≤0,70 y relay ≤0,35 | Peor 21,636 Mbit/s / 198,508 s; agente9,914 %, relay3,803 % |
| B04 · directo, 2000 ecos | p95≤2 ms; p99≤4 ms; pérdida≤0,1 % | Peores0,972 / 1,600 ms; cero pérdida |
| B05 · relay UDP, 2000 ecos | p95≤3 ms; p99≤8 ms; pérdida≤0,1 % | Peores1,600 / 3,260 ms; cero pérdida |
| B06 · relay TLS, 2000 ecos | p95≤3 ms; p99≤8 ms; pérdida≤0,1 % | Peores1,550 / 3,170 ms; cero pérdida |
| B07 · directo UDP a19,2 Mbit/s | ≥19,18 Mbit/s entregados; pérdida≤0,1 %; agente≤25 % CPU, relay≤15 %; RSS≤16 MiB | Peor agente11,548 %; margen CPU >2× |
| B08 · relay UDP a19,2 Mbit/s | Igual B07 | Peor agente12,049 %, relay7,328 % |
| B09 · relay TLS UDP a19,2 Mbit/s | Igual B07 salvo agente≤30 % CPU | Peor agente16,180 %, relay3,582 % |

Los valores exactos y los recuentos mínimos de procesos están en [`tests/load/budgets.json`](../../tests/load/budgets.json). El comprobador rechaza métricas ausentes/no finitas, escenarios con menos de dos muestras y recursos de agentes/relay ausentes; no usa un número inventado cuando falta evidencia:

```sh
python3 tests/load/summarize.py handshake tests/load/results
python3 tests/load/summarize.py relay tests/load/results
# Normalizar cada directorio de control con «summarize.py control DIRECTORIO».
# Tras completar control y soak; no omitir perfiles para conseguir verde:
python3 tests/load/check-budgets.py tests/load/results/tunnel.jsonl \
  tests/load/results/handshake-summary.jsonl tests/load/results/relay-summary.jsonl \
  tests/load/out/*/control-summary.jsonl tests/load/out/*/soak-summary.jsonl
python3 tests/load/check-budgets.py --self-check
```

El piloto debe medir otros presupuestos sobre su propio hardware; superar estas cotas en una VM compartida ocupada es una alerta a investigar, no prueba aislada de defecto de producto.

## Resultados MEDIDOS: handshake, antes de reservar cuota de cookie por par

Serie de **641,35 s de pared**, con arreglo de parada acotada ya incorporado, pero **anterior** a la reserva posterior de cookies para pares configurados. Agente SHA-256 `b71e262314bbb224d8a616f0026d217d42009d848164d0c25a493f474a6bb8f7`; CLI `e09fcbefc86d3302091dc9819c8e167c1f99c08b40849167051df0d91bcf59e9`. Ambos informan `0.1.0 (d9ab7ec4b4af44dd022ab655b7747c16231a6f8b)` con cambios de trabajo, no identidad de fuentes limpias. Coordinator/relay conservan los hashes de la tabla anterior.

### Parada real del proceso: verificación del arreglo

**21/21** invocaciones de `atenea down` hasta desaparición real del proceso completaron en menos de 3 s: mínimo **0,162 s**, media **1,959 s**, peor **2,079 s**. Las cinco primeras fueron **2,079 / 2,048 / 2,045 / 2,032 / 2,055 s**, media **2,052 s**. El arnés espera el PID, no solo la eliminación del socket, y falla al superar 10 s. El timeout de runtime de 2 s explica la mayoría de muestras; no es tiempo de transferencia ni de handshake.

Antes del arreglo se observó proceso vivo **más de 10 s** después de `limpieza de red terminada`, y reinicio inmediato con fallo de bind. El cierre de tareas async no cancelaba HTTP bloqueante pendiente; la corrección de runtime fue realizada y comprobada por la entrega de agente, no se atribuye a esta herramienta de carga. El ensayo previo abortado y sus logs se conservan como diagnóstico, no como repetición buena.

### Primer eco legítimo: tres controles y dos repeticiones por carga

Segundos. Cada celda publica **R1 / R2 → media / peor**. Los controles tienen R1/R2/R3. El reloj de primer eco comienza después de arrancar los iniciadores; la parada se mide aparte. Un eco posterior al flood de 20 s no satisface disponibilidad **durante** el flood.

| Tipo / INIT/s ofrecidos | a↔b | c↔b | a↔c |
| --- | --- | --- | --- |
| Control sin flood (3) | 15,835 / 0,055 / 0,060 → 5,317 / 15,835 | 19,937 / 3,324 / 0,167 → 7,809 / 19,937 | 20,304 / 20,308 / 20,554 → 20,388 / 20,554 |
| MAC1 inválido / 1k | 0,068 / 0,062 → 0,065 / 0,068 | 0,171 / 0,179 → 0,175 / 0,179 | 20,672 / 20,573 → 20,623 / 20,672 |
| MAC1 inválido / 10k | 0,067 / 15,319 → 7,693 / 15,319 | 0,174 / 0,174 → 0,174 / 0,174 | 20,567 / 20,319 → 20,443 / 20,567 |
| MAC1 inválido / 50k | 0,075 / 0,059 → 0,067 / 0,075 | 0,173 / 0,173 → 0,173 / 0,173 | 21,021 / 20,561 → 20,791 / 21,021 |
| MAC1 válido sin cookie / 1k | 1,316 / 0,062 → 0,689 / 1,316 | 1,167 / 0,178 → 0,672 / 1,167 | 20,172 / 25,562 → 22,867 / 25,562 |
| MAC1 válido sin cookie / 10k | 0,070 / 0,059 → 0,064 / 0,070 | 0,169 / 0,171 → 0,170 / 0,171 | 20,315 / 20,420 → 20,367 / 20,420 |
| MAC1 válido sin cookie / 50k | **40,572 / 0,069 → 20,320 / 40,572** | **40,418 / 0,005 → 20,211 / 40,418** | 25,819 / 20,566 → 23,193 / 25,819 |
| Cookie válida / 1k | 0,063 / 0,072 → 0,067 / 0,072 | 0,177 / 0,175 → 0,176 / 0,177 | 20,424 / 20,671 → 20,547 / 20,671 |
| Cookie válida / 10k | 0,062 / 0,064 → 0,063 / 0,064 | 0,167 / 0,170 → 0,168 / 0,170 | 20,307 / 20,316 → 20,312 / 20,316 |
| Cookie válida / 50k | 0,062 / 0,072 → 0,067 / 0,072 | 0,163 / 0,172 → 0,168 / 0,172 | 20,417 / 20,422 → 20,420 / 20,422 |

El primer control por sí solo ya tarda15,8–20,3s: no se atribuye el timeout inicial de15s a DoS. **El outlier de40,6s con MAC1 válido sí queda como hallazgo de disponibilidad**, no como ruido: ambos pares contra b recuperaron después del flood y R2 no lo reprodujo. Los controles no justifican ocultarlo ni ampliar un presupuesto para que pase.

### CPU/RAM del respondedor y coste observado

CPU % R1/R2; media/peor. RSS KiB. El coste por INIT es **CPU total del respondedor / INIT ofrecidos**, no microbenchmark de DH: incluye trabajo de dos pares legítimos, temporizadores y cuotas; en 50k hubo pérdida menor en la cola UDP. El coste por descarte real de MAC1 inválido puede calcularse con `rx_init_mac1` (50k:999805/999846 descartados frente a999999/999977 enviados).

| Tipo / INIT/s | CPU % R1 / R2 → media / peor | RSS R1 / R2 → media / peor | µs CPU/INIT ofrecido R1 / R2 |
| --- | --- | --- | --- |
| MAC1 inválido /1k | 3,219 /3,447 →3,333 /3,447 | 6764 /6784 →6774 /6784 | 35,50 /38,00 |
| MAC1 inválido /10k | 7,613 /7,387 →7,500 /7,613 | 6792 /6796 →6794 /6796 | 8,40 /8,15 |
| MAC1 inválido /50k | 26,818 /28,356 →27,587 /28,356 | 6808 /6816 →6812 /6816 | 6,05 /6,26 |
| MAC1 válido sin cookie /1k | 4,313 /3,565 →3,939 /4,313 | 6824 /6824 →6824 /6824 | 46,50 /48,50 |
| MAC1 válido sin cookie /10k | 10,022 /10,563 →10,293 /10,563 | 6856 /6856 →6856 /6856 | 11,05 /11,65 |
| MAC1 válido sin cookie /50k | 19,973 /36,962 →28,468 /36,962 | 6856 /6908 →6882 /6908 | 8,37 /8,16 |
| Cookie válida /1k | 4,162 /3,805 →3,984 /4,162 | 6912 /6916 →6914 /6916 | 46,00 /42,00 |
| Cookie válida /10k | 10,425 /9,625 →10,025 /10,425 | 6916 /6920 →6918 /6920 | 11,50 /10,40 |
| Cookie válida /50k | 35,241 /34,238 →34,740 /35,241 | 6920 /6920 →6920 /6920 | 7,80 /7,57 |

Controles sin flood: CPU **0,231 /0,185 /0,180 %** (media0,199, peor0,231); RSS **6668 /6700 /6724 KiB** (media6697, peor6724). Hubo **dos handshakes respondidos por control**; CPU total **0,05 /0,04 /0,04s**, equivalente a **25 /20 /20ms por aceptación** incluyendo toda la ventana ociosa y trabajo de fondo. **No** se presenta esa cota de servicio como coste puro de la operación criptográfica ni se compara sin advertencia con un flujo en lotes.

### Cookie y temporizadores observados

- Con MAC1 inválido: **0 COOKIE_REPLY** en las seis repeticiones.
- MAC1 válido sin cookie, R1/R2: **2079/2085** (1k), **2093/2093** (10k), **2095/2093** (50k). Todas las respuestas midieron **62 bytes**, frente a146 de INIT. Máxima tasa media observada en20s **104,75/s**, compatible con100/s más ráfaga100; **no** es un pico en ventanas móviles de1s.
- Cookie válida:19 respuestas residuales por ejecución, asociadas al calentamiento previo a usar MAC2; la carga posterior no se cuenta como handshake autorizado porque Noise es inválido. Predominaron descartes por cuota pre-DH (`rx_init_ratelimited`), sin convertir cookies en autorización.
- Separación máxima de muestras IPC: control≤0,517s; floods≤0,566s con objetivo0,5s. No prueba pausa del bucle de datos. Los huecos de DATA vacío observados normalmente rondaron4,0–4,25s o15,24s, según caminos. Control R2 alcanzó18,245s; en el outlier40s solo hubo un DATA vacío, **sin pares de muestras para estimar cadencia**.
- En ese outlier: CPU8,37s/RSS6856KiB; `rx_init=0`, `handshakes_responder=0`, `rx_init_invalid=396`, `rx_init_ratelimited=1704`, `tx_probe_ka=19`, `probe_failed=2`, `rx_keepalive=0`. Esta versión **no expone contadores de cookie enviados/recibidos**. El agotamiento del cupo global y un intento vencido/backoff son hipótesis motivadas, no una causa declarada sin captura.

## Verificación antes/después: reserva de cookies y reconexión

Se separan dos relojes que **no deben compararse como si fueran uno**:

1. **Handshake bajo inundación:** primer INIT observado del intento/candidato → RESP con receiver_index emparejado después de enviar MAC2. La captura demuestra el intercambio; el eco posterior confirma una sesión utilizable. Captura UDP/IPv4 en `eth0`, con DATA relay UDP desempaquetado; TLS y otros enlaces no son observables aquí. No se publica un tiempo inventado cuando no hubo MAC2/RESP visible.
2. **Reconexión tras reinicio:** proceso arrancado → primer INIT → primer eco. Incluye sondeo, candidatos, rendezvous y backoff, además del handshake. Las capturas comienzan aproximadamente1,24s antes del reloj del primer eco por preparación/arranque: no se mezclan sus orígenes temporales.

### Cinco repeticiones calientes, antes del arreglo de cuota

Tras tres controles y un calentamiento no contabilizado, MAC1 válido sin cookie a50k/s:

| Métrica | R1 | R2 | R3 | R4 | R5 | Media | Peor |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Primer eco a↔b, s | 0,068 | 1,112 | 6,312 | 0,063 | 0,067 | 1,525 | 6,312 |
| Primer eco c↔b, s | 1,165 | 11,431 | 0,185 | 0,189 | 0,168 | 2,628 | 11,431 |
| INIT→RESP con MAC2 a↔b, ms | 0,794 | 995,744 | 1241,427 | 0,653 | 0,917 | 447,907 | 1241,427 |
| Máximo INIT/candidato/índice observado en a | 2 | 3 | 3 | 2 | 2 | 2,4 | 3 |

El outlier original de40s **no se reprodujo en0/5** de esta serie caliente, pero hubo reconexiones de6,3/11,4s y handshakes de~1,24s. No se elimina el outlier de la primera serie. Los status de a/b y las capturas se conservaron; la versión anterior no publicaba contadores de cookie.

### Después del arreglo: cinco repeticiones por tasa

Agente SHA-256 **`fd10f5b258a1b4ff34302cb77ccf610379b6a32acbae30d246704c0027d58f6f`**, recompilado offline/locked en34,85s junto a CLI y herramienta; versión0.1.0(d9ab7ec…) con árbol modificado. La reserva por par configurado es independiente del cubo global de desconocidos; el código de producto y sus pruebas corresponden a la entrega de agente.

| Tasa / métrica | R1 | R2 | R3 | R4 | R5 | Media | Peor |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1k · eco a↔b, s | 0,065 | 5,061 | 5,062 | 0,070 | 0,054 | 2,062 | 5,062 |
| 1k · eco c↔b, s | 0,171 | 0,180 | 2,417 | 0,171 | 0,175 | 0,623 | 2,417 |
| 1k · INIT→RESP MAC2 a↔b, ms | 0,763 | 0,654 | 0,586 | 0,606 | — | 0,652 (4) | 0,763 |
| 10k · eco a↔b, s | 0,064 | 0,064 | 0,064 | 0,042 | 10,567 | 2,160 | 10,567 |
| 10k · eco c↔b, s | 1,109 | 0,173 | 1,415 | 0,165 | 0,173 | 0,607 | 1,415 |
| 10k · INIT→RESP MAC2 a↔b, ms | 0,564 | 0,862 | 0,731 | 0,974 | 0,955 | 0,817 (5) | 0,974 |
| 50k · eco a↔b, s | 4,564 | 20,567 | 0,058 | 1,111 | 0,064 | 5,273 | 20,567 |
| 50k · eco c↔b, s | 14,669 | 0,166 | 0,171 | 25,472 | 0,169 | 8,129 | 25,472 |
| 50k · INIT→RESP MAC2 a↔b, ms | 1,561 (relay UDP) | — | 0,797 | 996,639 | 0,541 | 249,884 (4) | 996,639 |
| 50k · máximo INIT/candidato/índice en a | 5 | 1 | 2 | 3 | 2 | 2,6 | 5 |

`—` = no intercambio MAC2 visible, **no cero**. A1k R5 el eco llegó sin desafío observado; A50k R2 el primer INIT salió a21,810s desde el inicio de captura, después del flood. Esa repetición no demuestra handshake **durante** la inundación. En las13 muestras MAC2 emparejadas observadas el peor handshake fue996,639ms; todas quedan dentro del presupuesto propuesto de2s, no se extrapola un percentil a partir de13 muestras.

En1k R2 el primer INIT apareció a6,296s y su RESP0,654ms después; en10k R5, a11,811s y0,955ms después. Esto separa el intercambio criptográfico de una espera anterior a él. En50k R1 se vieron5 INIT directos por candidato y el intercambio con cookie por relay UDP1,561ms después de su primer INIT relay.

| Respondedor / tasa | CPU % R1/R2/R3/R4/R5 | Media / peor CPU % | RSS KiB R1/R2/R3/R4/R5 | Media / peor RSS |
| --- | --- | --- | --- | --- |
| 1k | 4,478/4,179/4,244/3,819/3,960 | 4,136/4,478 | 6856/6880/6880/6892/6896 | 6881/6896 |
| 10k | 11,118/11,455/11,636/10,685/11,246 | 11,228/11,636 | 6900/6908/6928/6928/6936 | 6920/6936 |
| 50k | 38,529/34,313/36,851/29,849/35,694 | 35,047/38,529 | 6940/6944/6948/6948/6948 | 6946/6948 |

Contadores finales acumulados de b: `init_challenged=7267453`, `tx_cookie_reply=37669`, `mac2_accepted=31`, `cookie_reply_ratelimited_unknown=7229784`. `cookie_reply_ratelimited_peer` no se emitió (cero incidencias observadas). a recibió1 cookie en4/5,5/5 y4/5 repeticiones, respectivamente. No son31 handshakes legítimos distintos: `mac2_accepted` cuenta el paso de la barrera MAC2, no autorización Noise. Los contadores completos a/b se guardan por repetición.

**Hallazgo de rendimiento independiente:** reconexión tras reinicio hasta~20–25,5s en esta topología, también con controles de15–20s sin flood. Hipótesis: sondeo/backoff y rendezvous; no se diagnostica más aquí ni se lo confunde con tardar20s en validar una cookie. El margen operativo debe incluir esta reconexión, y una aceptación de roaming rápida exige un ensayo específico.

Presupuestos separados propuestos: **B10 parada real<3s (≥5 muestras)**; **B11 INIT→RESP bajo carga≤2s** (margen sobre996,639ms); **B12 reconexión tras reinicio≤30s** para esta topología, como alarma del límite medido, no garantía WAN; **B13 respondedor≤60% de un núcleo y16MiB RSS a50k INIT/s** (margen sobre38,529% y6948KiB). El outlier histórico de40,6s queda fuera de B12 y no se borra para obtener verde.

## Resultados MEDIDOS: relay

Los ensayos completos de capacidad conservan las cuotas de asignación **por defecto** y tabla de tasas65536. La regresión de rotación IPv4 usa una instancia con tabla128, declarada explícitamente. No se equipara «4096 asignaciones ociosas» a «4096 pares con tráfico sostenido».

| Métrica | R1 | R2 | Media | Peor |
| --- | ---: | ---: | ---: | ---: |
| Rotación5000 IPv4 / cookies35B recibidas | 4843 | 4842 | 4842,5 | 4842 |
| Rotación / pared, s | 5,079 | 5,089 | 5,084 | 5,089 |
| Rotación / CPU relay % | 3,715 | 4,287 | 4,001 | 4,287 |
| Rotación / pico RSS KiB | 4792 | 4792 | 4792 | 4792 |
| IP nueva tras reposición | servida | servida | 2/2 | sin bloqueo permanente observado |
| 100 IPv6 del mismo/64 / cookies recibidas | 27 | 25 | 26 | 27 |
| Agrupación IPv6 / pared, s | 0,363 | 0,288 | 0,325 | 0,363 |
| Asignaciones simultáneas aceptadas | 4096 | 4096 | 4096 | 4096 |
| Intento4097 | Full | Full | 2/2 | Full |
| Llenado / pared, s | 16,406 | 8,205 | 12,305 | 16,406 |
| Llenado / CPU relay, s | 2,060 | 2,890 | 2,475 | 2,890 |
| Llenado / CPU relay % | 12,557 | 35,222 | 23,889 | 35,222 |
| Proceso vacío / RSS KiB | 4752 | 4760 | 4756 | 4760 |
| Con4096 asignaciones / RSS KiB | 7212 | 7416 | 7314 | 7416 |
| Incremento RSS KiB/asignación | 0,601 | 0,648 | 0,625 | 0,648 |
| Timeouts de COOKIE durante llenado | **5** | 0 | 2,5 | **5** |
| 1 par E2E,512MiB SHA / Mbit/s | 21,648 | 21,594 | 21,621 | 21,594 |
| 1 par E2E / pared, s | 198,404 | 198,900 | 198,652 | 198,900 |
| 1 par E2E / CPU relay % | 4,420 | 4,449 | 4,435 | 4,449 |
| 1 par E2E / RSS KiB | 4796 | 4796 | 4796 | 4796 |
| 16 pares sintéticos / DATA ofrecidos | 320000 | 320000 | 320000 | 320000 |
| 16 pares / DATA recibidos | 42644 | 57551 | 50097,5 | 42644 |
| 16 pares / Mbit/s recibidos | **40,938** | **55,248** | **48,093** | **40,938** |
| 16 pares / pérdida % | **86,674** | **82,015** | **84,345** | **86,674** |
| 16 pares / CPU relay % | 100,651 | 100,658 | 100,655 | 100,658 |
| 16 pares / pico RSS KiB | 7212 | 7416 | 7314 | 7416 |

La oferta agregada fue~32k DATA/s y307,2Mbit/s durante10,000s reales,32 asignaciones activas de4096 residentes; el drenaje añade0,3s a la ventana de CPU. Las respuestas IPv6 del mismo/64 cumplen20/s+ráfaga20 durante su ventana; se verificó además alta cardinalidad con4096/64 distintos al llenar asignaciones. No se ha medido una tabla llena de65536 orígenes IPv6: la regresión de tabla llena es la de128 entradas/5000 IPv4.

**Hallazgos abiertos, no ocultados por el arnés:**5 timeouts UDP de COOKIE en R1 de capacidad; el cliente sintético aplica como el cliente real hasta3 intentos y conserva el número de pérdidas. Un ensayo anterior sin reintento abortó al agotar ese timeout, y otro que mezclaba tabla128 con capacidad devolvió `RateLimited` antes de llegar al total: no eran evidencia de4096 simultáneas. El ensayo final usa tabla predeterminada65536 y comprobó el gauge4096 antes de afirmar capacidad. Una corrección de idempotencia del arnés cambia `ip addr add` por `replace` únicamente para sus direcciones sintéticas entre repeticiones.

La entrega agregada de16 pares **no admite una promesa de307Mbit/s sin pérdida**: se perdieron82–87% de los datagramas con CPU próxima a un núcleo. No se atribuye el punto exacto de pérdida a kernel, cuotas o implementación sin perfil/counters posteriores adicionales; tampoco se sustituye esta salida negativa por la primera muestra aislada de71,9Mbit/s. Es un resultado de estrés local y un cuello de botella observable que debe revisar el piloto.

Presupuestos locales propuestos: **B14 capacidad4096 y rechazo Full del4097**, RSS≤16MiB, crecimiento≤1KiB/asignación, llenado≤30s y timeoutsCOOKIE≤10 (alarma, no SLO de disponibilidad); **B15 oferta16pares/307Mbit/s: entrega≥30Mbit/s, CPU≤150%, RSS≤16MiB**. B15 solo detecta regresión respecto a este escenario ya saturado; **no es presupuesto de servicio aceptable con esa pérdida**. No se dispone aún de presupuesto sin pérdida para16pares en hardware de producción.

## Resultados MEDIDOS: coordinator, clientes firmados sin agentes

Serie `atenea-load-65019`: **dos repeticiones completas** por escala y por prueba de cuotas, todas con salida0 en el cliente de coordinator. La ejecución de stack terminó1 **después**, por un error del arnés de conexión PostgreSQL al iniciar el seed: `PGDATABASE` con una URI no seleccionó el host remoto; se corrigió pasando parámetros libpq separados por entorno, sin URI en argv ni salida. Ese fallo no se convierte en una medición del control plane.

`MAX_PRESENCES=10000` está configurado explícitamente en el overlay de carga; el valor de producto4096 no se cambió. Se verificaron **1000/5000/10000 residentes reales** por `/healthz` al final, en las dos repeticiones y también tras las cuotas. La oferta se limita deliberadamente por red para no convertir429 en throughput útil: son puntos de operación medidos, **no búsqueda del máximo absoluto**.

| Escala / métrica | R1 | R2 | Media | Peor |
| --- | ---: | ---: | ---: | ---: |
| 1k · announces/s útiles | 40,033 | 40,040 | 40,037 | 40,033 |
| 1k · announce pared, s | 24,979 | 24,975 | 24,977 | 24,979 |
| 1k · announce p95/p99 ms | 2,177/2,896 | 2,371/2,986 | 2,274/2,941 | 2,371/2,986 |
| 1k · rendezvous/s útiles | 8,095 | 8,099 | 8,097 | 8,095 |
| 1k · rendezvous p95 ms | 3,612 | 2,571 | 3,091 | 3,612 |
| 1k · polls/s útiles, wait1s | 7,352 | 7,355 | 7,353 | 7,352 |
| 1k · polls p95 ms | 1005,620 | 1005,292 | 1005,456 | 1005,620 |
| 1k · pico CPU announce % | 3 | 4 | 3,5 | 4 |
| 1k · pico RSS announce KiB | 30708 | 32744 | 31726 | 32744 |
| 5k · announces/s útiles | 200,030 | 200,030 | 200,030 | 200,030 |
| 5k · announce pared, s | 24,996 | 24,996 | 24,996 | 24,996 |
| 5k · announce p95/p99 ms | 1,710/3,073 | 10,697/44,907 | 6,203/23,990 | 10,697/44,907 |
| 5k · rendezvous/s útiles | 40,094 | 40,098 | 40,096 | 40,094 |
| 5k · rendezvous p95 ms | 2,404 | 3,846 | 3,125 | 3,846 |
| 5k · polls/s útiles, wait1s | 40,097 | 40,097 | 40,097 | 40,097 |
| 5k · polls p95 ms | 1002,643 | 1002,487 | 1002,565 | 1002,643 |
| 5k · pico CPU announce % | 14,5 | 19,667 | 17,084 | 19,667 |
| 5k · pico RSS announce KiB | 136248 | 139716 | 137982 | 139716 |
| 10k · announces/s útiles | 395,013 | 395,028 | 395,020 | 395,013 |
| 10k · announce pared, s | 25,316 | 25,315 | 25,315 | 25,316 |
| 10k · announce p95/p99 ms | 1,141/2,874 | 1,191/2,360 | 1,166/2,617 | 1,191/2,874 |
| 10k · rendezvous/s útiles | 79,092 | 79,092 | 79,092 | 79,092 |
| 10k · rendezvous p95 ms | 2,662 | 2,665 | 2,663 | 2,665 |
| 10k · polls/s útiles, wait1s | 71,891 | 71,872 | 71,882 | 71,872 |
| 10k · polls p95 ms | 1002,449 | 1003,177 | 1002,813 | 1003,177 |
| 10k · pico CPU announce % | 15 | 15 | 15 | 15 |
| 10k · pico RSS announce KiB | 262080 | 259516 | 260798 | 262080 |

**Cero errores en las cargas normales**: los1000/5000/10000 announces obtuvieron200 por repetición; rendezvous81/401/791 y polls80/400/790 también200. Los polls esperan voluntariamente1s cuando no hay eventos; su p95 no es latencia de despacho de un evento. RSS refleja el proceso caliente, incluidas cachés/noces y perfiles previos, **no heap privado atribuible solo a una presencia**.

En las **seis pruebas de cuota** se comprobaron429 por announce inmediato repetido, tercer poll del mismo dispositivo y poll adicional de la red con64 retenidos; otra red siguió obteniendo200 y los cupos se recuperaron después de terminar los polls. Los64 se abren espaciados0,2s y esperan30s; el rechazo adicional confirma el techo de red para `MAX_LONG_POLLS=256`. El cliente admite hasta200 peticiones pendientes, **no se afirma que200 polls estuvieran retenidos simultáneamente**. El techo global256 y cancelación masiva no se midieron como capacidad máxima; las pruebas se centran en2/dispositivo y64/red.

Presupuestos propuestos para estos puntos, con dos muestras mínimas: **B18 1k** ≥28 announces/s, ≥5,5 rendezvous/s, p99 announce≤6ms, CPU≤8%, RSS≤64MiB; **B19 5k** ≥140 announces/s, ≥28 rendezvous/s, p99 announce≤90ms (conserva el outlier44,9ms), CPU≤40%, RSS≤220MiB; **B20 10k** ≥276 announces/s, ≥55 rendezvous/s, p99 announce≤6ms, CPU≤30%, RSS≤384MiB. Polls wait1s: p95≤1200ms. Son alarmas de regresión Docker/Mac, no memoria recomendada de un VPS.


## MEDIDO · control plane y PostgreSQL: tres escalas, dos concurrencias

Artefactos: `tests/load/out/atenea-load-85467/{control/,control-summary.jsonl,processes.jsonl,services.jsonl,states.jsonl}`. Matriz completa:1k/5k/10k dispositivos, clientes50/200, lectura/emisión y dos repeticiones por combinación; los24 perfiles terminaron con cero errores. Hay7d de muestras de red y auditoría sintéticas; son filas reales en PostgreSQL17, no siete días de tráfico real. Firma AtDevice real y raíz/emisor del laboratorio; no son miles de agentes Noise. Véase [metodología de control](carga-control-metodologia.md).

Cada celda es **media / peor entre dos repeticiones**: menor req/s, mayor p95 y mayor pico de recursos. CPU/RSS son el máximo de **un proceso** del servicio en cada perfil; la columna PG no suma backends ni sus páginas compartidas. Los JSONL conservan todos los PIDs para inspección por proceso. CPU100% equivale a un núcleo. El RPS efectivo incluye el drenaje de solicitudes pendientes después de la ventana activa nominal de10s; no se elimina esa cola.

| Dispositivos | Clientes | Perfil | req/s | p95 ms | CP CPU% | CP RSS KiB | PG CPU% por proceso | PG RSS KiB por proceso |
|---|---|---|---|---|---|---|---|---|
|1000|50|Lectura|168.73 /151.99|453.20 /507.40|36.25 /38.00|18044 /21224|12.00 /13.00|48048 /67944|
|1000|50|Emisión|81.86 /81.44|760.23 /797.34|29.00 /30.00|18316 /22028|7.25 /7.50|65932 /68348|
|1000|200|Lectura|189.73 /189.48|1198.90 /1201.98|39.00 /39.50|22126 /22380|11.00 /11.00|67352 /68404|
|1000|200|Emisión|78.64 /76.38|2747.85 /2788.14|42.50 /44.00|21910 /21956|9.50 /10.00|67824 /68484|
|5000|50|Lectura|31.38 /26.52|3594.55 /4183.97|5.63 /6.00|21738 /21800|9.88 /10.50|139990 /153500|
|5000|50|Emisión|20.64 /19.74|2793.70 /2888.56|8.75 /10.00|21738 /21800|8.50 /10.00|152760 /153564|
|5000|200|Lectura|32.70 /29.30|8147.05 /9492.28|10.25 /11.50|23506 /23808|14.50 /15.00|152890 /153564|
|5000|200|Emisión|21.41 /21.37|9593.86 /9596.91|9.25 /11.00|23072 /23136|9.00 /10.50|153630 /153948|
|10000|50|Lectura|14.40 /12.20|7048.24 /7710.13|3.08 /3.50|22362 /22568|12.50 /15.00|180730 /212224|
|10000|50|Emisión|10.90 /10.51|5259.43 /5420.68|5.75 /7.00|22362 /22568|8.50 /9.50|182906 /211412|
|10000|200|Lectura|16.06 /15.86|15095.13 /15201.23|4.25 /5.00|23518 /24124|20.25 /23.00|149108 /156808|
|10000|200|Emisión|10.86 /10.58|19002.11 /19509.73|5.25 /6.00|23190 /23736|9.50 /10.00|154702 /162736|

**Hallazgo de rendimiento:** a10k/200 clientes, lectura alcanza p95≈15.2s y emisión≈19.5s, aun sin errores. No es una experiencia interactiva aceptada ni un SLO de producción. **SIN COMPROBAR:** causa exacta, planes SQL, esperas/pool y coste del generador. El CPU bajo del proceso CP y el máximo individual de un backend no descartan saturación agregada de PostgreSQL. No se modifica el motor ni se atribuye el resultado al VPS.

### Almacenamiento y WebSocket

Tamaño físico de BD al acabar cada repetición, después de WebSocket; incluye índices y espacio físico, no WAL/backups. Las tablas se conservan con su historial de actualización dentro de cada escala. No es una fórmula bytes/dispositivo para dimensionar un proveedor.

| Dispositivos | BD bytes R1 / R2 | Muestras de red bytes R1 / R2 | Auditoría bytes R1 / R2 | Dispositivos bytes R1 / R2 |
|---|---|---|---|---|
|1000|50960051 /51328691|22249472 /22249472|17620992 /17620992|1105920 /1105920|
|5000|227374771 /227767987|116932608 /116932608|93904896 /93904896|3710976 /3719168|
|10000|532321971 /532813491|248692736 /248692736|255090688 /255090688|9814016 /9814016|

**64 suscriptores simultáneos**, distribuidos conforme a la cuota de sesión, fueron admitidos64/64 y recibieron cambios reales64/64 en las seis ejecuciones. Errores/rechazos0. Eventos recibidos R1/R2:1k64/128,5k64/64,10k64/64. La entrega puede agrupar cambios: el criterio es que todos los clientes reciban un cambio, no un número fijo de frames. No mide fan-out mayor de64, latencia de evento ni clientes móviles.

### Presupuestos propuestos B21–B23, no aceptación interactiva

Por escala y perfil, dos muestras mínimas, errores0 y recursos presentes. Margen≈30% por debajo del menor RPS,≈2× sobre p95 observado; guardas de CPU/RSS redondeadas. **Un presupuesto local que pasa con19s de p95 no vuelve aceptable esa latencia.** Repetir en piloto con objetivos acordados antes de declarar soporte de5k/10k.

| Nº / escala | req/s mínimo lectura50 / emisión50 / lectura200 / emisión200 | p95 máximo ms, mismo orden | CP CPU% / RSS KiB | PG por proceso CPU% / RSS KiB | BD máximo bytes |
|---|---|---|---|---|---|
|B21 ·1k|106 /57 /132 /53|1020 /1600 /2410 /5580|90 /65536|30 /131072|103000000|
|B22 ·5k|18 /13 /20 /14|8370 /5780 /19000 /19200|30 /65536|40 /262144|456000000|
|B23 ·10k|8 /7 /11 /7|15430 /10850 /30410 /39020|20 /65536|50 /393216|1070000000|

WebSocket exige64 admitidos y64 receptores, errores/rechazos0, dos muestras por escala. Estos umbrales solo detectan regresiones del mismo laboratorio.

Recursos durante WebSocket, media / peor de picos por proceso entre dos repeticiones:

| Dispositivos | CP CPU% | CP RSS KiB | PG CPU% por proceso | PG RSS KiB por proceso |
|---|---|---|---|---|
|1000|0.42 /0.50|25938 /26684|0.83 /1.33|67882 /68484|
|5000|0.75 /1.00|27072 /27308|0.50 /0.50|153662 /153948|
|10000|0.25 /0.50|29416 /29452|0.50 /0.50|155388 /163976|


