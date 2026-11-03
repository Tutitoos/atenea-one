# Modelo de datos de Atenea One (referencia)

Fuente: `docs/api/control-plane-v1.md` §10.1 y `docs/api/openapi.json`. Ejemplos sintéticos.

## `Device` (`GET /api/v1/networks/{net}/devices`)

| Campo | Significado | Origen |
| --- | --- | --- |
| `id` / `device_id` | UUID del dispositivo (iguales) | Control plane |
| `role` | `device` o `server` | Declarado (ADR-014) |
| `name` | Nombre mostrado; `name_source` = `admin` o `device` | Declarado |
| `hostname` | Nombre del sistema, o `null` | Declarado |
| `status` | `pending`, `approved` o `revoked` | Control plane (decisión de un administrador) |
| `join_origin` | Cómo se unió (`token`, `request`…) | Control plane |
| `ipv4`, `ipv6` | Direcciones del túnel; `null` si no está aprobado (p. ej. `10.77.0.2`) | Control plane |
| `endpoints` | Lista `ip:puerto` indicada por el administrador | Administrador |
| `fingerprint` | Huella de la clave; distingue nombres duplicados | Control plane |
| `enrolled_at`, `approved_at`, `revoked_at`, `last_config_at` | RFC 3339 o `null` | Control plane |
| `connection` | `online`, `stale`, `offline` o `unknown` (ver abajo) | Calculado por el servidor a partir de un informe declarado |
| `report` | Informe del agente, o `null` si nunca informó | Declarado |

## `report`

| Campo | Significado |
| --- | --- |
| `reported_at` | Cuándo informó el agente |
| `age_secs` | Antigüedad en segundos, según el reloj del servidor |
| `nat_mapping` | `independent`, `dependent`, `unknown` o `null` |
| `paths[].peer` | UUID del par |
| `paths[].path` | `direct`, `relay-udp`, `relay-tls`, `probing` o `none` |
| `paths[].last_rx_secs`, `rtt_ms`, `relay` | Segundos desde el último paquete, latencia, nombre del relay; pueden ser `null` |

## `connection`

| Valor | Condición |
| --- | --- |
| `online` | informe de menos de 90 s |
| `stale` | menos de 10 min |
| `offline` | más de 10 min |
| `unknown` | sin informe |

## Otros recursos

- Red: `ipv4_cidr` (p. ej. `10.77.0.0/24`), `ipv6_cidr`, `join_policy` (`cerrada` o `con_solicitud`), `has_issuer_cert`.
- Ruta: `prefix`, `status` (`pending`, `approved`, `rejected`), `snat`, `device_id`.
- Política: `src`/`dst` (`any`, `device`, `prefix`), `proto`, `port_min`–`port_max`.
- DNS: `servers` (hasta 3) y `domains` (hasta 8) de la red.
