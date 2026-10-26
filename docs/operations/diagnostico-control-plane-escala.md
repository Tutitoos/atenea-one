# Diagnóstico de escala del control plane

Fecha: 2026-10-26. Investigación de solo lectura del producto congelado para auditoría (`23e3d13`); **no se implementa ninguna corrección**. Las únicas escrituras fueron este informe y objetos sintéticos en una base privada, ya eliminada. No se recompiló, instaló ni descargó nada. Las mediciones SQL siguientes no son una repetición del ensayo HTTP histórico ni una certificación de capacidad.

## 1. Resultado y rectificación del escenario

**DEMOSTRADO:** la serie histórica `atenea-load-85467` no tenía 10 000 dispositivos en una red. Tenía **10 000 en 79 redes: 78 × 128 + 1 × 16**. Lo acreditan `tests/load/out/atenea-load-85467/control/10000-seed.jsonl:1-2`, el valor predeterminado 128 de `tests/load/control.sh:35,50` y el intercalado de redes de `tests/load/control.py:42-47`. La misma aclaración afecta a las descripciones de matriz/soporte que digan «10 000 en una red».

**DEMOSTRADO por código y experimento SQL:**

1. `ip_allocations` no tiene un índice cuyo primer campo sea `device_id`. Los dos índices únicos comienzan por `network_id` (`apps/control-plane/migrations/0001_control_plane.sql:8`), pero las consultas calientes filtran solo por dispositivo. PostgreSQL 17 ejecutó barridos de las **20 000 asignaciones** para devolver dos direcciones, y otros dos barridos por dispositivo en el inventario. El índice candidato elimina esos barridos y reduce el listado SQL de una red histórica de 128 dispositivos de **107,773–127,736 ms a 0,897–1,216 ms**, en este experimento secuencial.
2. Cada emisión ejecuta **2N consultas secuenciales adicionales**, N = dispositivos aprobados de la red, dentro de un bloqueo exclusivo de esa red (`issue.rs:23-34,184-209`). En una red histórica llena N=128: 256 viajes SQL adicionales. No son 20 000 viajes por emisión histórica; eso correspondería al escenario SQL no admitido de 10 000/red.
3. El bloqueo de red se conserva mientras se cargan pares, compilan políticas, codifica, firma, calcula SHA-256 y escriben versión/auditoría. Dos emisiones de esa misma red no pueden hacer esa sección en paralelo. Una segunda sesión de diagnóstico agotó `lock_timeout=100ms` al intentar bloquear la fila que la primera mantenía bloqueada (§7).
4. Las lecturas de inventario **no bloquean la red**. Sí renuevan una sesión con `UPDATE sessions`, compartida por los 200 workers históricos, en una transacción breve (`auth.rs:323-355`). El costoso inventario ocurre después de ese commit. No se debe atribuir sus 15 s de p95 al `FOR UPDATE` de emisión, ni a mantener la sesión bloqueada durante el listado.

**INFERIDO:** barridos globales repetidos + N+1 SQL + pool de 20 conexiones + espera de red explican una degradación compatible con la histórica. El A/B demuestra el coste evitable SQL, **no el porcentaje exacto de los 19,5 s históricos**. Faltan trazas históricas de esperas, CPU agregada de PostgreSQL y calibración del generador. No se demuestra una causa única de aquel p95 ni saturación de CPU del servidor.

## 2. Qué midió exactamente cada perfil histórico

Cada escala se sembró una vez y se reutilizó para ambas repeticiones. Fuentes de población: `control/1000-seed.jsonl:1-2`, `5000-seed.jsonl:1-2`, `10000-seed.jsonl:1-2`, bajo `tests/load/out/atenea-load-85467/`. Había dos IP por dispositivo, una política `any↔any` por red, 10 080 muestras y 10 080 eventos sintéticos por red (`control.py:239-257`).

| Dispositivos totales | Distribución exacta | Redes | Muestras / eventos sintéticos de cada clase |
|---:|---|---:|---:|
| 1 000 | 7 × 128 + 1 × 104 | 8 | 80 640 |
| 5 000 | 39 × 128 + 1 × 8 | 40 | 403 200 |
| 10 000 | 78 × 128 + 1 × 16 | 79 | 796 320 |

Tabla de **los 24 perfiles HTTP**, extraída de `control-summary.jsonl` del mismo directorio; «línea» identifica la fila exacta del artefacto local. Estos artefactos están ignorados/no publicados: se transcriben solo datos sanitizados, no se exige que existan en un clon público. RPS incluye drenaje; los números son por repetición, no promedios de percentiles.

