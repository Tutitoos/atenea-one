# Coordinator de Atenea One

Servicio sin base de datos: presencia, rendezvous autorizado mutuamente y dos puertos STUN propios. Los candidatos son pistas, nunca rutas autenticadas. Configuración normativa: `docs/protocol/coordinator-v1.md` §§2–3 y 6.

## Arranque

```sh
ATENEA_COORD_ANCHORS='["<clave pública raíz: 64 caracteres hex>"]' \
  cargo run -p atenea-coordinator
```

Preparar el ancla con la **clave pública** de la raíz que firmó el certificado de emisor; no usar la clave de identidad del dispositivo ni la del emisor. Codificar sus 32 bytes públicos en hexadecimal; nunca copiar el fichero de clave privada. Se admiten varias raíces explícitas; el coordinator no aprende `next_anchor` de los anuncios. Una lista vacía o claves débiles/incorrectas impiden arrancar. HTTP escucha por defecto solo en loopback; para exposición pública usar un proxy HTTPS de confianza sin registrar Authorization ni cuerpos. `observed_ip` es la dirección TCP real, no se confía en `X-Forwarded-For`.

`ATENEA_COORD_CONFIG=/ruta/coordinator.toml` carga TOML opcional; después las variables `ATENEA_COORD_*` prevalecen. Claves TOML en minúsculas. Listas en variables con sintaxis Figment: `ATENEA_COORD_STUN_PORTS='[3478,3479]'`, `ATENEA_COORD_ANCHORS='["hex", "hex"]'`.

| Sufijo de variable / clave TOML | Predeterminado | Uso |
| --- | --- | --- |
| `LISTEN` / `listen` | `127.0.0.1:8081` | Dirección HTTP |
| `STUN_BIND` / `stun_bind` | `0.0.0.0` | IP UDP; `::` permite IPv6 según la configuración dual-stack del sistema |
| `STUN_PORTS` / `stun_ports` | `[3478,3479]` | Dos puertos UDP distintos y no nulos |
| `ANCHORS` / `anchors` | `[]` (requiere configuración) | Raíces públicas hex de 32 bytes |
| `PRESENCE_TTL_SECS` | `60` | TTL máximo; también limita `not_after + 30` |
| `MAX_BODY_BYTES` | `163840` | Cuerpo HTTP máximo |
| `MAX_PRESENCES` | `4096` | Tabla de presencias |
| `MAX_LONG_POLLS` | `256` | Peticiones events simultáneas |
| `MAX_NONCES` | `65536` | Nonces retenidos 120 s; tabla llena rechaza autenticación |
| `MAX_RATE_ENTRIES` | `65536` | Entradas por tabla de cubos; llena deniega claves nuevas hasta reponer cubos |
| `ANNOUNCE_INTERVAL_SECS` | `2` | Cubo por dispositivo, ráfaga 1 |
| `RENDEZVOUS_PAIR_INTERVAL_SECS` | `1` | Cubo por par dirigido, ráfaga 1 |
| `RENDEZVOUS_PER_MINUTE` | `10` | Cubo por dispositivo, ráfaga 10 |
| `STUN_PER_SECOND` | `20` | Cubo por IP, compartido entre ambos puertos |
| `STUN_BURST` | `40` | Ráfaga STUN |
| `RELAY_SIGNING_KEY_FILE` | Sin clave | Semilla Ed25519 de 32 bytes hex (64 dígitos, salto final opcional); fichero regular con permisos 0600 |
| `RELAYS` | `[]` | Lista JSON de `{name, udp?, tls?, tls_name?}`; al menos UDP o TLS `host:puerto`, nombre TLS obligatorio si hay TLS |
| `RELAY_TOKEN_TTL_SECS` | `300` | Vida del token, entre 1 y 600 segundos |

Para las filas sin clave TOML explícita se usa el sufijo en minúsculas. Todos los límites son positivos. Los cubos cuentan intentos autenticados, incluidos intentos no disponibles; no hay estado duradero. Barrido cada 5 s y antes de acceder a presencia; eventos caducan a 30 s, máximo 16 (se elimina el más antiguo). Máximo 8 candidatos saneados por anuncio. `RUST_LOG=info` activa el registro operativo sin configuraciones, firmas ni nonces.

