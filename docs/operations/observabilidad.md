# Observabilidad de Atenea One (fase 9)

## Frontera de red

Los tres procesos ofrecen `GET /metrics` (texto Prometheus 0.0.4) en un **listener
HTTP independiente**, sin autenticación propia. Defaults:

| Servicio | Variable de entorno / campo TOML | Dirección |
| --- | --- | --- |
| Control plane | `ATENEA_CP_METRICS_LISTEN` / `metrics_listen` | `127.0.0.1:9101` |
| Coordinator | `ATENEA_COORD_METRICS_LISTEN` / `metrics_listen` | `127.0.0.1:9102` |
| Relay | `ATENEA_RELAY_METRICS_LISTEN` / `metrics_listen` | `127.0.0.1:9103` |

**No publicar estos puertos en Internet, ni añadir reverse_proxy ni backend
HAProxy para `/metrics`.** El router público del control plane reserva `/metrics`
como 404 incluso cuando sirve el panel SPA. El coordinator devuelve 404; el
listener público del relay no habla HTTP. El endpoint por red
`/api/v1/networks/{net}/metrics` del panel es otra API autenticada, no el exportador.
Si un puerto de métricas no se puede abrir, el servicio falla al arrancar.

El overlay utiliza `0.0.0.0:9100` dentro de contenedores sin `ports`, con red
`monitoring` interna. Relay y HAProxy comparten el namespace estable de `netns`
para conservar la frontera loopback de PROXYv2; Prometheus scrapea relay en
`netns:9100`, **no** en el listener 443 de HAProxy. Solo procesos de confianza deben unirse a las redes de
estos contenedores: Docker permite alcanzar también las interfaces `edge/backend`
entre sus miembros. Un firewall de host debe denegar accesos a subredes Docker
desde redes externas. Este aislamiento no sustituye autenticación si se despliega
un exportador fuera de una máquina/red de confianza.

## Activación del overlay

Preparar antes la instalación base descrita en ADR-009. Establecer el nombre DNS
privado `ATENEA_GRAFANA_HOST` (por ejemplo `metrics.example.test`); crear con RNG
una contraseña fuerte en `${ATENEA_RUNTIME_DIR}/secrets/grafana-admin-password`,
legible por el uid 472 del contenedor Grafana. Se monta como fichero secreto, nunca
como valor en Compose. Ejemplo sintético de comando, sin desplegar desde este repo:

```sh
docker compose --env-file /ruta/privada/stack.env \
  -f deploy/compose/prod/compose.yaml \
  -f deploy/compose/prod/observability.yaml config --quiet
docker compose --env-file /ruta/privada/stack.env \
  -f deploy/compose/prod/compose.yaml \
  -f deploy/compose/prod/observability.yaml up -d
```

Grafana solo se alcanza mediante el bloque Caddy importado de `grafana.caddy`.
Usa el mismo `CADDY_TLS` que el panel y autenticación propia de Grafana; anónimo y
alta de usuarios están desactivados. Para el piloto restringir su nombre DNS y su
acceso por firewall/VPN. Prometheus, Loki, Alloy y node_exporter no publican puertos.
Grafana no es una copia de seguridad: conservar los ficheros de provisioning y
respaldar por separado su volumen si se permiten cambios operativos locales.

## Métricas y cardinalidad

`atenea-observability` reutiliza Axum/Tokio; no añade SDK ni librerías externas de
métricas. Contadores en memoria: reiniciar un proceso los reinicia; usar `rate` e
`increase`, no diferencias manuales entre muestras que crucen reinicios.

- `atenea_http_requests_total{route,method,code}` y
  `atenea_http_duration_seconds_{bucket,count,sum}`: las plantillas vienen de
  **MatchedPath de Axum**, nunca de URI ni query. Fallback = `unmatched`; métodos
  no estándar = `OTHER`. Límites del histograma: 5/25/100/500 ms, 2/30 s y +Inf.
  Peticiones long-poll y WebSocket incluyen el tiempo hasta devolver respuesta;
  la duración de una conexión WS se expresa por su gauge, no este histograma.
