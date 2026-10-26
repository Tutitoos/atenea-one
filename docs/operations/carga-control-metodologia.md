# Carga de control: metodología y límites

Este documento describe un **arnés ejecutable**, no resultados medidos. No se han ejecutado aquí Docker, compilaciones ni cargas. No sustituye a las pruebas funcionales de `tests/stack` ni a los agentes y NAT reales de `tests/network-lab`.

## Alcance y componentes

- `tests/load/stack.sh control` reutiliza el stack de producción, el override de `tests/stack`, bootstrap/migraciones y el runner local. Los contenedores tienen prefijo `atenea-load-`, las redes son internas y no se publican puertos del host. Se comprueban imágenes locales; no se permite descargar imágenes ni dependencias durante la carga.
- `tests/load/control.sh` es el entrypoint **dentro** del runner. No llama Docker. Ejecuta perfiles secuencialmente y conserva JSONL y stderr separados incluso si una fase falla. Un error de una fase hace que el resultado global sea no cero, pero no evita intentar las siguientes fases alcanzables.
- `tests/load/control.py` usa Python estándar, HTTP real, PostgreSQL real mediante `psql`, long-polls reales y WebSocket RFC 6455 real. No simula respuestas de los servicios.
- `atenea-load-sign`, binario de `atenea-testpeer`, reutiliza `atenea-identity` para bundles, certificados, configuración y cabeceras AtDevice. La generación verifica sus propios bundles/configuraciones con la biblioteca del producto. Las firmas de carga se sirven por procesos persistentes, uno por worker Python, no mediante un proceso OpenSSL por petición.

## Prerrequisitos reales

1. Imágenes locales del stack enumeradas por `tests/load/stack.sh`, incluidas PostgreSQL 17, control-plane, coordinator y runner. Las imágenes del producto deben corresponder al código que se quiere medir.
2. Volumen local `atenea-lab-target` con los ejecutables Linux release actualizados del producto y helper `tests/load/results/bin/atenea-load-sign`, montado en `/load-bin` y disponible por `PATH` en el runner. También puede señalarse otra ruta con `ATENEA_LOAD_SIGNER`. Compilarlo es un paso **previo y autorizado por el operador**, no parte del ensayo; no se escribe en el caché protegido durante la carga:

   ```sh
   # En el entorno Linux de compilación ya preparado, sin descargas:
   cargo build --offline --release -p atenea-testpeer --bin atenea-load-sign
   ```

   El helper usa `ed25519-dalek` ya existente en el workspace, declarado directamente en `atenea-testpeer` y en su entrada del lockfile.
3. El runner necesita Python 3, `psql` y la libc/bibliotecas del binario compilado. La imagen del laboratorio de red instala Python y PostgreSQL client; no se instalan paquetes desde el arnés.
4. Base de datos **desechable y ya migrada**, y conexión administrativa `DATABASE_URL` solo de laboratorio. El servicio sigue usando su rol restringido habitual. Sembrar/borrar fixtures no debe ejecutarse contra producción.
5. Raíz y emisor exclusivos del laboratorio, montados en lectura: `ATENEA_LOAD_ROOT_KEY=/runtime/offline/root.key` y `ATENEA_LOAD_ISSUER_KEY=/runtime/secrets/issuer.key`. El coordinator debe confiar en esa raíz y el control-plane debe usar ese emisor. No se cambia el ancla ni se desactiva la verificación TLS. Sin estas variables, el helper también puede generar claves nuevas para un ensayo manual, pero los servicios deberán configurarse expresamente con ellas.

## Ejecución secuencial

Desde el host, una sola carga a la vez; no solapar con pruebas de túnel, abuso, soak ni otra instancia de Docker de este laboratorio:

```sh
bash tests/load/stack.sh control
```

La salida queda en `tests/load/out/atenea-load-<pid>/control/`. `stack.sh` conserva además su inventario/estadísticas según su propio contrato y elimina únicamente los recursos de ese proyecto. No se deben usar `prune` ni nombres ajenos para limpiar.

El entrypoint completa primero **toda la matriz coordinator**: 1000, 5000 y 10000 dispositivos, cada población con **dos repeticiones** independientes de actividad y cuotas. Genera una fixture nueva por población y la reutiliza entre las dos repeticiones; espera 65 s tras cada una para liberar presencias y cubos antes de repetir o cambiar de escala.