## API pública

Todas las rutas `/v1` requieren `Authorization: AtDevice v1 device=<hex16>, ts=<unix>, nonce=<hex16>, sig=<hex64>`, construido con `atenea_identity::api_auth::sign_request`. Firma sobre método, ruta **y query exacta**, timestamp, nonce y hash del cuerpo. Ventana inclusiva ±60 s; nonce de un solo uso durante 120 s por dispositivo. JSON estricto sin campos desconocidos.

- `POST /v1/announce`: `{"config":"<base64 estándar de SignedConfig>","candidates":["192.0.2.1:51820","[2001:db8::1]:51820"]}`. Respuesta `{"ttl_secs":60,"observed_ip":"192.0.2.10"}`; TTL puede ser menor por el lease. Verifica cadena sin antirrollback; device de cabecera debe coincidir. Sustituye presencia y vacía eventos anteriores.
- `POST /v1/rendezvous`: `{"peer":"<32 caracteres hex>"}`. Respuesta `{"peer":"<hex>","candidates":["192.0.2.1:51820"]}`. Solo misma red y listas de pares mutuas. Encola al destino `{"kind":"connect","peer":"<hex del solicitante>","candidates":["..."],"seq":1}`. Secuencia global creciente del proceso.
- `POST /v1/relay`: `{"peer":"<32 caracteres hex>"}` firmado igual que rendezvous. Respuesta `{"token":"<base64 estándar de 173 bytes>","relays":[{"name":"lab","udp":"relay.example:443","tls":"relay.example:443","tls_name":"relay.example"}],"ttl_secs":300}`. Autoriza mutuamente en la misma red; emite un token solicitante→par y encola al par `{"kind":"relay","peer":"<hex solicitante>","token":"<base64 de token par→solicitante>","relays":[...],"seq":2}`. Ambos tokens contienen la identidad Ed25519 de su origen registrada en la presencia vigente, ventana `ahora..ahora+ttl` e identificadores aleatorios independientes. Los eventos relay y connect comparten cola, secuencia y caducidad.
- `GET /v1/events?wait=25`: espera 0–30 s (25 por defecto), respuesta `{"events":[{"kind":"connect","peer":"<hex>","candidates":["..."],"seq":1}]}`. Entrega y vacía cola; timeout devuelve `{"events":[]}`. Cancelar la petición libera su permiso inmediatamente.
- `GET /healthz`: público, `{"status":"ok","presence":2}` cuenta solo presencias vigentes, sin identificadores.

Errores JSON uniformes: `{"error":{"code":"invalid_signature","message":"invalid_signature"}}`. Códigos: 401 `invalid_signature`; 403 `invalid_config`; 404 `peer_unavailable` (inexistente, unilateral, otra red o desconectado indistinguibles); 409 `not_announced`; 429 `rate_limited`; 400 `invalid_request`; 413 `body_too_large`; rutas/métodos desconocidos 404 `not_found`/405 `method_not_allowed`. El router de biblioteca requiere `ConnectInfo<SocketAddr>` para announce, como lo configura el binario; su ausencia produce 500 `missing_remote_address`.

**Precisión del contrato:** sin presencia no existe clave para autenticar rendezvous/events. Si la cabecera tiene formato y tiempo válidos pero no hay presencia se devuelve 409; no se puede afirmar la validez de su firma. Cabeceras malformadas/fuera de ventana y firmas incorrectas con presencia dan 401. No hay red local fijada: announce admite cualquier red firmada por las raíces configuradas; redes distintas solo son incompatibles en rendezvous.

STUN acepta únicamente Binding request bien formado y responde XOR-MAPPED-ADDRESS (32 bytes IPv4, 44 IPv6); basura, respuestas y paquetes malformados no reciben respuesta. Nunca transporta tráfico de túnel. Cerrar el proceso elimina todos los registros; no hay persistencia, clustering, TLS integrado ni descubrimiento externo.

## Configurar relay

```sh
chmod 600 /ruta/relay-signing.hex
export ATENEA_COORD_RELAY_SIGNING_KEY_FILE=/ruta/relay-signing.hex
export ATENEA_COORD_RELAYS='[{"name":"principal","udp":"relay.example:443","tls":"relay.example:443","tls_name":"relay.example"}]'
atenea-coordinator --print-relay-key
```