| Total / distribución anterior | Clientes máximos | Perfil | Repetición | Solicitudes / errores | RPS útil | p95 ms | Línea summary |
|---:|---:|---|---:|---:|---:|---:|---:|
| 1 000 / 8 redes | 50 | lectura | 1 | 1560 / 0 | 151,993 | 507,396 | 7 |
| 1 000 / 8 redes | 50 | lectura | 2 | 1908 / 0 | 185,476 | 399,005 | 18 |
| 1 000 / 8 redes | 50 | emisión | 1 | 867 / 0 | 82,268 | 723,112 | 3 |
| 1 000 / 8 redes | 50 | emisión | 2 | 864 / 0 | 81,443 | 797,345 | 14 |
| 1 000 / 8 redes | 200 | lectura | 1 | 2101 / 0 | 189,990 | 1195,827 | 5 |
| 1 000 / 8 redes | 200 | lectura | 2 | 2086 / 0 | 189,478 | 1201,976 | 16 |
| 1 000 / 8 redes | 200 | emisión | 1 | 1013 / 0 | 80,906 | 2707,569 | 1 |
| 1 000 / 8 redes | 200 | emisión | 2 | 958 / 0 | 76,383 | 2788,137 | 12 |
| 5 000 / 40 redes | 50 | lectura | 1 | 315 / 0 | 26,523 | 4183,972 | 53 |
| 5 000 / 40 redes | 50 | lectura | 2 | 430 / 0 | 36,237 | 3005,132 | 64 |
| 5 000 / 40 redes | 50 | emisión | 1 | 243 / 0 | 19,739 | 2888,559 | 49 |
| 5 000 / 40 redes | 50 | emisión | 2 | 254 / 0 | 21,541 | 2698,849 | 60 |
| 5 000 / 40 redes | 200 | lectura | 1 | 567 / 0 | 36,096 | 6801,825 | 51 |
| 5 000 / 40 redes | 200 | lectura | 2 | 482 / 0 | 29,300 | 9492,279 | 62 |
| 5 000 / 40 redes | 200 | emisión | 1 | 404 / 0 | 21,367 | 9590,810 | 47 |
| 5 000 / 40 redes | 200 | emisión | 2 | 405 / 0 | 21,459 | 9596,910 | 58 |
| 10 000 / 79 redes | 50 | lectura | 1 | 180 / 0 | 12,198 | 7710,127 | 30 |
| 10 000 / 79 redes | 50 | lectura | 2 | 223 / 0 | 16,595 | 6386,344 | 41 |
| 10 000 / 79 redes | 50 | emisión | 1 | 151 / 0 | 10,509 | 5420,677 | 26 |
| 10 000 / 79 redes | 50 | emisión | 2 | 151 / 0 | 11,291 | 5098,179 | 37 |
| 10 000 / 79 redes | 200 | lectura | 1 | 370 / 0 | 16,258 | 14989,027 | 28 |
| 10 000 / 79 redes | 200 | lectura | 2 | 358 / 0 | 15,863 | 15201,226 | 39 |
| 10 000 / 79 redes | 200 | emisión | 1 | 296 / 0 | 10,584 | 19509,734 | 24 |
| 10 000 / 79 redes | 200 | emisión | 2 | 304 / 0 | 11,137 | 18494,476 | 35 |

«Lectura» rota entre inventario completo **de una red**, métricas `limit=1000` y auditoría `limit=100`, no entre tres listados de 10 000 dispositivos (`control.py:199-215`). Los 64 WebSockets por escala/repetición son otra fase, no tráfico superpuesto a estos 24 perfiles (`control.sh:53-65`). La población sembrada no significa que los 10 000 dispositivos se hayan renovado en los 10 s nominales: a 10k/200 solo hubo 296 y 304 emisiones útiles.

## 3. Todo el trabajo de una emisión aceptada

Ruta `GET /api/v1/device/config` → `issue::get` (`api.rs:107`). N es la población aprobada de **esa red**; P, políticas; V, historial del dispositivo; A, auditoría de la red. Los tiempos son mediciones individuales de §5, no un desglose temporal HTTP.

| Orden | Trabajo / coste / bloqueo | Evidencia fuente |
|---|---|---|
| 1 | Extraer bytes/cabecera AtDevice, parsear identificadores/firma/nonce, comprobar timestamp ±60 s. Sin transacción de aplicación. | `issue.rs:143-160` |
| 2 | Adquirir conexión para `SELECT network_id,identity_pubkey,status FROM devices WHERE id=$1`; PK, una fila; devolver conexión al pool. | `issue.rs:161-169` |
| 3 | Calcular mensaje autenticado, BLAKE2s del cuerpo y verificar Ed25519 estricta. Consumir token de tasa y reservar nonce bajo mutexes en memoria. Comprobar aprobado. No firma una raíz ni llama al IdP. | `issue.rs:170-183`; `crates/identity/src/api_auth.rs:102-115`; `lib.rs:63-110` |
| 4 | Segunda adquisición y `BEGIN`; **`SELECT … FROM networks WHERE id=$1 FOR UPDATE`**. Bloqueo exclusivo de fila hasta COMMIT/rollback; la espera ya ocupa una de las 20 conexiones. | `issue.rs:184-185` |
| 5 | Releer status del dispositivo por PK. **No hay `FOR UPDATE` del dispositivo aquí**: el orden se protege con el bloqueo de red compartido por las mutaciones. Parsear certificado de 178 bytes, validar fechas y duración ≤3600 s. | `issue.rs:186-208` |
| 6 | Listar N aprobados, ordenar por UUID, recuperar claves/endpoints; **por cada fila** consultar IP y rutas aprobadas, secuencialmente; parsear direcciones/prefijos, comprobar tamaños de claves, sanear hasta cuatro candidatos. IP sin índice adecuado: O(total de asignaciones) × N. Rutas dispone de `UNIQUE(device_id,prefix)`; una tabla vacía puede elegir Seq Scan legítimamente. | `issue.rs:23-66,209-213`; migraciones `0001:8`, `0003:11-14` |
| 7 | Leer políticas ordenadas, deserializar selectores; construir prefijos privados; compilar pares y reglas inversas deduplicadas. Recorre políticas/dispositivos y busca duplicados linealmente en peers: no es solo coste SQL; en malla densa puede aproximarse a O(P·N²). Rechaza overflow al final. | `policies.rs:59-75`; `issue.rs:79-141,214-232` |
| 8 | Leer DNS de red, filtrar servidores según prefijos y rutas autorizadas. | `dns.rs:19-29`; `issue.rs:233-238` |
| 9 | `SELECT max(version) … WHERE device_id=$1`; índice único `(device_id,version)` disponible. Calcular `max(now_ms,last+1)`, construir configuración y canonicalizar. | `issue.rs:239-269`; `0001:11` |
| 10 | Codificar configuración (puede fallar validación estructural), adjuntar certificado, firmar Ed25519 con emisor; SHA-256 del fichero completo. Todo **dentro del bloqueo de red**. | `issue.rs:270-272`; `crates/identity/src/lib.rs:282-300` |
| 11 | INSERT `configuration_versions`: FK de dispositivo, secuencia `bigserial`/`nextval`, PK y unique `(device_id,version)`, escritura WAL. | `issue.rs:273`; `0001:11` |
| 12 | UPDATE por PK de `devices.last_config_at`; adquiere bloqueo de fila del dispositivo hasta commit. Este es el bloqueo de dispositivo efectivo de la emisión; no confundir con el `FOR UPDATE` explícito de approve/status. | `issue.rs:274-277`; `devices.rs:164-179`; `panel.rs:180-183` |
| 13 | `EXISTS` de auditoría de ese dispositivo/red/acción en la última hora. El índice de página `(network_id,id DESC)` no cubre target/action/at: recorre potencialmente A eventos de red. Si falta evento: INSERT de auditoría con JSON de versión/hash hexadecimal, nueva secuencia `bigserial`, PK y `audit_network_page`. Una entrada/dispositivo/hora; el historial guarda todas las emisiones. | `issue.rs:278-293`; `audit.rs:14-25`; `0001:12-13` |
| 14 | COMMIT: persistencia/WAL según configuración de PG; liberar ambos bloqueos y conexión. Incrementar **un** contador LeaseIssued o LeaseRenewed; publicar dos notificaciones `devices`/`audit` mediante mutex, secuencia atómica y broadcast (incluso si no se insertó auditoría). Responder fichero con `no-store` y versión. | `issue.rs:294-313`; `panel.rs:49-56` |

