# Relay de Atenea One

Reenvía únicamente datagramas cifrados entre asignaciones autorizadas y emparejadas.
No termina Noise, no recibe claves de sesión y no elige destinos suministrados por el cliente.

## Arranque

Construir con `cargo build -p atenea-relay --release`. Preparar una clave de firma
exclusiva para relay en el coordinator (no reutilizar claves de identidad/anclas):
`atenea-coordinator --print-relay-key` imprime su clave pública, que se configura
como `COORDINATOR_KEYS` en este proceso. La semilla privada permanece en el coordinator.

En el equipo del coordinator, con el directorio `/etc/atenea` preparado:

```sh
umask 077
openssl rand -hex 32 > /etc/atenea/relay-signing-key.hex
ATENEA_COORD_RELAY_SIGNING_KEY_FILE=/etc/atenea/relay-signing-key.hex \
  atenea-coordinator --print-relay-key
```

Configurar esa misma ruta en el servicio del coordinator y su lista `RELAYS`
con las entradas públicas UDP/TLS y `tls_name` del certificado.

Preparar certificado de servidor y clave privada PEM para el nombre público del
relay, emitidos por una CA confiable para los agentes. Para laboratorio se puede
usar una CA privada y configurar `relay_ca_file` en el agente. Restringir la clave
privada a modo 0600. No introducir claves ni certificados en Git; las pruebas
usan rcgen y material efímero, nunca certificados incorporados al repositorio.

Para un certificado efímero de laboratorio con OpenSSL ya instalado:

```sh
umask 077
openssl req -x509 -newkey ed25519 -nodes -days 1 \
  -keyout relay-key.pem -out relay-cert.pem \
  -subj /CN=localhost -addext 'subjectAltName=DNS:localhost'
```

Configurar `relay_ca_file` con ese certificado únicamente en clientes del
laboratorio; usar una CA y un certificado de servidor normales en producción.

Ejemplo de TOML local (rutas y clave pública sintéticas):

```toml
udp_listen = "0.0.0.0:443"
tcp_listen = "127.0.0.1:8443"
coordinator_keys = ["<clave-pública-Ed25519-hex-de-64-caracteres>"]
tls_cert_file = "/etc/atenea/relay-cert.pem"
tls_key_file = "/etc/atenea/relay-key.pem"
status_socket = "/run/atenea/relay.sock"
proxy_protocol = true
```

`ATENEA_RELAY_CONFIG=/etc/atenea/relay.toml atenea-relay` carga valores por defecto,
TOML y entorno, en ese orden. Crear previamente el directorio del socket de estado
con permisos de acceso apropiados. Una ruta de socket preexistente impide arrancar:
no se borra automáticamente un socket de otro proceso. SIGINT/SIGTERM detienen
listeners y conexiones y retiran el socket propio. Tras una caída abrupta, comprobar
que no existe otro proceso antes de retirar manualmente el socket residual.
`--print-config-example` imprime todos los valores por defecto en JSON (inspección;
para cargar un fichero se usa TOML).

UDP 443 es la entrada directa; HAProxy termina el encaminamiento TCP, **no TLS**,
y usa `send-proxy-v2` hacia TCP loopback 8443. También se puede escuchar TCP/TLS
directamente desactivando PROXY. Solo TLS 1.3, ALPN `atenea-relay/1`, sin 0-RTT.
PROXY exige loopback, comando PROXY y familia TCP4/TCP6; rechaza LOCAL, UNSPEC y
extensiones TLV. El plazo de handshake incluye la lectura de PROXY. La IP de esa
cabecera se usa para límites de asignación, tasa y conexiones. El límite global de
conexiones incluye sockets que todavía no han completado PROXY/TLS.

## Variables y límites

Todas tienen el prefijo `ATENEA_RELAY_`; el nombre del campo TOML es el sufijo en
minúsculas. Listas de entorno usan sintaxis Figment, por ejemplo
`ATENEA_RELAY_COORDINATOR_KEYS='["<hex>"]'`.

| Sufijo | Por defecto | Significado |
| --- | --- | --- |
| `CONFIG` | sin fichero | TOML opcional |
| `UDP_LISTEN` | `0.0.0.0:443` | Socket UDP |
| `TCP_LISTEN` | `127.0.0.1:8443` | Socket TCP |
| `COORDINATOR_KEYS` | `[]` (obligatorio) | 1–32 claves públicas Ed25519 hex, no débiles |
| `TLS_CERT_FILE`, `TLS_KEY_FILE` | vacíos (obligatorios) | PEM del servidor |
| `PROXY_PROTOCOL` | `false` | Cabecera v2 obligatoria cuando se activa; listener loopback |
| `STATUS_SOCKET` | `/tmp/atenea-relay.sock` | Socket Unix de lectura |
| `MAX_ALLOCATIONS` | 4096 | Total de asignaciones, máximo 1.000.000 |
| `MAX_PER_NETWORK` | 1024 | Por red, no mayor que el total |
| `MAX_PER_IP` | 32 | Por IP de vinculación, no mayor que el total |
| `PACKETS_PER_SECOND` | 2000 | Cubo por asignación |
| `BITS_PER_SECOND` | 24000000 | Cubo por asignación, mínimo 8; precisión de un byte |
| `COOKIE_PER_SECOND` | 20 | Cubo COOKIE_REQ por IP |
| `ALLOC_PER_SECOND` | 10 | Cubo ALLOC por IP |
| `MAX_DATAGRAM` | 1530 | Tamaño máximo de carga DATA, no mayor que 1530 |
| `TCP_QUEUE` | 256 | Mensajes pendientes por conexión, máximo 65536; descarta el nuevo |
| `IDLE_SECS` | 90 | Inactividad de lectura TCP, máximo 86400 |
| `TCP_PER_IP` | 8 | Conexiones por IP, no mayor que TCP_TOTAL |
| `TCP_TOTAL` | 2048 | Conexiones totales, máximo 65536 |
| `HANDSHAKE_SECS` | 10 | Plazo PROXY+TLS, máximo 300 |
| `ALLOC_SECS` | 5 | Primera asignación válida tras TLS, máximo 300 |
| `MAX_RATE_ENTRIES` | 65536 | Entradas IP/tipo de petición, máximo 1.000.000 |

