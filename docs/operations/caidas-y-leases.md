# Caídas del stack y leases (fase 9)

## Reproducción y alcance

```sh
bash tests/stack/drills/run.sh lab
bash tests/stack/drills/run.sh pilot
```

Ejecutar **secuencialmente**. Requiere todas las imágenes y cachés del stack ya locales. El arnés no compila, instala ni publica; comprueba las imágenes antes del bootstrap y usa `--pull never` para Compose y `--pull=never` para los contenedores propios. Reutiliza el bootstrap, Compose de producción, overlay, OIDC, API y helpers NAT de `tests/stack`; no monta el socket Docker en el runner. Solo el controlador anfitrión puede parar/arrancar servicios del proyecto único `atenea-drill-*`. Antes de cada inyección imprime `docker ps -a` y `docker network ls`; al salir elimina únicamente su runner, proyecto Compose, volúmenes y runtime privado. Conserva imágenes, cachés y recursos anteriores.

Dos agentes reales con TUN, namespaces y NAT se enrolan/aprueban por API, obtienen configuración firmada y transmiten antes de inyectar fallos. El bloqueo `p2p` usa las mismas reglas nftables del laboratorio NAT; `udp` fuerza TLS 443 por HAProxy. Un segundo contenedor relay tiene nombre y clave TLS propios, certificado de la CA efímera y dirección independiente, y figura en `ATENEA_COORD_RELAYS`; comparte únicamente la clave **pública** de verificación del coordinator, no la clave privada TLS del principal. No representa un segundo proveedor real.

Tras el cutover relay-v2, `prepare.py` conserva IDs aleatorios distintos en `config/relay-id` y `config/relay2-id`, los conecta a sus respectivos TOML y genera `ATENEA_COORD_RELAYS` en `stack.env` con esas mismas audiencias. Reiniciar los contenedores no regenera IDs. Cada relay recibe su propio token en la concesión; la disponibilidad del secundario ya no depende de reutilizar un token del principal. Las mediciones y salidas de este documento son **históricas, anteriores a relay-v2**: no verifican los fixtures nuevos ni ALPN `/2`, y no se han repetido durante este cutover.

Cada escenario exige una transferencia TCP de 1 000 000 bytes con SHA-256 coincidente antes de inyectar el fallo. Durante todo el ensayo se envía ping continuo con registro `PING {sent, done, ok}` en segundos Unix de reloj de pared, sin detener la sonda al parar servicios. Un éxito se registra al volver `ping`; un fallo lleva el instante de envío y hasta 1 s de timeout. Cadencia nominal 0,2 s con respuesta; durante pérdidas, aproximadamente 1,2 s. Se publican el último éxito, el envío del primer fallo posterior y su fin: el envío es un **estimador**, no una cota superior exacta del corte; el timeout limita la precisión. `valid_until_unix` se lee del estado IPC de **ambos** agentes después de parar el emisor; manda el lease que antes caduque, más 30 s de tolerancia. Las mediciones de recuperación parten del fin de `docker compose start` (y reposición de ruta emulada para coordinator), salvo la conmutación de relay, que parte del inicio de `stop` e incluye la parada.

Perfiles: laboratorio **60/20/30 s** (lease/sondeo de reemisión/tolerancia); piloto **300/60/30 s**. Sondeo y reemisión no se confunden con los 20 s de announce del coordinator. Las revocaciones que no llegaron al control plane no se consideran encoladas: se comprueba que el dispositivo sigue aprobado al volver y se ejecuta una revocación real al final.

**SIN COMPROBAR:** red real/proveedor, relojes desincronizados entre nodos y cargas altas. Todo corre en un único kernel Linux de Docker Desktop y comparte reloj; no se deduce HA multi-proveedor ni precisión de reloj distribuido.