Una emisión exitosa hace **2N+10 sentencias de datos** contando la consulta de autenticación, **2N+11** si inserta auditoría, más BEGIN/COMMIT; las adquisiciones pueden incluir ping/protocolo de SQLx y conexiones nuevas. A N=128 son 266/267 sentencias de datos; a N=10 000, 20 010/20 011 (este último escenario no es producto admitido). No hay asignación IPAM ni incremento de versión global de red por emisión. Las devoluciones tempranas revierten la transacción; tokens/nonces ya consumidos en memoria no se revierten. Errores SQL incrementan DatabaseError (`api.rs:29-39`); el middleware registra HTTP (`api.rs:130-138`).

### Pool y esperas

`main.rs:53-56` fija **20 conexiones**, sin configurar `acquire_timeout`; SQLx **0.8.6** predetermina **30 s**. Evidencia del valor: fuente local ya instalada `sqlx-core-0.8.6/src/pool/options.rs:143-163` (`acquire_timeout: Duration::from_secs(30)`, `test_before_acquire: true`, fairness activa). No son 30 s de timeout de consulta ni de espera del bloqueo: no se configura aquí `lock_timeout`/`statement_timeout`. El mismo pool sirve lecturas, auth y el sampler/purgas de fondo (`panel.rs:231-267`).

En una única red, una emisión puede estar trabajando y hasta otras 19 conexiones esperando el mismo bloqueo; las restantes solicitudes esperan el pool, incluso para autenticarse. En 79 redes hay paralelismo entre redes, acotado a 20 conexiones y recursos PG. **SIN COMPROBAR:** ocupación/espera reales del pool en la serie histórica. Aumentar el pool no elimina N+1 ni el bloqueo por red y puede empeorar competencia en PostgreSQL.

## 4. Límite de producto, no solo constante

- **256 pares por configuración**, además de 256 reglas: contrato `docs/protocol/signed-config-v1.md:85-86`; v2 hereda lo no modificado (`signed-config-v2.md:4`). `issue.rs:138-139` implementa ese contrato mediante `409 policy_overflow`.
- Además, **256 dispositivos aprobados por red**, aunque las ACL sean dispersas: contrato `docs/api/control-plane-v1.md:36`; aplicado por `networks.rs:229-243` desde aprobación/IPAM. La aprobación del 257.º devuelve `409 network_full`. Es un límite distinto del número de peers; con 256 aprobados una malla completa tiene 255 pares por dispositivo. La tabla de soporte ya menciona 256 aprobados/red (`soporte-limites-y-recuperacion.md:40`); debe quedar vinculada expresamente al contrato y al comportamiento de error, no a una capacidad de 10 000/red.
- Al crear una política, se compila para todos los dispositivos **antes de commit**; overflow devuelve 409 al administrador y revierte esa política (`policies.rs:127-165`). No se ha ensayado aquí su representación visual en el panel.
- Una BD heredada o sembrada directamente puede exceder el cupo; no hay CHECK SQL de cardinalidad. Con 10 000 aprobados y `any↔any`, cada uno selecciona 9999 pares y `issue::get` falla en compile antes de firmar, registrar versión o actualizar `last_config_at`. El solicitante recibe `409` JSON `policy_overflow`, no un fichero parcial (`api.rs:25-47`; `issue.rs:232,270-294`). Sin políticas no hay peers, pero seguiría cargando todos los dispositivos. **No es una forma admitida de saltarse el cupo de 256 aprobados.**
- El agente trata ese 409 como «error HTTP del control plane», conserva el fichero anterior, registra sync_errors/last_error y no renueva su lease (`apps/agent/src/sync.rs:26-42,71-89,115-127`). Un dispositivo nuevo no obtiene configuración; uno ya configurado conserva únicamente su lease previo y acabará fail-closed. El error específico no se convierte automáticamente en una alerta de administrador acreditada por este ensayo.