Todos los límites son positivos. Cubos con ráfaga de un segundo; la renovación
no reinicia los cubos de DATA. Tablas de tasas se limpian tras 120 s sin uso. El
barrido de asignaciones es cada 5 s; la vinculación deja de servir tráfico en su
instante de expiración aunque el barrido todavía no haya pasado. Las renovaciones
mantienen alloc_id y sustituyen la vinculación anterior; el tráfico no prolonga
la vida autorizada. Solo un ALLOC nuevo puede renovarla (máximo 120 s y nunca más
allá de not_after+30). Las lecturas TCP parciales están acotadas a una trama y un
bloque de lectura de 2048 bytes. Longitudes 0 y >2048 cierran la conexión.

## Estado

Cada conexión al socket Unix obtiene un objeto JSON seguido de salto de línea y
EOF. Proteger el directorio del socket para limitar quién puede leer metadatos.
No incluye IPs, identidades, tokens, cookies, firmas ni cargas.

```json
{
  "allocations": 2,
  "bound_udp": 1,
  "bound_tcp": 1,
  "tcp_connections": 1,
  "traffic": {
    "forwarded_packets": 2,
    "forwarded_bytes": 80,
    "udp_packets": 1,
    "udp_bytes": 40,
    "tcp_packets": 1,
    "tcp_bytes": 40,
    "drops": {"no_peer": 1},
    "rejects": {"1": 1}
  }
}
```

Todos los contadores son enteros no negativos. `bound_udp + bound_tcp = allocations`:
toda asignación está vinculada. Los contadores de transporte describen la dirección
de **salida**; bytes son solo datagramas cifrados, sin sobrecarga. Para TCP cuentan
la aceptación en cola (no confirman recepción remota). `drops` omite motivos con
cero eventos; motivos: `malformed`, `cookie`, `cookie_rate`, `unexpected`, `unbound`,
`size`, `allocation_rate`, `no_peer`, `udp_write`, `tcp_queue`, `tcp_closed`,
`tcp_capacity`, `tcp_protocol`. `rejects` usa claves de texto `1`–`8` de ALLOC_ERR
según relay-v1; omite códigos no observados. Rechazos sin cookie no generan respuesta.

## Pruebas y cobertura de §8

`cargo test -p atenea-relay`; `cargo clippy -p atenea-relay --all-targets -- -D warnings`;
`cargo fmt -p atenea-relay`. Todo funciona en loopback, sin privilegios ni Linux.

| Prueba | Cobertura |
| --- | --- |
| `udp_tls_and_mixed_bidirectional` | ALLOC UDP/TLS, ambos sentidos UDP↔UDP/TLS↔TLS/UDP↔TLS, estado Unix |
| `unpaired_unbound_and_rebinding` | Pareja ausente, origen no vinculado, renovación desde otra dirección |
| `invalid_tokens_proofs_cookies_and_time` | Emisor distinto, caducado, estructura mala, posesión mala, ts fuera de ventana, cookie inválida/de otro origen, red incorrecta sin pareja |
| `capacity_rates_size_and_tcp_frames` | Capacidad total/por IP, cookie por segundo, tamaño, longitud TCP 0/>2048 |
| `partial_frames_and_alloc_idle_handshake_deadlines` | Lecturas parciales, plazos handshake/ALLOC/inactividad |
| `proxy_v2_real_ip_and_alpn_rejection` | PROXY loopback, IP real en límite de conexiones, ALPN incorrecto |
| `tls12_and_connection_limits` | TLS 1.2 rechazado, límites TCP global/por IP |
| `expired_cookie_sweep_and_peer_state` | Reloj inyectado: cookie caducada, caducidad, barrido, avisos de pareja |
| `allocation_ip_network_packet_and_byte_limits` | Reloj inyectado: capacidad total/red/IP, ALLOC por segundo, cubos DATA paquetes/bytes |
| `refresh_rebind_pair_and_expire` | Tabla pura: renovación, pareja, caducidad y liberación de índices |
| `refill_and_bounds` | Reloj inyectado: recarga de cubos y tabla de tasas acotada |
| `full_tcp_queue_drops_new_packet` | Cola llena descarta nuevo mensaje y cuenta sin crecer |

El rechazo de PROXY externo se garantiza además por validación de configuración
(listener solo loopback) y comprobación de la dirección del socket aceptado. La
caducidad de cookies y asignaciones usa reloj inyectado en pruebas puras, evitando
esperas de minutos. Laboratorios NAT y regresiones del agente/coordinator pertenecen
a sus respectivos componentes, no a este paquete.