- Control plane: `atenea_database_errors_total` (incluye conflictos SQL),
  `atenea_websocket_connections`, `atenea_leases_{issued,renewed,rejected,expired}_total`,
  `atenea_approvals_total`, `atenea_revocations_total`. Primera configuración de
  dispositivo = emisión; las siguientes = renovaciones. Rechazo = intento de
  obtener configuración que no termina en 2xx. Aprobaciones incluyen dispositivos
  y rutas. El muestreador cuenta cada minuto dispositivos cuyo último lease
  expiró desde el último barrido correcto, sin renovar: no representa paquetes
  bloqueados ni confirma el reloj de un agente. No recupera expiraciones anteriores
  al arranque del proceso; el contador no es un registro de auditoría.
- Coordinator: rutas `/v1/announce`, `/v1/rendezvous`, `/v1/relay` con código HTTP
  como resultado; `atenea_presences` barre presencias expiradas antes del scrape;
  `atenea_relay_tokens_issued_total` cuenta dos tokens por petición autorizada;
  `atenea_relay_token_rejections_total{reason}` usa ocho motivos cerrados:
  `invalid_signature`, `invalid_config`, `peer_unavailable`, `not_announced`,
  `rate_limited`, `relay_unavailable`, `invalid_request`, `other`.
- Relay: `atenea_relay_{udp,tls}_sessions`, `atenea_relay_tls_connections`,
  `atenea_relay_allocations`, `atenea_relay_allocations_total` (ALLOC aceptados,
  incluidas renovaciones), `atenea_relay_{packets,bytes}_total{transport,direction}`.
  Ingress cuenta DATA recibido antes de validarlo; egress solo reenvío aceptado por
  UDP/cola TLS. Bytes = carga cifrada sin framing. Sesiones = asignaciones vinculadas
  que quedan en tabla; una expiración puede esperar al barrido de cinco segundos.
  `atenea_relay_drops_total{reason}` tiene los trece motivos del README del relay;
  `atenea_relay_rejects_total{code}` usa los códigos ALLOC_ERR 1–8 del protocolo.
  Los límites aplicados se ven en `allocation_rate`, `cookie_rate`, `tcp_capacity`
  y código 3 (`RateLimited`); los rechazos de capacidad usan su código normativo.
- node_exporter habilita **solo filesystem** para disco; no publica interfaces,
  direcciones de red ni hostname. Sus etiquetas de disco/montaje pertenecen al host,
  no son identificadores de dispositivos VPN.

Los contadores de eventos compartidos se publican con cero también en servicios
que no los producen: filtrar siempre `job` en PromQL. El número de series depende
solo de rutas, métodos/códigos y motivos de conjuntos cerrados; no hay etiquetas
con dispositivo, red, usuario, IP, clave, token o ruta parametrizada. La prueba
`many_devices_networks_and_private_inputs_do_not_create_series` envía 10.000
combinaciones red/dispositivo con token/query/cookie privados y conserva exactamente
el mismo número de series. Las muestras Prometheus sí contienen la instancia de
scrape (nombre de servicio privado de Docker), no direcciones de clientes.

## Logs y retención

No se registran cabeceras, cuerpos HTTP, configuraciones, tokens, cookies, claves,
correos ni direcciones. Se retiraron las direcciones de escucha del mensaje de
arranque del relay y el detalle dinámico de JoinError; errores de arranque se
resumen por servicio para que Figment/errores de terceros no impriman entradas
privadas. Ante un arranque fallido comprobar presencia/permisos de ficheros,
puertos, migraciones y acceso PostgreSQL sin copiar secretos a registros.
Mantener `RUST_LOG=info`: activar trazas de bibliotecas HTTP/OIDC/SQL de terceros
no forma parte de la garantía de privacidad operativa y puede revelar metadatos.
No habilitar access logs con query o cabeceras en el proxy.