Subir estos límites no forma parte de la corrección de rendimiento: tocaría contrato firmado, parsers, memoria/validación de agentes, API de aprobación y pruebas de seguridad. Ningún resultado de este informe autoriza ese cambio.

## 5. Medición SQL real y comparación del índice

### Entorno y datos

Se usó exclusivamente el PostgreSQL existente `atenea-control-plane-tests-pg17` mediante `docker exec … psql -X -U atenea_test`, por socket local dentro del contenedor: sin abrir red nueva, sin otro contenedor/imagen y sin descargar. `docker inspect` confirmó contenedor ya iniciado, imagen `postgres:17`, red preexistente bridge; no se alteró su configuración. El URL de pruebas anunciado apunta al mismo servicio; no se necesitó otro cliente instalado.

Base creada: **`diag_control_plane_20261026`**. Se aplicó literalmente SQL de las migraciones **0001–0004** en orden; no el migrador Rust (por ello sin tabla `_sqlx_migrations`). PostgreSQL informado: **17.11, aarch64**, `shared_buffers=128MB`, `max_connections=100`, `jit=on`. Medidas de pared/locales sobre Docker Desktop, no producción.

Forma A: una organización, un usuario, una red privada, 10 000 dispositivos approved, 20 000 asignaciones (10k IPv4 + 10k IPv6), 10 000 reports, una política any, 10 080 samples, 10 080 audit_events sintéticos. UUID de dispositivo = `md5(i::text)::uuid`, IPv4 = `'10.90.0.0'::inet+i`, IPv6 = `'fd90::'::inet+i`, i=1..10000. Claves/rellenos de bundle/certificado **solo satisfacen longitudes SQL, no son identidades ni certificados criptográficamente válidos**. Esto permite medir SQL sin usar claves reales ni publicar secretos. `device_routes` e historial de versiones estaban vacíos; las escrituras ensayadas se revirtieron. Esta forma A deliberadamente excede el contrato de admisión.

Forma B posterior: los mismos 10k dispositivos repartidos en **79 redes (78×128+16)** y 796 320 filas de cada clase samples/audit, manteniendo 20k asignaciones y 10k reports. Se redistribuyeron mediante UPDATE; para la comparación estable B se hizo `VACUUM ANALYZE` únicamente en esta base, no en otra. No replica exactamente bloat, cachés, claves ni cronología de la BD histórica.

### Consultas individuales, forma A

`EXPLAIN (ANALYZE, BUFFERS)`, una ejecución inicial por consulta, planificación excluida del campo Execution Time. Búferes = bloques compartidos reportados, no bytes leídos físicamente del disco. Cost = unidades del planificador, no ms.

| Consulta fuente | Execution Time ms | Plan / filas / búferes relevantes |
|---|---:|---|
| Auth dispositivo, `issue.rs:161` | 0,037 | PK Index Scan, 1 fila, 3 hit |
| Red FOR UPDATE, `issue.rs:185` | 0,420 | LockRows sobre Seq Scan de tabla de 1 red, 1 fila, 2 hit; sin contención |
| Status, `issue.rs:186` | 0,024 | PK Index Scan, 1 fila, 3 hit |
| Cargar dispositivos, `issue.rs:24` | 4,884 | Seq Scan+Sort, 10k filas, 418 hit; cost 1229,39..1254,39 |
| IPs de un dispositivo, `issue.rs:29` | 1,501 | Seq Scan+Sort, 2 filas, elimina 19998, 200 hit; cost 447,01..447,02 |
| IPv4 escalar del listado, `devices.rs:132` | 1,191 | Seq Scan, 1 fila, elimina 19999, 197 hit; cost 0..497 |
| Rutas aprobadas, `issue.rs:34` | 0,025 | Seq Scan tabla vacía+Sort, 0 filas; **no demuestra falta de índice de rutas** |
| Políticas, `policies.rs:60` | 0,074 | Seq Scan+Sort, 1 fila, 4 hit |
| DNS, `dns.rs:20` | 0,026 | Seq Scan de 1 red, 1 fila, 1 hit |
| max(version), `issue.rs:240` | 0,024 | Aggregate, historial vacío; **no mide historial retenido** |
| Auditoría EXISTS, `issue.rs:280` | 1,036 | Seq Scan, elimina 10080, 144 hit; índice de página no resuelve este filtro |
| Inventario exacto, `devices.rs:132` | 6994,015 | Merge Left Join; dos subplanes Seq Scan con loops=10000; 3 940 509 hit; JIT 260,696 ms |
| Métricas, `panel.rs:228`, since concretado a 7 días | 1,011 | Index Scan Backward `network_samples_pkey`, LIMIT 1000+Sort, 36 hit |
| Página auditoría exacta, `audit.rs:39`, before=max bigint | 0,049 | Index Scan Backward PK, 100 filas, 4 hit; una sola red |

El inventario procesa **400 millones de filas candidatas** en los dos subplanes de asignaciones: 2×10000×20000. Los scans principales de tablas completas de una única red son razonables; lo evitable es repetir el scan global por dispositivo/familia. En B el EXISTS usó `audit_network_page` → Bitmap Heap Scan y filtró **10080** eventos de la red: 2,774 ms inicial/frío y 0,996 ms después/caliente. Esa diferencia no la causa el índice de IP.

Escrituras medidas separadamente en B, siempre `BEGIN; EXPLAIN (ANALYZE, BUFFERS) …; ROLLBACK;`: INSERT versión **0,739 ms** (incluye trigger FK 0,456 ms; secuencia y 69 hit), UPDATE dispositivo **0,203 ms** (PK, 26 hit), INSERT auditoría **0,306 ms** (secuencia/índices, 77 hit). El rollback no deshace avances de secuencias, confinados a la base privada después borrada. No mide latencia de COMMIT durable ni crecimiento de historial.