Después completa **toda la matriz PostgreSQL/control-plane**, de nuevo en orden 1000, 5000 y 10000. Para cada población:

1. Siembra una sola vez PostgreSQL y siete días de muestras, reutilizando la fixture generada antes; crea credenciales sintéticas de sesión locales.
2. Ejecuta dos repeticiones de lecturas y emisión, primero con hasta **50**, después con hasta **200** peticiones en vuelo; cada combinación genera su fichero `N-r1-*` o `N-r2-*`.
3. En cada repetición abre **64 WebSockets** por defecto y produce cambios reales mediante informes de dispositivo firmados.
4. Tras ambas repeticiones elimina solamente la organización y usuarios de la fixture, sus registros asociados y su auditoría sintética, con el rol administrador del laboratorio.

La ventana activa predeterminada es de **10 s** por carga normal de coordinator/control-plane/WebSocket; las cuotas añaden sus esperas reales de long-poll. La precarga, el drenaje de peticiones y los cooldowns quedan fuera de esa duración objetivo. Puede ampliarse con `LOAD_CONTROL_SECONDS`, sin convertir estos ensayos en un soak.

Hay pausas de 65 s entre fases de control-plane y entre poblaciones para no heredar la ventana de cuota ni las presencias del caso anterior. Se conservan ficheros de fixture para trazabilidad; contienen claves **solo de laboratorio** y cookies, por lo que la carpeta no debe publicarse como artefacto abierto.

Para una ejecución manual dentro de un runner **ya** conectado a las redes internas y con las variables del contrato de `stack.sh`, sin lanzar servicios adicionales:

```sh
# /src RO; /results RW; helper en PATH; las tres claves/URLs y DATABASE_URL ya exportadas.
export LOAD_CONTROL_COUNTS='1000 5000 10000'
export LOAD_CONTROL_SECONDS=10
export LOAD_CONTROL_WS=64
bash /src/tests/load/control.sh
```

`control.sh` acepta además `LOAD_CONTROL_OUT`, `LOAD_CONTROL_PER_NETWORK` (128), `LOAD_CONTROL_EVENTS` (10080/red), `LOAD_CONTROL_RATE` (solo tráfico coordinator, por defecto aproximadamente 2 solicitudes/s/red), `LOAD_CONTROL_POLL_CAPACITY` (256, debe coincidir con el servidor) y `ATENEA_LOAD_SIGNER` (ruta del helper). `LOAD_CONTROL_OUT` debe ser nuevo. Si se ejecuta vía host, las variables deben reenviarse al runner por `stack.sh`; no basta con definirlas en el host cuando no estén en su lista de reenvío.

Ejemplos de fases individuales sobre fixtures existentes, **una tras otra**:

```sh
python3 /src/tests/load/control.py self-check
python3 /src/tests/load/control.py coordinator --fixtures /results/control/fixtures-1000 \
  --signer atenea-load-sign --url "$COORD_URL" \
  --concurrency 200 --duration 60 --rate 16
python3 /src/tests/load/control.py control-plane --fixtures /results/control/fixtures-1000 \
  --signer atenea-load-sign --url "$CP_URL" \
  --workload read --concurrency 50 --duration 60
# Esperar 65 s antes de otra fase firmada.
python3 /src/tests/load/control.py control-plane --fixtures /results/control/fixtures-1000 \
  --signer atenea-load-sign --url "$CP_URL" \
  --workload issuance --concurrency 200 --duration 60
python3 /src/tests/load/control.py websocket --fixtures /results/control/fixtures-1000 \
  --signer atenea-load-sign --url "$CP_URL" \
  --origin https://panel.stack.test --connections 64 --duration 60
```

Las fases individuales dejan JSONL en stdout: redirigirlo a un fichero nuevo. TLS usa raíces del sistema o `--ca <CA-del-laboratorio>`, nunca `verify=false`. Con HTTP privado no se mide TLS; el WebSocket debe enviar el `Origin` del `PUBLIC_URL` del servicio, aunque conecte por su IP interna.

## Coordinator: población, actividad y cuotas