**Aplicado (2026-10-26):** el agente renueva anticipadamente la concesión relay
con re-ALLOC in situ y conserva presencia/candidatos pendientes tras errores
de announce, con coalescencia, backoff y cuotas sin ampliar. Véanse los plazos
en [README del agente](../../apps/agent/README.md#transporte-estado-y-recuperación).
Esto no modifica las cifras históricas siguientes ni acredita un nuevo ensayo
Docker, de conmutación entre relays o de disponibilidad en producción.

## Resultados medidos: laboratorio 60/20/30

Ejecución final `atenea-drill-14891`, salida **0**, **1340,45 s de pared**, sin reiniciar agentes. Dos repeticiones por caída de relay/coordinator/HAProxy y por componente emisor (una directa y una relay UDP); la combinada se hizo una vez. Las transiciones deliberadas para preparar el camino no se cuentan como pérdida causada por la caída. Una transferencia íntegra precedió a cada inyección; los conteos de pérdida son **sondas**, no porcentaje de paquetes de carga.

| Escenario | Qué se mantiene | Qué se pierde | Tiempo medido, media / peor (s) | Contraste con ADR-009 §2 |
| --- | --- | --- | --- | --- |
| Relay principal parado, P2P bloqueado | Proceso y sesión de agentes; relay secundario disponible | Tráfico hasta conmutar: 18 sondas fallidas en cada repetición | Conmutación **22,076 / 22,099** | La conmutación existe y es automática; no es instantánea ni ≤10 s. No se cambió Rust |
| Coordinator parado, directo establecido | 120/120 sondas correctas en dos ventanas de ≈12,35 s | Rendezvous del nuevo agente falla durante la caída | Presencia **4,811 / 8,593**; nuevo par **19,404 / 27,770** tras volver | Presencia <20 s; conectar un par no equivale a reconstruir presencia |
| Coordinator parado, relay UDP establecido | 119/119 sondas correctas en dos ventanas de ≈12,35 s | Nuevo rendezvous durante la caída | Presencia **8,492 / 15,966**; nuevo par **26,520 / 32,214** | Contradice la lectura «no relay hasta volver»: el relay establecido sigue; se corrigió solo esa frase del ADR |
| Control plane parado | Directo/relay UDP hasta el lease + tolerancia | Renovación, API de revocación y, al expirar, tráfico | Corte estimado a `valid_until +30 s`; reanudación automática **6,244 / 6,356** tras start | Fail-closed confirmado en ambos caminos |
| PostgreSQL parado | Directo/relay UDP hasta el lease + tolerancia | Renovación y revocación aunque siga vivo el proceso del control plane | Corte estimado a `valid_until +30 s`; reanudación automática **6,819 / 6,945** | La BD es dependencia del emisor, no del tráfico establecido |
| HAProxy parado, directo | 79/79 sondas correctas en ventanas de ≈8,1 s | Panel | Panel comprobado de nuevo y sonda nueva en **0,208 / 0,212** después de start | Aislamiento previsto; vuelta medida sin tocar agentes |
| HAProxy parado, relay UDP | 77/77 sondas correctas en ventanas de ≈8,1 s | Panel | Panel comprobado y sonda nueva en **0,211 / 0,212** | UDP no depende del proceso HAProxy |
| HAProxy parado, relay TLS 443 | Agentes y leases vivos | Panel y tráfico TLS: 6 sondas fallidas por ventana de caída; 13/40 más al recuperar | Recuperación automática TLS **32,678 / 48,993** después de start | Cumple aislamiento tras corregir Compose; recuperación variable, no inmediata |
| Relay + coordinator parados | Token ya obtenido permite abrir el segundo relay | Tráfico hasta conmutar; no nuevos rendezvous | **22,269** (n=1; 18 sondas fallidas) | La lista y el token existentes permiten failover aun sin coordinator durante esta ventana; no demuestra renovación indefinida |
| Revocación al volver el emisor | API vuelve y acepta revocación real | Tráfico del dispositivo revocado | **20,779** (n=1) desde petición hasta primera pérdida detectada | Las peticiones fallidas durante la caída **no se encolaron**: b siguió aprobado hasta la revocación explícita |

`start` incluye el arranque de contenedor; se publica aparte como `EVENT` con `began/ended`. Para coordinator, `ended` incluye reponer la ruta **del laboratorio** a las NAT emuladas, que Docker pierde al recrear su namespace. Esto no reinicia ni reconfigura agentes y no es una ruta necesaria en el despliegue real. HAProxy ya no requiere reponer rutas al reiniciar: pertenecen a netns.

### Corte de lease: timestamps reales

Ambos agentes tenían el mismo `valid_until` en estas cuatro repeticiones. `último OK` es recepción de respuesta; `primer KO enviado` es el inicio de la sonda que luego agotó timeout. Es un estimador de corte de cadencia ≈0,2 s, **no una cota física exacta**: la cota superior conservadora es el fin del primer KO (≈1 s después). Por ejemplo, una sonda enviada 4 ms antes del límite puede ser procesada después. Todos los límites son compatibles con `now < valid_until +30` de ADR-002; se comprobó ausencia de tráfico pasado el límite.

| Emisor parado / camino | `valid_until` Unix | Último OK Unix | Primer KO enviado Unix | Fin primer KO Unix | Estimador relativo a `valid_until` (s) |
| --- | --- | --- | --- | --- | --- |
| Control plane / directo | 1792933617 | 1792933646,9718604 | 1792933647,1724668 | 1792933648,1867833 | 29,972 → 30,172 |
| Control plane / relay UDP | 1792933757 | 1792933786,8950987 | 1792933787,0959668 | 1792933788,1000922 | 29,895 → 30,096 |
| PostgreSQL / directo | 1792933897 | 1792933926,7881892 | 1792933926,9959388 | 1792933928,0079281 | 29,788 → 29,996 |
| PostgreSQL / relay UDP | 1792934037 | 1792934066,8669124 | 1792934067,0693170 | 1792934068,0748296 | 29,867 → 30,069 |

Media de envío del primer KO respecto a `valid_until`: **30,083 s**; peor **30,172 s**. No se usa esa media para prometer precisión por debajo del timeout/cadencia.

### Salida funcional pegada del laboratorio

Extracto literal de `RESULT` y `COMPLETE` de la ejecución final; se omiten creación/health de Docker y el resto de sondas, no resultados negativos:

```text
RESULT {"began": 1792932811.409037, "elapsed": 22.054, "ended": 1792932833.4634402, "failed": 18, "probes": 20, "relay": "secundario", "repetition": 1, "scenario": "relay-failover"}
RESULT {"began": 1792932891.7158709, "elapsed": 22.099, "ended": 1792932913.8146956, "failed": 18, "probes": 19, "relay": "secundario", "repetition": 2, "scenario": "relay-failover"}
RESULT {"began": 1792932955.9348989, "elapsed": 12.358, "ended": 1792932968.293137, "failed": 0, "new_rendezvous": false, "probes": 60, "repetition": 1, "scenario": "coordinator-direct"}
RESULT {"began": 1792932998.617954, "elapsed": 12.34, "ended": 1792933010.9577856, "failed": 0, "new_rendezvous": false, "probes": 60, "repetition": 2, "scenario": "coordinator-direct"}
RESULT {"began": 1792932968.625386, "elapsed": 8.593, "ended": 1792932977.2184205, "failed": 0, "probes": 42, "repetition": 1, "scenario": "coordinator-presence-direct"}
RESULT {"began": 1792933011.278201, "elapsed": 1.03, "ended": 1792933012.3083966, "failed": 0, "probes": 5, "repetition": 2, "scenario": "coordinator-presence-direct"}
RESULT {"began": 1792932968.625386, "elapsed": 11.037, "ended": 1792932979.6626806, "failed": 0, "last_announce": 1792932977, "probes": 53, "repetition": 1, "scenario": "coordinator-new-peer-direct"}
RESULT {"began": 1792933011.278201, "elapsed": 27.77, "ended": 1792933039.0478978, "failed": 0, "last_announce": 1792933037, "probes": 135, "repetition": 2, "scenario": "coordinator-new-peer-direct"}
RESULT {"began": 1792933068.6674938, "elapsed": 12.35, "ended": 1792933081.0177543, "failed": 0, "new_rendezvous": false, "probes": 59, "repetition": 1, "scenario": "coordinator-relay-udp"}
RESULT {"began": 1792933118.67767, "elapsed": 12.342, "ended": 1792933131.0201552, "failed": 0, "new_rendezvous": false, "probes": 60, "repetition": 2, "scenario": "coordinator-relay-udp"}
RESULT {"began": 1792933081.3479922, "elapsed": 15.966, "ended": 1792933097.3142397, "failed": 0, "probes": 77, "repetition": 1, "scenario": "coordinator-presence-relay-udp"}
RESULT {"began": 1792933131.338578, "elapsed": 1.018, "ended": 1792933132.3567524, "failed": 0, "probes": 5, "repetition": 2, "scenario": "coordinator-presence-relay-udp"}
RESULT {"began": 1792933081.3479922, "elapsed": 20.826, "ended": 1792933102.17409, "failed": 0, "last_announce": 1792933097, "probes": 101, "repetition": 1, "scenario": "coordinator-new-peer-relay-udp"}
RESULT {"began": 1792933131.338578, "elapsed": 32.214, "ended": 1792933163.552514, "failed": 0, "last_announce": 1792933157, "probes": 156, "repetition": 2, "scenario": "coordinator-new-peer-relay-udp"}
RESULT {"began": 1792933220.032306, "elapsed": 8.142, "ended": 1792933228.1746616, "failed": 0, "panel": false, "probes": 40, "repetition": 1, "scenario": "haproxy-direct"}
RESULT {"began": 1792933230.756344, "elapsed": 8.103, "ended": 1792933238.8596175, "failed": 0, "panel": false, "probes": 39, "repetition": 2, "scenario": "haproxy-direct"}
RESULT {"began": 1792933229.888588, "elapsed": 0.212, "ended": 1792933230.1010249, "failed": 0, "probes": 1, "repetition": 1, "scenario": "haproxy-recovery-direct"}
RESULT {"began": 1792933240.5790322, "elapsed": 0.204, "ended": 1792933240.7834692, "failed": 0, "probes": 1, "repetition": 2, "scenario": "haproxy-recovery-direct"}
RESULT {"began": 1792933251.359083, "elapsed": 8.122, "ended": 1792933259.4807298, "failed": 0, "panel": false, "probes": 38, "repetition": 1, "scenario": "haproxy-relay-udp"}
RESULT {"began": 1792933262.083245, "elapsed": 8.1, "ended": 1792933270.1832583, "failed": 0, "panel": false, "probes": 39, "repetition": 2, "scenario": "haproxy-relay-udp"}
RESULT {"began": 1792933261.1703308, "elapsed": 0.212, "ended": 1792933261.3824184, "failed": 0, "probes": 1, "repetition": 1, "scenario": "haproxy-recovery-relay-udp"}
RESULT {"began": 1792933271.876936, "elapsed": 0.211, "ended": 1792933272.0878942, "failed": 0, "probes": 1, "repetition": 2, "scenario": "haproxy-recovery-relay-udp"}
RESULT {"began": 1792933276.9001348, "elapsed": 11.106, "ended": 1792933288.0059621, "failed": 6, "panel": false, "probes": 20, "repetition": 1, "scenario": "haproxy-relay-tls"}
RESULT {"began": 1792933306.953413, "elapsed": 11.124, "ended": 1792933318.0772989, "failed": 6, "panel": false, "probes": 21, "repetition": 2, "scenario": "haproxy-relay-tls"}
RESULT {"began": 1792933289.7033758, "elapsed": 16.364, "ended": 1792933306.0670707, "failed": 13, "probes": 14, "repetition": 1, "scenario": "haproxy-recovery-relay-tls"}
RESULT {"began": 1792933319.756701, "elapsed": 48.993, "ended": 1792933368.7498639, "failed": 40, "probes": 41, "repetition": 2, "scenario": "haproxy-recovery-relay-tls"}
RESULT {"began": 1792933520.050422, "elapsed": 22.269, "ended": 1792933542.319621, "failed": 18, "probes": 20, "relay": "secundario", "scenario": "relay+coordinator"}
RESULT {"began": 1792933557.966027, "cutoff_after_valid_until": [29.972, 30.172], "elapsed": 93.194, "ended": 1792933651.1596363, "failed": 3, "first_failed": 1792933647.1724668, "last_ok": 1792933646.9718604, "path": "direct", "probes": 431, "profile": "lab", "repetition": 1, "scenario": "lease-control-plane", "valid_until": {"a": 1792933617, "b": 1792933617}}
RESULT {"began": 1792933697.892431, "cutoff_after_valid_until": [29.895, 30.096], "elapsed": 93.274, "ended": 1792933791.1662982, "failed": 3, "first_failed": 1792933787.0959668, "last_ok": 1792933786.8950987, "path": "relay-udp", "probes": 429, "profile": "lab", "repetition": 2, "scenario": "lease-control-plane", "valid_until": {"a": 1792933757, "b": 1792933757}}
RESULT {"began": 1792933651.880681, "elapsed": 6.356, "ended": 1792933658.237109, "failed": 5, "probes": 6, "profile": "lab", "repetition": 1, "scenario": "lease-recovery-control-plane"}
RESULT {"began": 1792933791.888463, "elapsed": 6.132, "ended": 1792933798.0206316, "failed": 5, "probes": 6, "profile": "lab", "repetition": 2, "scenario": "lease-recovery-control-plane"}
RESULT {"began": 1792933837.8843431, "cutoff_after_valid_until": [29.788, 29.996], "elapsed": 93.147, "ended": 1792933931.0309427, "failed": 3, "first_failed": 1792933926.9959388, "last_ok": 1792933926.7881892, "path": "direct", "probes": 430, "profile": "lab", "repetition": 1, "scenario": "lease-postgres", "valid_until": {"a": 1792933897, "b": 1792933897}}
RESULT {"began": 1792933978.0305128, "cutoff_after_valid_until": [29.867, 30.069], "elapsed": 93.167, "ended": 1792934071.197258, "failed": 3, "first_failed": 1792934067.069317, "last_ok": 1792934066.8669124, "path": "relay-udp", "probes": 429, "profile": "lab", "repetition": 2, "scenario": "lease-postgres", "valid_until": {"a": 1792934037, "b": 1792934037}}
RESULT {"began": 1792933931.214154, "elapsed": 6.945, "ended": 1792933938.159308, "failed": 5, "probes": 7, "profile": "lab", "repetition": 1, "scenario": "lease-recovery-postgres"}
RESULT {"began": 1792934071.405153, "elapsed": 6.692, "ended": 1792934078.0974998, "failed": 5, "probes": 6, "profile": "lab", "repetition": 2, "scenario": "lease-recovery-postgres"}
RESULT {"began": 1792934078.0980303, "elapsed": 20.779, "ended": 1792934098.8767114, "failed": 1, "probes": 96, "scenario": "revocation-after-recovery"}
COMPLETE {"profile": "lab", "wall": 1792934098.8779557}
DRILL cleanup project=atenea-drill-14891 exit=0; pulls=0 (pull_policy=never/--pull=never; imágenes precomprobadas)
```

## Resultados medidos: piloto 300/60/30

Ejecución secuencial posterior `atenea-drill-19840`, salida **0**, **1900,56 s de pared**. Se hicieron **cuatro** caídas largas reales, no se extrapoló desde laboratorio: control plane y PostgreSQL, cada uno una vez con directo y otra con relay UDP. En cada caso b tenía un lease un segundo posterior al de a; el corte corresponde al lease mínimo de a. Se observó tráfico durante aproximadamente 329 s después de iniciar la parada, y ausencia después de expirar. No se cambiaron relojes ni se aceleraron temporizadores.

| Escenario | Se mantiene / se pierde | Tiempo medido, media / peor (s) | ADR-009 §2 / ADR-002 |
| --- | --- | --- | --- |
| Control plane caído (n=2) | Directo y relay siguen hasta caducar; no renovación ni revocación | Primer KO enviado respecto al lease mínimo: **30,138 / 30,150**; reanudación **30,060 / 32,208** tras start | Fail-closed, compatible con máximo nominal 330 s desde última emisión; vuelve sin intervención en agentes |
| PostgreSQL caído (n=2) | Misma continuidad y corte aun con proceso del control plane vivo | Primer KO enviado respecto al lease mínimo: **30,133 / 30,170**; reanudación **28,511 / 28,516** | Fail-closed también por caída de BD; conexiones de aplicación se restablecen solas |
| Revocación solicitada al volver (n=1) | Peticiones fallidas durante la caída no se encolaron; b seguía aprobado | **59,879** hasta primera pérdida detectada | Compatible con sondeo de 60 s; no hay revocación online cuando el emisor no puede atender |

| Emisor / camino | `valid_until` a / b (Unix) | Último OK Unix | Primer KO enviado Unix | Fin primer KO Unix |
| --- | --- | --- | --- | --- |
| Control plane / directo | 1792934501 / 1792934502 | 1792934530,9452403 | 1792934531,1500000 | 1792934532,1549046 |
| Control plane / relay UDP | 1792934921 / 1792934922 | 1792934950,9199622 | 1792934951,1245077 | 1792934952,1333160 |
| PostgreSQL / directo | 1792935461 / 1792935462 | 1792935490,8928890 | 1792935491,0948880 | 1792935492,1007843 |
| PostgreSQL / relay UDP | 1792935881 / 1792935882 | 1792935910,9690833 | 1792935911,1698422 | 1792935912,1724527 |

Media de envío del primer KO respecto al lease mínimo: **30,135 s**, peor **30,170 s**, con la precisión/timeout descritos arriba. En este perfil recuperar tarda ≈28–32 s frente a ≈6–7 s de laboratorio; no se promete disponibilidad continua más allá del lease ni recuperación instantánea del emisor.

Salida literal funcional del piloto:

```text
REVOCATION-UNAVAILABLE {"error": "HTTPError", "service": "control-plane", "wall": 1792934206.0847425}
RESULT {"began": 1792934202.827308, "cutoff_after_valid_until": [29.945, 30.15], "elapsed": 332.22, "ended": 1792934535.047051, "failed": 3, "first_failed": 1792934531.15, "last_ok": 1792934530.9452403, "path": "direct", "probes": 1587, "profile": "pilot", "repetition": 1, "scenario": "lease-control-plane", "valid_until": {"a": 1792934501, "b": 1792934502}}
RESULT {"began": 1792934535.792138, "elapsed": 32.208, "ended": 1792934567.999774, "failed": 26, "probes": 27, "profile": "pilot", "repetition": 1, "scenario": "lease-recovery-control-plane"}
REVOCATION-UNAVAILABLE {"error": "HTTPError", "service": "control-plane", "wall": 1792934626.097084}
RESULT {"began": 1792934622.848736, "cutoff_after_valid_until": [29.92, 30.125], "elapsed": 332.302, "ended": 1792934955.1509993, "failed": 3, "first_failed": 1792934951.1245077, "last_ok": 1792934950.9199622, "path": "relay-udp", "probes": 1576, "profile": "pilot", "repetition": 2, "scenario": "lease-control-plane", "valid_until": {"a": 1792934921, "b": 1792934922}}
RESULT {"began": 1792934955.8743198, "elapsed": 27.913, "ended": 1792934983.7874796, "failed": 23, "probes": 24, "profile": "pilot", "repetition": 2, "scenario": "lease-recovery-control-plane"}
REVOCATION-UNAVAILABLE {"error": "HTTPError", "service": "postgres", "wall": 1792935163.080607}
RESULT {"began": 1792935162.8667672, "cutoff_after_valid_until": [29.893, 30.095], "elapsed": 332.258, "ended": 1792935495.1244884, "failed": 3, "first_failed": 1792935491.094888, "last_ok": 1792935490.892889, "path": "direct", "probes": 1587, "profile": "pilot", "repetition": 1, "scenario": "lease-postgres", "valid_until": {"a": 1792935461, "b": 1792935462}}
RESULT {"began": 1792935495.44011, "elapsed": 28.505, "ended": 1792935523.9452367, "failed": 23, "probes": 25, "profile": "pilot", "repetition": 1, "scenario": "lease-recovery-postgres"}
REVOCATION-UNAVAILABLE {"error": "HTTPError", "service": "postgres", "wall": 1792935583.1133165}
RESULT {"began": 1792935582.884522, "cutoff_after_valid_until": [29.969, 30.17], "elapsed": 332.23, "ended": 1792935915.1149013, "failed": 3, "first_failed": 1792935911.1698422, "last_ok": 1792935910.9690833, "path": "relay-udp", "probes": 1584, "profile": "pilot", "repetition": 2, "scenario": "lease-postgres", "valid_until": {"a": 1792935881, "b": 1792935882}}
RESULT {"began": 1792935915.396478, "elapsed": 28.516, "ended": 1792935943.9129438, "failed": 23, "probes": 24, "profile": "pilot", "repetition": 2, "scenario": "lease-recovery-postgres"}
RESULT {"began": 1792935943.9141114, "elapsed": 59.879, "ended": 1792936003.7929387, "failed": 1, "probes": 284, "scenario": "revocation-after-recovery"}
COMPLETE {"profile": "pilot", "wall": 1792936003.7943103}
DRILL cleanup project=atenea-drill-19840 exit=0; pulls=0 (pull_policy=never/--pull=never; imágenes precomprobadas)
```

## Hallazgo corregido: namespace de HAProxy

El Compose anterior hacía de HAProxy el dueño del namespace de relay (`network_mode: service:haproxy`). En la ejecución `atenea-drill-8770`, parar/arrancar el contenedor HAProxy conservó tráfico directo y relay UDP durante la caída, pero después de varios reinicios no se consiguió volver a establecer TLS en **180 s**. Los agentes seguían vivos con lease activo y coordinator accesible. Docker había sustituido el namespace del contenedor dueño; relay permanecía unido al anterior. Es un fallo real del diseño del Compose, **no una corrección Rust**.

La corrección añade un dueño estable `netns` con la imagen HAProxy ya local, sin capacidades ni secretos, UID 99 y `sleep infinity`; HAProxy y relay se unen a él. Redes, alias y 443 TCP/UDP pertenecen a netns; 8443 sigue solo en loopback y PROXY v2 sigue restringido a loopback. Se adaptan overlay de laboratorio, observabilidad/Prometheus, validación del rol Ansible y pruebas de infraestructura. Reiniciar netns queda fuera del aislamiento: si se recrea, hay que recrear ambos consumidores. No se convierte esa pieza en HA.

## Hallazgos abiertos

### Fallo multipar: dos defectos de sondeo corregidos; atribución histórica pendiente

- **Escenario exacto:** proyecto `atenea-drill-11879`, seis agentes acumulados: `a`, `b`, `c2`, `c3`, `c4`, `c5`. Cada `c*` se añadió para una repetición de caída del coordinator; no se habían retirado los anteriores. Falló el nuevo par **a↔c5** en la segunda repetición con P2P bloqueado (`net_block_p2p`); tráfico a↔b ya establecido por relay UDP. Topología home, todos los `c*` detrás de NAT1 con IPv4 LAN distinta. Lease/sondeo/tolerancia **60/20/30 s**; announce 20 s. El coordinator se paró, c5 se arrancó durante la caída y su primer ping falló; después se arrancó coordinator, se repuso la ruta de retorno del laboratorio y se esperó presencia antes de intentar el nuevo par.
- **Revisión:** HEAD registrado al redactar el informe `3b26ab97070bd4d411752fddaa96772eba8604af`, con cambios de trabajo sin commit. Imágenes efectivamente usadas: coordinator `sha256:ee91a94861b2c9c9635dd2e04f13b58096add0fa695718a8b365801ba997bf48`, relay `sha256:a1c38018cca748289028494978d65ee0230e31008ea125c83616ff55c7e855f0`, control plane `sha256:bb7777cd6af45aeb318280d86505d2a87e5618415edb88c23f3ca78335397218`; runner `sha256:371610f0c57e6971dfd76a68cb0d4d6e21d8f044c9cac536b1fa56ee60f6be2e`. Los binarios de agentes/CLI provenían del volumen local `atenea-lab-target`; **no se verificó su correspondencia exacta con ese commit**.
- **Síntoma:** el nuevo par quedó `probing` durante el plazo de **150 s**, sin handshake y con asignación relay `peer_ready=true`; a↔b siguió respondiendo. El bucle agotó su plazo y el ensayo salió con error, no con PASS. No se atribuye causa.
- **Estado acotado observado** en a a Unix `1792932709`: lease activo, `valid_until_unix=1792932755`, `sync.last_error=null`; coordinator `announces=27`, `last_announce_ok_unix=1792932695`, `last_error=null`, `rendezvous=47`; peer pendiente `path=probing`, `session=null`, `last_handshake_secs_ago=null`, `rx_bytes=0`, `tx_bytes=0`; su asignación `state=ready`, `peer_ready=true`, `relay=principal`, `pings_sent=26`, `pings_lost=0`, `rx_packets=1`, `tx_packets=1`. El par a↔b tenía `path=relay-udp`, sesión activa y `last_rx_secs=0`. Es un extracto del estado, no una captura de todos los paquetes.

Salida acotada real del coordinator y del plazo agotado:

```text
coordinator-1 | 2026-10-25T12:47:16.783028Z INFO atenea_coordinator: coordinator preparado
coordinator-1 | 2026-10-25T12:47:51.309628Z INFO atenea_coordinator: coordinator preparado
coordinator-1 | 2026-10-25T12:48:44.106698Z INFO atenea_coordinator: coordinator preparado
coordinator-1 | 2026-10-25T12:49:18.444885Z INFO atenea_coordinator: coordinator preparado
AssertionError: Tiempo agotado esperando condición
DRILL cleanup project=atenea-drill-11879 exit=1; pulls=0 (pull_policy=never/--pull=never; imágenes precomprobadas)
```

En aquella ejecución no se capturó el estado IPC de c5 ni el estado interno del coordinator. El arnés de caídas final retira/revoca cada par nuevo entre repeticiones para mantener dos agentes estables más uno nuevo; eso no constituye una corrección del producto ni cobertura multipar. La investigación posterior siguiente conserva esta evidencia histórica sin atribuirle una causa no observada.

### Investigación multipar posterior (2026-10-25)

**Laboratorio barato:** `tests/network-lab/run-multi-peer.sh` ejecuta secuencialmente un
único contenedor `atenea-lab` por caso, con `--pull=never --network none`, TUN real,
namespaces/NAT de `nat/lib.sh`, coordinator/STUN y relay reales compilados de las fuentes
actuales. No requiere stack/OIDC/BD/HAProxy. `a` y los `c*` comparten NAT1; `b` usa NAT2.
Todos los pares tienen configuración firmada mutua y ping cada 0,5 s; el estado se muestrea
cada ~1 s. `PAIR` registra ambos caminos y tiempo desde iniciar las sondas; `STATUS`
guarda el IPC completo de cada agente; `RELAY`, `COORDINATOR` y `LOG` guardan contadores,
métricas y hasta 12 líneas de log por proceso. No publica claves privadas ni tokens.
Los bloqueos también se copian a cada namespace adicional: el helper original solo
cubre `a`/`b`/routers y dejaría directo entre dos `c*` de la misma LAN.

Las configuraciones duran 900 s y no se renuevan durante el caso (máximo 155 s):
se aísla establecimiento de caminos, **no** se repite el lease 60/20/30 ni la secuencia
de cuatro reinicios del coordinator del hallazgo histórico. No hay revocaciones,
retiradas, cambios de reloj ni rutas Docker de retorno que reponer.

```sh
# Compilar solo desde cachés; no modificar tags de imágenes locales.
docker ps -a
docker run --pull=never --rm --name atenea-multi-build --network none \
  -v "$PWD:/src:ro" -v atenea-lab-target:/cache \
  -v atenea-cargo-registry:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/cache/target -e CARGO_BUILD_JOBS=2 -w /src atenea-lab \
  cargo build --release --locked --offline \
  -p atenea-agent -p atenea-cli -p atenea-coordinator -p atenea-relay
set -o pipefail
bash tests/network-lab/run-multi-peer.sh | tee /tmp/atenea-multi-before.log
# Tras la corrección: una repetición N=6/8, dimensionando la opción ya existente.
ATENEA_RELAY_MAX_PER_IP=64 bash tests/network-lab/run-multi-peer.sh 6 8 \
  | tee /tmp/atenea-multi-after.log
```

**OBSERVADO antes de corregir** (una pasada por celda, límite por IP 32):

| N | P2P | Pares establecidos / total | Primer / último establecimiento (s) | Fin del caso (s) |
| --- | --- | --- | --- | --- |
| 3 | permitido | 3/3 | 21,229 / 22,238 | 22,282 |
| 3 | bloqueado | 3/3 | 30,358 / 30,358 | 30,415 |
| 4 | permitido | 6/6 | 20,224 / 27,311 | 27,372 |
| 4 | bloqueado | 6/6 | 25,283 / 46,509 | 46,563 |
| 6 | permitido | 15/15 | 21,289 / 47,692 | 47,732 |
| 6 | bloqueado | 15/15 | 25,308 / 77,055 | 77,100 |
| 8 | permitido | 28/28 | 21,381 / 57,969 | 58,023 |
| 8 | bloqueado | **17/28** | 25,425 / 46,794 (solo los logrados) | **155,819: incompleto** |

El ensayo N=6 no reprodujo los 150 s históricos: no es un fallo determinista de
cardinalidad seis. Una pasada por celda no caracteriza estadísticamente intermitencia,
orden de arranque ni sensibilidad a los intervalos. Las claves son nuevas por caso.
El primer controlador basal fue editado antes de que terminara su última lectura:
tras emitir los ocho `RESULT`, imprimió `none: command not found`; esa salida exterior
no se usa como PASS. Los estados/resultados de los ocho contenedores sí quedaron
guardados; el controlador definitivo no se modifica mientras está ejecutándose y
propaga el fallo de cualquier caso incompleto.

**Aislamiento:**

- **(a) Coordinator:** N=6 bloqueado: 107 rendezvous HTTP 200 y 93 HTTP 429; 15
  solicitudes relay HTTP 200, cero rechazos de tokens `rate_limited` y
  `peer_unavailable`. N=8 bloqueado: 205/264 rendezvous 200/429; relay 45/85
  HTTP 200/429; `peer_unavailable=0`, presencia=8. Los límites contribuyen a
  los tiempos, pero no explican por sí solos un handshake pendiente con relay listo.
  El límite relay de diez solicitudes/minuto cuenta **intentos del solicitante**,
  incluso si el par o la ventana compartida después rechazan. Como techo ideal:
  diez pares nuevos/minuto/dispositivo si basta una solicitud por par; reintentos
  y solicitudes simultáneas de ambos extremos consumen presupuesto sin crear pares.
  Cada par no dirigido admite una emisión cada 5 s y dos emisiones/minuto
  (cuatro tokens, dos por emisión). No cambió ninguna cuota.
- **(b) Motor, dos bugs demostrados con pruebas unitarias que fallaron antes:**
  `relay_handshake_retries_after_the_first_datagrams_are_lost` obtuvo `probing`
  en vez de `relay-udp` tras 3 s cuando se perdían los envíos iniciales; el temporizador
  retransmitía msg1 **solo por directo**, aunque el relay estuviera listo.
  `repeated_candidates_do_not_extend_a_pending_probe` demostró que cada aviso
  reiniciaba `started` y `rounds` del mismo intento, anulando sus límites 10 s/12 rondas.
  Ahora se retransmite el **mismo msg1** también por el mejor relay listo en cada ronda;
  nuevos candidatos adelantan el próximo envío sin renovar ventana ni presupuesto.
  Se conservan Noise, desempate por clave, antirreplay, leases y límites de solicitudes.
  No se cambia el formato ni la autorización del protocolo: se cumplen límites y
  caminos ya documentados. **INFERENCIA:** ambas condiciones pueden explicar el
  síntoma histórico, pero no se afirma que esa ejecución concreta las sufriera.
- **(c) Capacidad/cola/candidatos:** defaults 4096 presencias, 256 long-polls,
  cola de 16 eventos/30 s y ocho candidatos; se observaron N presencias y dos
  candidatos por par. No se observó pérdida de eventos; tampoco hay contador de
  expulsiones que permita descartarla categóricamente. El `announce` sustituye
  presencia y cola: su posible contribución al episodio histórico sigue siendo
  **INFERENCIA**, no causa demostrada.
- **(d) Relay:** N=6 bloqueado: 30 asignaciones, cero rechazos, siete DATA `no_peer`.
  N=8 bloqueado: 39 asignaciones vigentes (32 como máximo en NAT1 más hasta siete
  de NAT2), **44 rechazos código 4 (Full)** y 43 DATA `no_peer`.
  No son 28 asignaciones: cada par requiere dos. Con K agentes tras una IP y
  N agentes en la malla hacen falta K·(N−1) asignaciones/IP; N·(N−1) en total.
  N=6 home requiere 25 en NAT1; N=8 home requiere 49: 32 no puede alojarlas.
  Todos tras una IP: 30 para N=6, 42 para N=7, 56 para N=8.
  `ATENEA_RELAY_MAX_PER_IP`/TOML ya era configurable y validado; se añade prueba
  explícita de 64 válido y 0/superior al total inválidos, sin cambiar default32.
  TLS mantiene su límite independiente de ocho conexiones/IP.
- **(e) Arnés:** el fallo N=8 aparece sin bajas/revocaciones ni saltos de reloj.
  Eso descarta que esos eventos sean necesarios para el caso nuevo, pero **no**
  prueba la ausencia de un artefacto específico del simulacro histórico.

**Propuesta, no aplicada:** presupuestar y repartir con equidad solicitudes por
dispositivo/par antes de subir cuotas del coordinator; decidir el contrato y
sus consecuencias antiabuso por separado. Subir MAX_PER_IP dimensiona capacidad,
no elimina los límites ALLOC/COOKIE, TCP, por red/global o del coordinator.

**Repetición final, una vez N=6 y N=8 con/sin P2P**, binarios recompilados
offline y `MAX_PER_IP=64`: salida exterior **0**, 244,56 s de pared.
Ahora el laboratorio exige además tráfico positivo recibido/enviado en **ambos**
extremos de cada par y falla si al terminar queda algún camino `probing`/`none`.
Los cuatro casos cumplieron: **cero pares pendientes**, todos con tráfico.

| N | P2P | Pares | Primer / último establecimiento con tráfico (s) | Fin (s) |
| --- | --- | --- | --- | --- |
| 6 | permitido | 15/15 directos | 21,335 / 46,682 | 46,745 |
| 6 | bloqueado | 15/15 relay UDP | 25,340 / 61,872 | 61,926 |
| 8 | permitido | 28/28 directos | 21,367 / 56,986 | 57,035 |
| 8 | bloqueado | 28/28 relay UDP | 25,433 / 72,264 | 72,304 |

Extracto literal de la salida final:

```text
RESULT {"blocked": false, "complete": true, "elapsed": 46.745, "established": 15, "max_per_ip": 64, "n": 6, "pairs": 15, "pending": []}
RESULT {"blocked": true, "complete": true, "elapsed": 61.926, "established": 15, "max_per_ip": 64, "n": 6, "pairs": 15, "pending": []}
RESULT {"blocked": false, "complete": true, "elapsed": 57.035, "established": 28, "max_per_ip": 64, "n": 8, "pairs": 28, "pending": []}
RESULT {"blocked": true, "complete": true, "elapsed": 72.304, "established": 28, "max_per_ip": 64, "n": 8, "pairs": 28, "pending": []}
```

En N=8 bloqueado hubo 56 asignaciones UDP vigentes, cero rechazos del relay,
11 DATA `no_peer` (al abrir antes que la pareja), 28 peticiones relay HTTP200,
cero rechazos de tokens `rate_limited`/`peer_unavailable`; rendezvous 150 HTTP200
y 218 HTTP429. Los límites de solicitudes siguen afectando el establecimiento;
no se promete convergencia inmediata ni que una sola pasada mida rendimiento.
Al haber cambiado código **y** capacidad operativa, la mejora N=8 de 17/28 a
28/28 no aísla cuánto corresponde a cada factor: los tests unitarios sí aíslan
los dos defectos del motor.

**Comprobaciones macOS** (`CARGO_BUILD_JOBS=2`, Cargo `--locked --offline`):
`cargo fmt --all -- --check`, clippy de `atenea-agent`/`atenea-relay` con
`--all-targets -- -D warnings`, y tests de ambos paquetes: **90** del agente,
**7+1+7** del relay, todos correctos. Los dos tests nuevos del motor se vieron
fallar antes de su corrección; una primera versión del assert de caducidad
exigía que no hubiera *otro* intento posterior y se corrigió para comprobar
la identidad del intento original, que es el límite documentado.
No se repitieron los simulacros de caídas completos ni clippy/tests Linux
del workspace (los ejecuta el responsable).

**Descargas de esta investigación: 0** imágenes/capas/crates/paquetes/toolchains;
ambas compilaciones Docker y todos los laboratorios usan `--network none`,
imágenes con `--pull=never` y Cargo offline. Las comprobaciones macOS fueron
offline. Se conservaron imágenes y volúmenes protegidos; solo se actualizaron
los binarios/cachés de compilación locales ya autorizados. **Recursos Docker
propios dejados: 0** contenedores/redes/volúmenes/imágenes. Inventario final:
solo `atenea-control-plane-tests-pg17` preexistente, redes bridge/host/none,
filtros `atenea-multi-`/`atenea-drill-` de volúmenes vacíos. Salidas completas
locales: `/tmp/atenea-multi-before.log` y `/tmp/atenea-multi-after.log`.
No se hizo commit/push ni se publicaron artefactos.



## Regresiones, descargas y recursos dejados

Después de ambos perfiles se ejecutaron **una vez cada uno**, secuencialmente, `tests/stack/run.sh` y `tests/stack/upgrade.sh`: salida **0** en ambos; **118,62 s de pared en conjunto**. El stack de ocho servicios alcanzó salud en 29 s, mantuvo aislamiento loopback/PROXY v2 y transfirió 1 MB por directo, relay UDP y TLS 443; el upgrade/rollback duró 44 s incluidas ambas compilaciones offline. Se validó además `docker compose config --quiet` del base y del base+backup+observability combinados con referencias locales por digest y rutas sintéticas (solo resolución, **no arranque** de esos overlays). En esta primera entrega no se ejecutó infraestructura; la ampliación posterior de reinicios del motor se documenta abajo.

Salida funcional real de esas regresiones:

```text
PASS docker compose config
PASS stack saludable en 29 s (incluye migraciones)
PASS SNI relay + ALPN atenea-relay/1 termina TLS en relay vía HAProxy
PASS SNI desconocido no negocia relay
PASS PROXYv2 forjado externo rechazado antes de TLS
PASS backend PROXY relay 8443 solo loopback
PASS OIDC Dex por código sobre HTTPS, cookie Secure y sesión real
PASS CSRF rechazado sin token; red, certificado y dos agentes CLI enrolados/aprobados con huellas
PASS direct: TCP 1000000 bytes con SHA-256 coincidente
PASS relay-udp: TCP 1000000 bytes con SHA-256 coincidente
PASS relay-tls: TCP 1000000 bytes con SHA-256 coincidente por HAProxy TLS443/SNI verificado
PASS revocación efectiva; recorrido API/agentes en 33.8 s
STACK E2E: TODO OK (API OIDC; navegador cubierto por tests/e2e-panel)
N commit=bb6d63d538e64021d78d64a77012a4ebe4b53e18; N+1 working-tree HEAD=3b26ab97070bd4d411752fddaa96772eba8604af
N digest=sha256:0e23fd96767b0939d0dc37a869477d52479fbdc72a80424f18bc1b53751d50f1
N+1 digest=sha256:af61af55294fcc469d326bf1d41ef598c7a4522376d651e50a57c12e6e77e312
PASS HTTP initial: sesión, red, dispositivo, política y escritura persistente
PASS N schema=1,2; datos=eac86ac3680718e14248bd812c581f48
PASS HTTP next: sesión, red, dispositivo, política y escritura persistente + DNS 0003
PASS HTTP compatibility: sesión, red, dispositivo, política y escritura persistente
PASS N sobre schema N+1 (sin down migrations)
PASS HTTP before-rollback: sesión, red, dispositivo, política y escritura persistente
PASS HTTP rollback: sesión, red, dispositivo, política y escritura persistente
PASS rollback digest=sha256:0e23fd96767b0939d0dc37a869477d52479fbdc72a80424f18bc1b53751d50f1; datos=eac86ac3680718e14248bd812c581f48; 5 escrituras y DNS N+1 conservados
PASS upgrade/rollback PostgreSQL real; contenedores y worktree efímeros
PASS upgrade completo en 44 s (incluye ambas compilaciones offline)
PASS docker compose config base y base+backup+observability (netns)
```

**Descargas observadas: 0 imágenes/capas, 0 crates, 0 paquetes/toolchains.** El arnés de caídas no compila ni hace pull; el bootstrap reutilizado solo invoca imágenes ya precomprobadas. El upgrade compila con `CARGO_BUILD_JOBS=2`, `--locked --offline`, `--network none`; BuildKit resolvió la base distroless local (primer paso de metadatos 1,1 s, siguiente 0,0 s) sin transferencia de capas. Sus mensajes `transferring context: 15.08MB/15.75MB` son copia **local** del binario al builder, no descargas WAN. No se capturaron paquetes ni un contador de bytes de red: no se afirma cero bytes de consultas de metadatos a registros. No hubo ninguna transferencia observada >30 MB ni se instaló nada.

**Recursos propios dejados en Docker: 0 contenedores, 0 redes, 0 volúmenes y 0 imágenes temporales de upgrade.** El inventario final mostró únicamente `atenea-control-plane-tests-pg17` (`e5aa569f23df`, preexistente), redes `bridge`, `host`, `none`; filtro de volúmenes `atenea-drill-` vacío y filtro `atenea-stack-` únicamente la caché protegida `atenea-stack-modules`. Se conservaron imágenes locales, todas las cachés y el runtime preexistente de otro trabajo. Los intentos previos fallidos del arnés también limpiaron sus propios proyectos; no se hizo prune, commit, push, publicación ni despliegue.

En esta primera entrega no se cambió Rust. La ampliación posterior refuerza el utilitario Rust de healthcheck y documenta sus comprobaciones abajo; no cambia código del agente/coordinator. El posible fallo multipar queda abierto como se detalla arriba; red/proveedor real, relojes desincronizados y cargas altas siguen **SIN COMPROBAR**.

## Ampliación: reinicio del motor Docker

El integrador detectó una segunda convergencia Ansible fallida: los ocho servicios habían sido healthy y, después del handler diferido de Docker, HAProxy/relay quedaron unhealthy. No se descartó por una pasada verde: el problema depende del orden de restauración del motor y se reprodujo con reinicios explícitos.

### Causa y reproducción

Tres `systemctl restart docker` iniciales con `live-restore=true` conservaron procesos e inodos: no ejercitaban un arranque frío. En el nodo **aislado** de diagnóstico se fijó temporalmente `live-restore=false` y se hicieron tres restauraciones frías reales, restableciendo el punto inicial por Compose entre ellas. Resultado: **1/3** dejó HAProxy y relay `exited/unhealthy`, otros seis healthy; **2/3** arrancaron correctamente. El primer fallo se observó a los **26,471 s** desde iniciar el reinicio. El motor era Docker **26.1.5+dfsg1**, `containerd-snapshotter=false`, `storage-driver=vfs`; el journal informa que overlay2 no pudo montarse, así que no se atribuye la carrera a overlay2.

Evidencia real del journal, no una hipótesis sobre los healthchecks:

```text
2026-10-25T14:07:57.026395565Z level=error msg="failed to start container" container=e0283ef267322f149d7effce85a380fb2351ce4d8d649bc1c28288594faac0a7 error="cannot join network of a non running container: 044146060af30f1ecb6793a0746cc6f1ef3c7672df7aba652ff130e7eec2b09a"
2026-10-25T14:07:57.026687441Z level=error msg="failed to start container" container=3023106dd2d0e8e138c14d2be7abd0133b08c2ffe4855268ecf4028168861a3d error="cannot join network of a non running container: 044146060af30f1ecb6793a0746cc6f1ef3c7672df7aba652ff130e7eec2b09a"
```

Los IDs son respectivamente relay, HAProxy y netns del ensayo. `depends_on` ordena Compose, **no** el restaurador del daemon. La política `unless-stopped` por sí sola no vuelve a intentar correctamente esa dependencia cuando netns aún no está listo.

### Corrección y medida posterior

Ansible instala `atenea-stack.service`, ligado a Docker por `Requires/After/PartOf` y habilitado en `docker.service.wants`. Detiene los tres compartidores antes del motor; después espera Caddy y sus dependencias y recrea netns/HAProxy/relay juntos, con espera de salud. No elimina volúmenes ni ejecuta migraciones al reiniciar. El rol vacía los handlers del motor antes de desplegar/comprobar salud. `live-restore=true` sigue siendo el valor productivo, pero la recuperación no depende de él.

HAProxy ahora comprueba namespace del proceso principal, configuración y listener de `netns:443`; relay comprueba namespace, socket Unix y conexión a ese listener. El utilitario `deploy/images/healthcheck.rs` añadió TCP y destino adicional sin dependencias. Se ejecutaron **rustfmt, clippy-driver `-D warnings` y sus 2 pruebas en macOS**, con `CARGO_BUILD_JOBS=2`; también sus **2 pruebas Linux** y compilación con `--network none` y toolchain local. Es un binario independiente compilado por `rustc`, no un paquete Cargo del workspace. No se modificó ningún paquete del agente/coordinator.

Para no alterar imágenes protegidas se creó una imagen temporal nueva del relay cambiando solo el healthcheck, ID Docker Desktop `sha256:27fc18fe12a4627c8af6c2d93d85066663fd596e83ad581baf64497e860bab85`; el almacén clásico la identifica como `sha256:51dc884a0627acf6dca3c88a7f7d97d31c585a102e27001afb5bb57fed3d6386`. Se usa mediante `INFRA_RELAY_IMAGE` en el ensayo; `atenea/relay:local`, la imagen de herramientas y los alias pinned no se modificaron.

Con la unidad instalada se repitieron **tres reinicios fríos consecutivos** (`live-restore=false` solo diagnóstico), sin reparación manual de contenedores:

| Repetición | Inicio Unix | Hasta ocho healthy (s) | Namespace netns = HAProxy = relay | Probes HAProxy / relay |
| --- | --- | --- | --- | --- |
| 1 | 1792937715,6875594 | 36,753 | `net:[4026533774]` | 0 / 0 |
| 2 | 1792937752,4647598 | 36,817 | `net:[4026534030]` | 0 / 0 |
| 3 | 1792937789,3006108 | 37,038 | `net:[4026534286]` | 0 / 0 |

Media **36,869 s**, peor **37,038 s**. El script espera el job systemd ya iniciado por Docker; no ejecuta su propia recreación en la etapa `after`. Los ocho servicios y los inodos se comprueban mediante inspect y probes reales, no solo por etiquetas de Compose. No se ensayó reboot de máquina física ni actualización real de paquetes Docker.

### Dos regresiones completas consecutivas

Se ejecutó dos veces `INFRA_RELAY_IMAGE=atenea-stack-relay-health:local INFRA_IMAGE=atenea-infra:local bash tests/infra/run.sh`, sin trabajos Docker simultáneos:

| Pasada | Convergencia inicial (s) | Tiempo runner / pared exterior (s) | Segunda convergencia principal | Tres reinicios Docker hasta ocho healthy (s) |
| --- | --- | --- | --- | --- |
| 1 | 57 | 487 / 488,57 | `ok=33 changed=0 failed=0` | 26,205 / 26,222 / 26,149 |
| 2 | 55 | 476 / 476,16 | `ok=33 changed=0 failed=0` | 25,922 / 25,996 / 25,936 |

Ambas salidas fueron **0**. Los seis reinicios usaron el valor productivo `live-restore=true`: las aplicaciones que conserva el daemon permanecieron estables, pero la unidad sí reconstruyó la entrada. Cada repetición exigió los ocho healthy, igualdad de inode netns/HAProxy/relay y probes 0. El ensayo también completó SOPS/rotación, syntax/lint offline, migración, HTTPS con CA, puertos/nftables/secretos/SSH, upgrade feliz, fallo forzado con rollback exacto, nodo secundario relay+STUN y su segunda convergencia `changed=0`, y plan OpenTofu local.

Además se provocó un **namespace obsoleto real** reiniciando **solo netns**, sin curarlo automáticamente: los dos consumidores conservaron el inode anterior y se volvieron unhealthy; ambos probes devolvieron **1**. En la primera pasada, netns fue `net:[4026533372]` y los consumidores `net:[4026533244]`; HAProxy rechazó DNS de `caddy` y relay de `netns`. No se consideró sano solo por existir `/tmp/relay.sock` o ser válida la configuración. Las observaciones tardaron **170,036 / 168,235 s**, incluyendo 30 reintentos de salud y probes finales; **no** es una sonda instantánea ni la duración de recuperación. Reiniciar la unidad reconstruyó el namespace y recuperó los ocho. El caso quedó permanente en `tests/infra/restart-docker.py`.

Salida funcional literal, repetida en ambas pasadas:

```text
PASS namespace obsoleto: probes fallan; unidad reconstruye entrada
PASS Docker restart after: 3 repeticiones reales
PASS panel HTTPS tras HAProxy/Caddy con CA interna
PASS upgrade feliz y upgrade fallido con rollback al mapa anterior
PASS relay: 2 healthy; docker ps/inspect y nftables puertos exactos; secretos 0400/UID; SSH; idempotencia si se solicita
INFRA OK: stack real, migraciones, HTTPS, digests, upgrade/rollback, relay+STUN, secretos e idempotencia.
```

Logs locales sin incorporar al repositorio: `/tmp/atenea-drill-docker-cold-{before,after}.log`, `/tmp/atenea-drill-infra-final{1,2}.log`. Los nodos `atenea-infra-35190` (diagnóstico), `atenea-infra-55281` y `atenea-infra-58474` y sus volúmenes anónimos fueron eliminados; el runner solo leyó/exportó las imágenes preexistentes del host y cargó sus copias en el Docker aislado. No cambió tags pinned ni la imagen de herramientas.

### Regresión final del Compose y limpieza

Por haber cambiado las sondas del Compose base se ejecutó **una vez** `tests/stack/run.sh`, seleccionando la imagen temporal del relay con `RELAY_IMAGE=sha256:27fc18fe12a4627c8af6c2d93d85066663fd596e83ad581baf64497e860bab85`. Salida **0**, **75,83 s** de pared; ocho servicios saludables en **29 s**, migración incluida. Pasaron OIDC/CSRF/enrollment/revocación, aislamiento loopback/PROXY y transferencias TCP de 1 MB con SHA-256 por directo, UDP relay y TLS 443:

```text
PASS stack saludable en 29 s (incluye migraciones)
PASS SNI relay + ALPN atenea-relay/1 termina TLS en relay vía HAProxy
PASS SNI desconocido no negocia relay
PASS PROXYv2 forjado externo rechazado antes de TLS
PASS backend PROXY relay 8443 solo loopback
PASS OIDC Dex por código sobre HTTPS, cookie Secure y sesión real
PASS CSRF rechazado sin token; red, certificado y dos agentes CLI enrolados/aprobados con huellas
PASS direct: TCP 1000000 bytes con SHA-256 coincidente
PASS relay-udp: TCP 1000000 bytes con SHA-256 coincidente
PASS relay-tls: TCP 1000000 bytes con SHA-256 coincidente por HAProxy TLS443/SNI verificado
PASS revocación efectiva; recorrido API/agentes en 33.9 s
STACK E2E: TODO OK (API OIDC; navegador cubierto por tests/e2e-panel)
```

Log: `/tmp/atenea-drill-stack-final.log`. La imagen temporal se eliminó al acabar, igual que binarios/contexto temporal y la configuración fría de diagnóstico. **Inventario final**: solo el contenedor preexistente `atenea-control-plane-tests-pg17` (`e5aa569f23df`), redes `bridge/host/none`, ningún volumen `atenea-drill-` y únicamente la caché protegida `atenea-stack-modules` bajo `atenea-stack-`. **0 recursos propios de ejecución restantes; 0 descargas observadas**. Para repetir el ensayo con estos cambios hay que construir la imagen relay de las fuentes actuales o proporcionar otra local mediante `INFRA_RELAY_IMAGE`; los tags protegidos conservan sus binarios anteriores.