### A/B: índice creado solo en `diag_*`

```sql
CREATE INDEX ip_allocations_device_family
    ON ip_allocations(device_id, family) INCLUDE (addr);
ANALYZE ip_allocations;
```

Se retiró/recreó solo este índice en la base propia para cada comparación; ningún cambio a las migraciones del repositorio. Tres EXPLAIN consecutivos por consulta en una misma sesión psql, antes y después, sin carga concurrente; no son p95 estadísticos ni una comparación de almacenamiento frío controlado.

| Forma / consulta | Antes: tres Execution Time ms | Después: tres Execution Time ms |
|---|---|---|
| A, IPs de dispositivo (2 filas) | 0,985 / 0,442 / 0,391 | 0,022 / 0,005 / 0,007 |
| A, IPv4 escalar (1 fila) | 0,960 / 0,445 / 0,427 | 0,024 / 0,007 / 0,005 |
| A, inventario (10 000 filas) | 5568,623 / 5501,537 / 5528,065 | 19,538 / 18,073 / 17,247 |
| B, IPs de dispositivo (2 filas) | 0,690 / 0,472 / 0,366 | 0,016 / 0,008 / 0,006 |
| B, IPv4 escalar (1 fila) | 0,682 / 0,390 / 0,382 | 0,021 / 0,007 / 0,006 |
| B, inventario (128 filas) | 127,736 / 108,546 / 107,773 | 1,216 / 0,905 / 0,897 |

Planes A posteriores: `Index Only Scan using ip_allocations_device_family`, condición device_id (y family para el escalar), Heap Fetches=0. En el primer listado posterior: **40 384 hit +131 read**, frente a 3 940 509 hit antes; los subplanes siguen teniendo 10 000 loops cada uno, pero ya no barren 20 000 filas. Cost del listado pasó de ~9 942 463 a ~88 823; JIT dejó de aparecer en ese plan. El índice medía **1 097 728 bytes (1072 KiB)** para estas 20k filas. Visibility map y futuras escrituras pueden exigir Heap Fetches; INCLUDE no promete siempre index-only.

### Emisión aislada: qué se pudo cronometrar y qué no

Se ejecutó en psql con `\timing on` y `\o /dev/null` un **esqueleto SQL secuencial**: BEGIN, red FOR UPDATE, status, lista de aprobados, 2 consultas por dispositivo, políticas, DNS, max(version), INSERT versión, UPDATE dispositivo, EXISTS, INSERT auditoría y ROLLBACK. Se consumieron resultados sin imprimirlos; sin RTT TCP al servidor, sin ejecutar Rust, canonicalización, firma, HTTP, autenticación ni COMMIT durable. No es una «emisión aislada completa», ni debe restarse de las latencias HTTP históricas.

| Forma | Sentencias incluyendo BEGIN/ROLLBACK | Suma `\timing` antes / después ms | Pared del proceso docker+psql antes / después ms |
|---|---:|---:|---:|
| A: 10k en una red, solo SQL no admitido | 20012 | 4457,405 / 1096,946 | 4704,319 / 1354,001 |
| B: red de 128 con 10k totales | 268 | 62,699 / 20,116 | 124,406 / 74,503 |

La suma de `\timing` contiene parseo/protocolo local, no solo tiempo de ejecución del servidor. El tiempo de pared añade lanzamiento y espacios entre sentencias. El esqueleto A llega a escrituras artificialmente: el handler real con la política any fallaría antes por overflow, y los certificados sintéticos no superarían su parseo. **SIN COMPROBAR:** T exacto de bloqueo de una emisión HTTP válida aislada antes/después; no se inventa esa cifra a partir del esqueleto SQL. Para medirla hace falta fixture criptográfica válida en tamaño permitido y temporización del handler/binario identificado.

## 6. Techo de emisiones y renovación a 60 s

Si una emisión mantiene el bloqueo de una red durante **T ms**, en carga estacionaria de esa misma red:

- techo de serialización: **R_red ≤ 1000/T emisiones/s**;
- renovar D dispositivos cada 60 s exige **D/60 < 1000/T**, es decir **D < 60000/T** (igualdad es saturación, sin margen para ráfagas ni administración);
- con utilización objetivo 70 %: **D ≤ floor(42000/T)**, además del cupo contractual **256 aprobados/red**;
- para 128/red, T debe ser **<468,75 ms**; para 256/red, **<234,375 ms**; para hipotéticos 10k/red, **<6 ms**, y el contrato ya los prohíbe.

Ejemplos algebraicos, **no medidas de T**: T=10/50/100 ms → 100/20/10 emisiones/s → umbral 6000/1200/600 dispositivos a 60 s (producto sigue limitado a 256). El inverso del RPS histórico agregado **no es T por red**.

Con los números corregidos, la proyección agregada prudente del peor perfil de 200 clientes de cada escala es:

| Población realmente medida | Peor emisión RPS | RPS × 60 (población agregada al 100 %, extrapolada) | Promedio por red si se conservaran esas redes y reparto uniforme |
|---|---:|---:|---:|
| 1k / 8 redes | 76,383 | 4583 | ~573, pero >256/red y otro tamaño cambiaría el coste |
| 5k / 40 redes | 21,367 | 1282 | ~32 |
| 10k / 79 redes | 10,584 | 635 | ~8 |

Estos son cálculos aritméticos **a coste fijo en cada población**, no capacidad medida de una población reducida. Reducir dispositivos cambia el coste del scan global y el número de consultas; no permite anunciar «solo sostiene 8 por red» como límite del producto. Tampoco «635 en una red»: el throughput observado se reparte por 79 redes. En 10k la demanda sería **166,667 emisiones/s**, ~15,75× el peor RPS observado; las dos tandas de 10 s con drenaje no demostraron esa cadencia ni un ciclo completo de renovación de toda la población.