Cada grupo tiene hasta 128 dispositivos por defecto; todos los pares del grupo aparecen mutuamente en configuraciones firmadas válidas. El máximo configurable de fixture es 256 y no se crean redes de 10000 pares, que infringirían el contrato del producto. Se usan nuevas identidades aleatorias, nonces aleatorios por solicitud, tiempo actual y candidatos reservados de documentación. No se envía tráfico de datos a esos candidatos.

La precarga intercala redes y se limita a 5 announces/s/red para no convertir la siembra en una prueba accidental del cubo compartido de 8/s/red. Cada 20 s se inicia, cuando termina el ciclo anterior, otra vuelta de renovación. Las configuraciones sintéticas duran como máximo una hora; la fase coordinator debe terminar dentro de esa validez. La presencia final de `/healthz` debe coincidir exactamente con N, no basta con contar solicitudes enviadas. Una vuelta lenta, rechazo de announces o caducidad se reflejan como fallo, no se etiquetan como éxito de escala.

La actividad alterna `POST /v1/rendezvous` firmado y `GET /v1/events?wait=N` firmado. Rendezvous usa un par autorizado real; el poll recibe la cola del dispositivo. `--wait` está acotado a 0..30. El perfil normal mantiene la cadencia por debajo del cubo nominal; `--rate 0` significa lazo cerrado sin limitación voluntaria, **no** tasa cero.

El modo `--quota` añade:

- Dos announces consecutivos del mismo dispositivo: el segundo debe ser `429`.
- Dos polls pendientes del mismo dispositivo: un tercero debe ser `429`; otra red debe poder consultar; tras responder los polls debe recuperarse la capacidad.
- `ceil(max_long_polls/4)` polls pendientes de dispositivos distintos de una red, espaciados para no agotar por sí solos el cubo de red: uno más debe ser `429`; otra red debe seguir operativa y al terminar los polls debe recuperarse capacidad.
- Una ráfaga firmada posterior del mismo dispositivo, conservando como tales las respuestas `429` y distinguiéndolas de `401` y errores de transporte.

Para la cuota de red hacen falta más dispositivos en un grupo que `ceil(max_long_polls/4)`; el grupo por defecto de 128 cubre el máximo por defecto de 64 polls/red. Una configuración distinta exige adaptar ambos parámetros. Las pruebas observan el contrato HTTP; un `429` no identifica por sí solo qué cubo interno lo produjo si coinciden límites. La caché de nonces y el límite global deben analizarse junto con métricas/configuración del servidor; este arnés **no afirma** aislar ni demostrar exhaustivamente todas las ramas internas de cuota/cancelación.

**Límite de admisión:** el valor por defecto del producto es 4096 presencias. Con ese valor, 5000 y 10000 **deben registrar rechazo/fallo de población completa**, no una capacidad de 10000. Para medir admisión efectiva de 10000 hace falta un ensayo separado con `ATENEA_COORD_MAX_PRESENCES=10000` (o TOML equivalente), registrar también los otros límites de tablas/cubos y dejar explícito que se ha cambiado la configuración. El worker no eleva límites ni reinicia servicios de forma oculta.

## PostgreSQL y control-plane

La fixture contiene N dispositivos aprobados, bundles criptográficamente válidos, IPs de ambas familias y una política `any` por red. Cada red tiene su certificado de emisor firmado por la raíz del laboratorio. Se insertan directamente en la base para que el tiempo de enrolamiento/OIDC no contamine la carga: **no es una medición de enrolamiento, IPAM, RBAC ni login**. Los CIDR pueden repetirse entre redes sembradas; este seed no afirma haber recorrido la validación de solapamiento del API de creación.

Por cada red se insertan **10080 muestras** a intervalos de un minuto (siete días), informes actuales de todos los dispositivos y **10080 eventos sintéticos de auditoría** por defecto. Los puntos son fixtures señaladas como tales, no siete días observados de funcionamiento ni métricas del túnel. Son agregados por red, según el esquema real, no N×10080 informes por dispositivo. El sampler real puede añadir/reemplazar el punto del minuto actual y purgar datos según su retención normal.

- `--workload read`: alterna inventario, métricas desde hace siete días (`limit=1000`, límite real del API) y auditoría (`limit=100`). Son lecturas autenticadas que alcanzan PostgreSQL.
- `--workload issuance`: `GET /api/v1/device/config` firmado con cada identidad real de fixture; el servidor construye/firma, registra versión y actualiza PostgreSQL. No se reemplaza por un `SELECT 1`, ni por un benchmark SQL de una consulta parecida.
- `--workload mixed`: intercala emisión y las tres lecturas; queda disponible para ensayos explícitos, no sustituye las fases separadas de la matriz.