La semilla debe generarse de forma segura y ser distinta de claves raíz, emisor y dispositivos. `--print-relay-key` imprime únicamente su clave pública hex y sale sin abrir sockets; entregar esa clave **pública**, nunca la semilla, al operador del relay para su lista `ATENEA_RELAY_COORDINATOR_KEYS`. El fichero TOML también permite `[[relays]]`; los endpoints admiten DNS, IPv4 e IPv6 entre corchetes y puertos 1–65535.

Sin clave o sin relays, las peticiones autenticadas a `/v1/relay` reciben 503 `relay_unavailable`; el resto del servicio sigue funcionando. Clave configurada inválida o RELAYS inválido impiden arrancar. Relay tiene tablas independientes de rendezvous: ventanas móviles de una petición por par no dirigido cada 5 s y 10 intentos/minuto por dispositivo. El máximo de **cuatro tokens por par y minuto equivale a dos peticiones exitosas**, porque cada petición emite dos tokens; se comparte entre ambas direcciones. Los límites devuelven 429 `rate_limited` y las tablas llenas rechazan nuevas entradas. Sin presencia se conserva 409; autorización unilateral, otra red y par ausente conservan el mismo 404 que rendezvous. No se altera el intervalo configurable de rendezvous (1 s por defecto).

## Verificación (§7)

- `inspection_existing_vectors` (identity): válido, todos los negativos sin estado, aceptación explícita de errores antirrollback/reloj, identidad ajena estructuralmente válida, búsqueda entre múltiples anclas y raíz desconocida. Excepción del fixture existente `neg_wrong_device.bin`: además de cambiar el dispositivo deja un selector que referencia al dispositivo antiguo; se rechaza por `InvalidStructure` (paso 14), no por identidad local. Se conserva el vector original intacto y se prueba por separado la aceptación sin paso 10.
- `valid_config_replacement_expiry_and_sweeper`: anuncio válido, saneamiento/deduplicación, reemplazo, TTL y límite del lease exacto con reloj inyectado.
- `configurations_and_authentication_rejected_uniformly`: raíz distinta, configuración caducada o con firma inválida, device distinto, firma de petición inválida, desfase, cabecera inválida, nonce repetido.
- `mutual_authorization_and_uniform_unavailability`: mutuo válido, unilateral, no autorizado, ausente y red distinta con idéntico 404; 409 sin presencia.
- `long_poll_delivery_bounded_queue_and_event_expiry`: long-poll que despierta por evento, secuencia, máximo 16, descarte antiguo, caducidad a 30 s.
- `rate_presence_nonce_body_and_poll_limits`: límites announce/par/dispositivo, tabla de presencia, nonces y cuerpo.
- `cancelled_poll_releases_capacity_and_sweeper_runs`: cupo de polls, cancelación y barrido periódico.
- `bounded_and_refilled`: tabla de cubos acotada y reposición.
- `udp_binding_garbage_and_shared_rate_limit`: sockets UDP reales loopback, XOR-MAPPED-ADDRESS, basura ignorada, amplificación y límite por IP compartido entre puertos.
- `relay_tokens_inverse_event_and_independent_limits`: verifica firma y 173 bytes, red/origen/destino/identidad/ventana/TTL, token inverso y secuencia, separación de rendezvous, límite de 5 s compartido y cuatro tokens/minuto.
- `relay_authorization_matches_rendezvous_and_missing_configuration`: igualdad de respuestas unilateral/no autorizado/otra red/ausente/sin presencia, autenticación requerida, 503 sin clave y/o relays sin afectar rendezvous.
- `relay_device_limit_and_shared_bounded_expiring_queue`: 10/min por dispositivo, cola mixta de 16 con descarte antiguo y caducidad de relay a 30 s.
- `relay_config_validation_key_permissions_and_print_command`: configuración y endpoints inválidos rechazados antes de escuchar, TTL 0 y >600, clave malformada/permisos abiertos, lectura JSON de RELAYS y salida pública de `--print-relay-key`.

```sh
cargo test -p atenea-identity -p atenea-coordinator
cargo clippy -p atenea-identity -p atenea-coordinator --all-targets -- -D warnings
```