Como orientación de orden de magnitud, sustituyendo **el proxy SQL**, no T real: 62,699 ms (B antes) daría 15,95 emisiones/s/red y umbral ~957 dispositivos a 60 s; 20,116 ms después, 49,71/s y ~2983. Son extrapolaciones optimistas incompletas, **no capacidad validada ni ganancia HTTP**; ambas quedan subordinadas al cupo de 256. El proxy A de 4457,405 ms daría 0,224/s y ~13 dispositivos, o después 0,912/s y ~55, manteniendo artificialmente el trabajo de cargar 10k en cada emisión. No es una solución estable ni un tamaño de red sostenible: al cambiar N cambia T y ese escenario no se admite.

**Conclusión de dimensionamiento:** no existe todavía una medida de máximo sostenible por red a renovación 60 s. Lo demostrado es el cupo 256, el escenario histórico de hasta128/red, el déficit agregado observado a 10k y el techo algebraico condicionado a medir T. La futura prueba debe medir T(N), concurrencia entre redes y renovación de toda la población, no extrapolar del p95 ni del inverso del throughput agregado.

## 7. Evidencia experimental reproducible y limpieza

Extractos sanitizados de salida real de esta investigación (los números de §5 son transcripción de esos EXPLAIN y `\timing`; no proceden de CI ni del ensayo histórico):

```text
PostgreSQL 17.11 ... on aarch64-unknown-linux-gnu ... 64-bit
CREATE DATABASE
# migraciones 0001,0002,0003,0004: todas terminaron sin error
# A: INSERT 10000 devices; INSERT 10000+10000 ip_allocations
# A antes, IP:
Seq Scan on ip_allocations ... rows=2 loops=1
Rows Removed by Filter: 19998
# A antes, listado:
SubPlan 1: Seq Scan on ip_allocations ... rows=1 loops=10000
SubPlan 2: Seq Scan on ip_allocations ... rows=1 loops=10000
Execution Time: 6994.015 ms
# candidato, A primer listado posterior:
Index Only Scan using ip_allocations_device_family ... loops=10000
Heap Fetches: 0
Execution Time: 51.933 ms
# las tres repeticiones calientes posteriores son las publicadas en §5
# B tras redistribuir:
count|min|max|sum = 79|16|128|10000
count audit_events|count network_samples = 796320|796320
```

Experimento del bloqueo, sobre la forma A y una única invocación docker exec con dos sesiones psql: sesión 1 `BEGIN; SELECT id FROM networks WHERE id=… FOR UPDATE; SELECT pg_sleep(1); ROLLBACK;`; sesión 2 arranca 0,2 s después con `SET lock_timeout='100ms'; SELECT id FROM networks WHERE id=… FOR UPDATE;`. La espera deliberada de 1 s **no mide T del producto**, solo demuestra exclusión de fila. Salida de la segunda sesión:

```text
ERROR:  canceling statement due to lock timeout
CONTEXT:  while locking tuple (0,1) in relation "networks"
```

Para repetir planes se aplican las cuatro migraciones a otra base `diag_*`, se inserta la forma A indicada arriba, se ejecuta `ANALYZE`, y se copia **literalmente** el SQL de `issue.rs:24,29,34` y `devices.rs:132`, sustituyendo parámetros por el UUID de red y `md5('1')::uuid`; se precede cada consulta por `EXPLAIN (ANALYZE, BUFFERS)`. Repetir tres veces antes/después del DDL candidato; no ejecutar con el tráfico histórico ni sobre otra base. Para B se agrupa i por `(i-1)/128`, se crean 79 redes y 10080 filas samples/audit por red, manteniendo los 10k dispositivos y 20k asignaciones. Las fechas de muestras se fijan desde `date_trunc('minute',now())` hacia atrás; el query de métricas conserva el límite1000. El historial vacío y las rutas vacías son límites explícitos de esta reproducción, no resultados sobre datos envejecidos.

**Limpieza efectuada:** se ejecutó desde `postgres`:

```sql
DROP DATABASE diag_control_plane_20261026;
SELECT count(*) AS remaining
FROM pg_database WHERE datname='diag_control_plane_20261026';
```

Salida real: **`DROP DATABASE` y `remaining = 0`**. Quedaron borrados redes, usuario/organización sintéticos, dispositivos, asignaciones, reports, política, muestras/eventos, secuencias avanzadas y el índice experimental. No se crearon sesiones/credenciales privadas, ficheros auxiliares, imágenes, volúmenes ni contenedores; no se terminó ninguna sesión ni se modificaron otras bases. No se publica ningún bundle/cookie de las fixtures históricas.

## 8. Corrección propuesta, coste, riesgo y contrato

Orden recomendado, **sin implementación**:

| Propuesta | Alcance/coste | Riesgo y contrato |
|---|---|---|
| **1. Índice aditivo `ip_allocations_device_family(device_id,family) INCLUDE(addr)`** | Primera corrección pequeña, común a emisión e inventario, A/B SQL demostrado. ~1,05 MiB en20k filas de esta fixture; coste de mantenimiento en altas/bajas/IPAM. Mantener índices únicos actuales. | Sin cambio API/protocolo/autorización. El índice es de rendimiento, no una nueva unicidad. Debe verificarse plan con visibilidad/bloat reales. No implica una mejora HTTP de la misma magnitud. |
| **2. Eliminar 2N round trips en `load_devices`** | Leer direcciones/rutas del conjunto de red en consultas por lote y agrupar sin multiplicar filas IP×ruta; preservar orden/candidatos y semántica. Afecta también validación de políticas que reutiliza el helper. | Más filas por respuesta/agrupación en Rust, pero no cambiar peers ni orden firmado. Probar IPv4/IPv6, sin rutas, varias rutas, pending/revoked, UUID ajeno. Coste medio; mantener bloqueo inicialmente para no cambiar aislamiento mientras se reduce T. |
| **3. Acortar/rediseñar sección crítica, después de medir** | No hacer «quitar FOR UPDATE» aislado. Serializar versiones por dispositivo; combinar bloqueo compartido de red para emisiones con exclusivo en mutaciones podría permitir emisiones simultáneas conservando exclusión frente a cambios. Evaluar compatibilidad de FK/orden de locks. Alternativa snapshot+revalidación con revisión de red, de mayor alcance. | Riesgo alto: revocación concurrente, ACL/rutas/DNS/certificado coherentes, monotonía y deadlocks. Bloquear solo el dispositivo sin protocolo común para mutaciones permite firmar datos obsoletos. Debe conservar contrato `control-plane-v1` §6 y ADR-002; snapshot/revisión necesita especificar nuevo punto de linearización. No mover firma fuera del lock sin revalidar estado antes de publicar. |
| **4. Índice específico de auditoría si sigue siendo caliente** | Candidato parcial `ON audit_events(network_id,target,at DESC) WHERE action='config.issue'`; A/B propio pendiente. No meter condición móvil `now()` en predicado del índice. | Más almacenamiento/escrituras; sin cambiar la política de una entrada/dispositivo/hora. No fue el índice ensayado aquí. |
| **5. Lectura de sesión sin escritura por petición** | No hay lock de red que quitar al inventario. Reducir UPDATE de sesión exige un diseño explícito de actividad/logout/inactividad y prueba con una/múltiples sesiones. | Seguridad/contrato de 1h inactividad y logout; no reemplazar silenciosamente por SELECT que deje de renovar o autorice una sesión revocada. Primero medir su fracción de espera real. |
| **6. Caché de lista de pares, solo si queda coste** | No primera opción: lote+índice más simple. Caché por red necesitaría invalidación al aprobar/revocar/cambiar endpoints/rutas/políticas/DNS/certificado y límites de memoria. No cachear el fichero firmado con versión/lease como si fuera configuración estática. | Datos obsoletos pueden ampliar permisos o retrasar revocación. Introduce contrato de coherencia y riesgo alto; evitar hasta demostrar necesidad. |

Migración aditiva concreta propuesta para después de levantar la congelación: un archivo siguiente al esquema0004, p.ej. `0005_ip_allocations_device_family.sql`, con el `CREATE INDEX` de §5. **No se ha creado ese archivo.** En una ventana pequeña puede ejecutarse dentro de la transacción normal del migrador, aceptando bloqueo de escrituras durante creación. Para base viva grande, el DDL operativo sería:

```sql
CREATE INDEX CONCURRENTLY ip_allocations_device_family
ON ip_allocations(device_id, family) INCLUDE (addr);
```

`CREATE INDEX CONCURRENTLY` **no puede ejecutarse dentro de la transacción predeterminada de una migración SQLx**. Hace falta elegir explícitamente procedimiento fuera de transacción/soporte no-transaccional del migrador, controlar índice inválido si falla y ordenar el registro de migración; no pegar ese DDL en una migración normal ni ocultar conflicto con `IF NOT EXISTS`. Ninguna de estas opciones altera los índices únicos/contrato IPAM; la decisión operativa debe documentarse antes de ejecutar en producción.

## 9. Servidor frente a generador y prueba de salida

**DEMOSTRADO por el arnés:** un runner contiene Python, su ThreadPool y hasta un proceso persistente de firma por worker (`control.py:50-61,107-122`). Los 200 clientes no son 200 máquinas. Cada solicitud cronometra firma/IPC (para emisión), apertura y cierre de nueva conexión HTTP, envío y lectura completa; no hay keep-alive reutilizado (`control.py:68-104`). Se usa HTTP privado directo a CP: no mide HAProxy/TLS para esas fases (`stack.sh:188,210-216`). Control plane y PG son **otros contenedores**, no procesos dentro del runner; comparten anfitrión/VM. El runner se conecta a redes edge/backend (`stack.sh:189-201`); el muestreador solo enumera CP/coordinator/relay/postgres, no Python/firmantes (`stack.sh:224-230`).

El lazo cerrado limita solicitudes pendientes a concurrency, no fija demanda abierta ni demuestra clientes residentes. Tras 10 s nominales drena la cola; el RPS incorpora drenaje. Por ello el p95 incluye servicio+colas+generador, y no se convierte en «tiempo SQL», «tiempo de firma» ni T. CPU CP≤6 % y máximo PG/backend≤23 % no prueban ociosidad de PG agregado. **SIN COMPROBAR:** CPU del generador, de todos los backends sumados, saturación de host/VM, red/IPC/stdout, distribución de adquisición y esperas. Las lecturas también hacen UPDATE de la misma sesión de laboratorio, factor específico del perfil, sin firma AtDevice.

Prueba que verificaría la corrección, a ejecutar tras descongelación y autorización:

1. Repetir A/B de **solo índice** sobre base desechable con 1k/8,5k/40,10k/79 redes (128 por grupo y resto exacto), 10080 muestras/eventos por red y claves/certificados válidos. Misma imagen/binario identificado, hardware y estado inicial; sin descargas; ejecuciones secuenciales. EXPLAIN debe eliminar scans globales de `ip_allocations` en los dos puntos calientes y conservar resultados.
2. Repetir 24 perfiles de lectura/emisión 50/200 de la matriz, pero añadir ventana suficiente para **renovar toda la población** varias veces a 60 s, oferta requerida declarada y drenaje separado; también una sola red de128 y otra de256. No usar10k/red como caso permitido ni elevar cupos para hacer pasar el ensayo. Medir perfiles separados y, después, mezcla representativa explícita.
3. Medir T desde adquisición del lock hasta commit, espera de lock y pool por separado, tiempo SQL por consulta, firma/codificación, CPU agregada de todos los backends y CP, WAL/IO y recursos del generador/firmantes. `pg_stat_activity`/`pg_locks` deben distinguir esperas de row/transaction locks de CPU; no basta CPU del postmaster ni máximo por PID. Para comparar índice sin tocar producto se puede usar muestreo PG y tracing externo; para tiempos internos exactos hace falta instrumentación aprobada después de la congelación.
4. Calibrar generador con endpoint/barato conocido, medir CPU/IPC y repetir desde otro runner/anfitrión con capacidad suficiente, manteniendo firmado real. Comparar nueva conexión frente a sesión persistente **como dos perfiles distintos**, no cambiar el perfil original silenciosamente. Comparar una y varias sesiones admin para aislar `UPDATE sessions`; no presentar diferencias como optimización del producto ya implementada.
5. Aceptación funcional: firmas válidas, versiones crecientes bajo concurrencia de mismo dispositivo, ningún dispositivo revocado incluido tras el punto de linearización acordado, políticas/rutas/DNS coherentes, una auditoría/hora y todos los registros de versión; 257.º aprobado sigue409 network_full, overflow heredado sigue cerrado, aislamiento entre redes y sin deadlocks. Conservar pruebas de lease300/60/30 y revocación incluso bajo carga.
6. Aceptación de capacidad: registrar throughput **útil** (no429), p95/p99 y margen con renovación completa, y cumplir SLO acordado antes de fijar soporte. Proponer provisionalmente utilización≤70 % de la capacidad observada, no aprobar19s de p95 por pasar un presupuesto histórico. La prueba es negativa si el generador no sostiene la oferta o alguna población pierde lease; no basta «sin500».

No se implementó ninguna de las propuestas ni se ejecutó carga HTTP/Rust, CI o benchmark de producción. Permanecen sin medir el reparto causal exacto del p95 histórico y el máximo sostenible por red a60s; el índice mejora SQL demostrado, no garantiza por sí solo el SLO end-to-end.

## Aplicado

**2026-10-26 — corrección 1 autorizada tras este diagnóstico.** Se añade `apps/control-plane/migrations/0005_ip_allocations_device_family.sql` con `CREATE INDEX IF NOT EXISTS ip_allocations_device_family ON ip_allocations(device_id, family) INCLUDE (addr)`, exactamente el índice ensayado en §5, sin retirar los índices únicos existentes. Las migraciones 0001–0004 no cambian esas columnas; emisión filtra por `device_id` y ordena por `family`, inventario filtra por `device_id,family`, e IPAM conserva su acceso por `network_id`. No cambia API, autorización, cuotas ni formatos firmados.

Se elige `CREATE INDEX` normal transaccional: bloquea brevemente escrituras sobre `ip_allocations` durante la creación. Aplique 0001–0005 **antes de desplegar el binario nuevo**, mientras N funciona sobre el esquema ampliado de N+1 (ADR-009 §7); rollback solo de binarios. `CONCURRENTLY` queda descartado dentro de la transacción SQLx, no constituye el procedimiento de esta migración.

Regresión PostgreSQL real: `migracion_indice_asignaciones_cubre_emision_y_listado_e_idempotencia` (`apps/control-plane/tests/migrations.rs`) siembra 10 000 dispositivos en 79 redes y 20 000 asignaciones, ejecuta `ANALYZE` y `VACUUM ANALYZE ip_allocations` como en el A/B de §5, comprueba mediante `EXPLAIN (FORMAT JSON)` los accesos de emisión y los dos subplanes del listado completo sin desactivar `enable_seqscan`, y exige `Index Only Scan`/`Index Scan` sobre el índice nuevo. Comprueba además la definición del catálogo, una segunda pasada del migrador, historial `1,2,3,4,5` y conservación de las 20 000 filas. Antes de añadir 0005 falló por `Seq Scan on ip_allocations` en emisión. Sin fijar visibilidad tras la carga, PostgreSQL eligió legítimamente `Bitmap Heap Scan` con el índice nuevo: la aceptación index-only se limita a la fixture estabilizada, no promete ese plan bajo escrituras reales. No necesita reloj inyectado: el comportamiento probado es selección de acceso SQL, no agenda temporal.

Las listas del ensayo de upgrade se amplían a `1,2,3,4,5` sin cambiar su baseline N anterior a 0003; el negativo de backup usa ahora la migración futura 6. La medición HTTP de extremo a extremo y los laboratorios de stack, infraestructura y recuperación quedan para ejecución del responsable; esta corrección no demuestra una ganancia HTTP ni un nuevo techo de capacidad.

Resultado local final: `CARGO_BUILD_JOBS=2 cargo test -p atenea-control-plane --tests --offline --locked`, con `ATENEA_TEST_DATABASE_URL` apuntando al PostgreSQL 17 de pruebas existente, terminó con **40 pruebas aprobadas, 0 fallidas, 0 omitidas**. Antes se detectó una caché incremental que no incorporaba el fichero nuevo de migración: `apps/control-plane/build.rs` declara ahora `cargo:rerun-if-changed=migrations`, sin dependencias nuevas. Prueba mecánica: un build base, añadir un fichero temporal no SQL a `migrations/`, y otro build offline/locked que volvió a compilar el paquete; el temporal se retiró antes de la pasada final.