Se prueban hasta 50/200 solicitudes pendientes, sin cola ilimitada. Las respuestas `429` de emisión son **rechazo**, no throughput útil; el proceso sale con error ante respuestas inesperadas. Los resultados de lectura y emisión deben publicarse por separado. Emisiones a dispositivos de la misma red compiten por los bloqueos/transacciones reales. No se ejecuta una carga SQL adicional en paralelo que cambie la competencia por CPU o conexiones.

## WebSocket N

La fixture crea ocho usuarios miembro y ocho sesiones de laboratorio, permitiendo hasta 64 conexiones sin infringir el máximo de ocho por usuario. El worker distribuye grupos de ocho conexiones por sesión. Con N mayor que 64 se espera que solo entren 64 y que los demás reciban `429`; no se presenta ese rechazo como una difusión de N conexiones admitidas. Con un fichero de sesión externo sin pool, el máximo esperado es ocho.

Se valida el `Sec-WebSocket-Accept`, se leen eventos JSON, se responde a ping con pong enmascarado y se realiza cierre. En paralelo con estas conexiones —dentro de **la misma fase**, no otro generador— se envía `POST /api/v1/device/status` firmado cada 5,1 s, provocando cambios reales. La fase exige que cada conexión admitida reciba al menos un evento `changed`, identifica sus eventos por conexión y cuenta upgrades, receptores, rechazos y errores. No mide render del navegador, resincronizaciones REST del panel, consumidores lentos ni latencia end-to-end de propagación; `seq` no contiene timestamp de emisión y no se convierte en una latencia inventada.

## JSONL e interpretación

Cada fichero de fase contiene registros `run`, `request`, `summary` y `exit` según corresponda; hay registros específicos `seed`, `db-size`, `presence`, `websocket-upgrade`, `websocket-event`, `websocket-summary` y `cleanup`. Los errores detallados/tracebacks quedan en `.stderr`. Se conservan códigos HTTP, bytes de respuesta, latencia por solicitud, distribución de estados y percentiles nearest-rank p50/p95/p99 por fase. `summary.wall_s` es la ventana monotónica entre el inicio de la primera petición y el final de la última de esa fase, incluye huecos y drenaje; `success_rps` cuenta exclusivamente respuestas 2xx divididas por esa ventana, mientras `request_rps` incluye todos los resultados. Las fases intercaladas tienen ventanas que se solapan: no sumar sus tiempos como si fueran secuenciales. El tiempo de solicitud incluye firma, establecimiento HTTP, transporte y lectura completa; no es tiempo CPU puro del servidor. El cliente abre una conexión HTTP por solicitud: registrar esta metodología al comparar con agentes con pooling.

Los percentiles incluyen todos los resultados de la fase, también rechazos, por lo que no deben venderse como percentiles exclusivos de éxito sin filtrar las filas `request`. No se mezclan los tiempos de siembra con las latencias de API. Contar los `200`/`204` útiles y las duraciones observadas para throughput; no convertir concurrencia configurada en RPS medidos. Capturar también imágenes/digests, configuración efectiva, recursos del host/VM y estadísticas del servidor para comparar ejecuciones.

`db-size` consulta después de la siembra y después de cada carga de control-plane/WebSocket el tamaño físico de la base completa (`physical_bytes`) y los tamaños totales con índices/TOAST de `network_samples`, `devices` y `audit_events`, mediante las funciones reales de PostgreSQL. Son tamaños del estado observado, no tamaño de payload ni estimaciones N×fila. La reutilización y limpieza pueden dejar espacio físico asignado; sin VACUUM FULL no se presupone que el tamaño global regrese al inicial entre poblaciones. No se muestra el URI administrativo ni contraseñas en los registros JSONL.

La CPU/memoria del generador, procesos de firma, stdout/JSONL y apertura de sockets pueden limitar el resultado. No hay calibración automática ni atribución de un techo al servidor si el cliente no sostiene la carga. No se han establecido umbrales de aceptación de latencia ni se han inventado mediciones en este documento.