Alloy lee solo ficheros JSON Docker montados de solo lectura: **no tiene el socket
Docker**, ni siquiera un montaje `:ro` que permitiría peticiones mutadoras a su API.
El overlay pide al driver `json-file` que incluya `com.docker.compose.service`.
Alloy descarta servicios ajenos y conserva únicamente la etiqueta `service` del
conjunto `control-plane|coordinator|relay`; no etiqueta filename, contenedor, IP,
usuario ni contenido. Es necesario recrear contenedores preexistentes para aplicar
las opciones de logging. El directorio `/var/lib/docker/containers` es el del
servidor Linux, no el del cliente macOS que ejecuta Compose.

Rotación Docker: 3 × 10 MB por servicio. Loki: retención siete días, compactor y
almacén filesystem en volumen propio; Prometheus: siete días. No hay envío a un
SaaS. Loki/Prometheus sin autenticación son exclusivamente internos. Dimensionar
los volúmenes y revisar el dashboard de disco; los límites de ingestión no
sustituyen el dimensionado. La retención requiere tiempo de compactación: no es
una promesa de borrado instantáneo del último byte a los siete días.

## Alertas y diagnóstico

Cinco reglas en `alerts.yaml`: servicio caído un minuto, relay sin sesiones
quince minutos (aviso, normal si no hay clientes), 5xx sostenidos, leases que
expiran sin renovar, disco libre <15% diez minutos. Revisar `up` antes de atribuir
un contador a fallos del cliente. `increase` requiere suficientes scrapes. Las
reglas se evalúan y aparecen en Prometheus; **no hay Alertmanager ni notificación
externa configurada**. El operador debe integrar su canal si necesita avisos fuera
de la interfaz, sin publicar el API interno de Prometheus.

Los tres dashboards de Grafana (`atenea-control-plane`, `atenea-coordinator`,
`atenea-relay`) y los datasources Prometheus/Loki están provisionados por fichero;
no hace falta instalar plugins ni importar recursos externos.

## Reproducir el ensayo

```sh
tests/observability/run.sh
```

Exige Docker y la imagen local `atenea-lab`; no instala nada en macOS. Monta el
repo de solo lectura, usa la caché existente sin borrarla y crea una red/volumen
propios que elimina al terminar. Levanta PostgreSQL y Dex reales, los tres
servicios y dos agentes con TUN/namespaces mediante el laboratorio de fase 6;
realiza OIDC, enrolamiento, aprobación, emisión/renovación, announce y tráfico
cifrado por relay UDP tras bloquear P2P. Prometheus real consulta series por
PromQL. Se comprueba que `/metrics` no sirve métricas desde los listeners públicos
HTTP/TLS; las conexiones TLS de prueba usan CA efímera y el SNI/ALPN del relay.

La prueba envía tokens, cookies, claves, correo, IP y configuración-canario, además
de comprobar tokens/cookies/claves/configuraciones realmente generados. Inspecciona
los logs originales de los tres procesos y arranques con configuración inválida.
Copia esos registros reales a ficheros con el formato JSON del driver Docker;
Alloy consume esos ficheros y Loki debe devolver registros de los tres servicios
sin secretos ni etiquetas dinámicas. No se simula la respuesta de Loki.
Se valida la configuración/reglas con promtool y se consulta la salud real de
Grafana. El laboratorio no utiliza el socket ni el directorio del daemon de
Docker Desktop para recoger logs: ese montaje de producción requiere Linux.

## Límites de la verificación

Sin despliegue real, firewall/DNS/CA de producción, acceso externo, prueba de
carga, retención de siete días transcurridos, disparo/entrega de alertas, ni revisión
visual en navegador de los dashboards. La salud de Grafana no demuestra revisión
visual. El node_exporter del host Linux y la recogida desde el directorio real del
daemon quedan sin comprobar en este laboratorio macOS; se ensaya la misma
configuración Alloy con registros reales en formato Docker. Las pruebas no son
una auditoría de todas las combinaciones posibles de `RUST_LOG` de terceros.
