# Contrato de configuración firmada v1

- **Estado:** Aceptado (2026-10-24, por el responsable del proyecto). Sigue sin implementación ni vectores (§6).
- **Fecha:** 2026-10-24
- **Ámbito:** MVP-0 (fase 3) y base de la fase 3B. Los relays, coordinator y rutas de subred no existen en v1 (ver "Evolución").
- **Basado en:** ADR-001 (claves X25519 de peers), ADR-002 (lease, rollback, reloj), ADR-005 (cadena raíz → emisor → configuración).

Este documento define el formato binario, las reglas de verificación, el estado persistente del agente y los errores. Un cambio incompatible exige subir `version`.

## 1. Convenciones

- Enteros sin signo en **big-endian**. Tiempos: segundos Unix UTC en `u64`.
- `[N]` = N bytes fijos. `vec<T>` = contador (`u8` o `u16`, indicado) seguido de los elementos.
- Todos los campos reservados (`flags`) deben valer 0; otro valor = `Malformed`.
- **Codificación única:** cada estructura admite exactamente una representación. El parser es estricto: sin bytes sobrantes, listas **ordenadas** y sin duplicados (ver orden en cada lista). Re-codificar lo parseado debe dar bytes idénticos.
- Firma: Ed25519 con verificación estricta. Todo mensaje firmado va precedido de una cadena de dominio distinta con terminador `0x00` que no se transmite.
- Identificador de clave: `key_id = BLAKE2s-256(clave_pública)[0..8]`.
- Identificadores `network_id` y `device_id`: 16 bytes (UUID v7 en forma binaria), generados por el emisor (red) o por el dispositivo (dispositivo).

### Tipos compuestos

| Tipo | Codificación |
| --- | --- |
| `Addr` | `family(u8: 4 \| 6)` + `addr([4] \| [16])` |
| `Prefix` | `Addr` + `len(u8)`; `len` ≤ 32 o ≤ 128; los bits de host deben ser 0 |
| `Endpoint` | `Addr` + `port(u16)`, solo UDP; `port` ≠ 0 |

Orden de listas de `Addr`, `Prefix` y `Endpoint`: ascendente por los bytes de su codificación.

## 2. Mensajes

### 2.1 Paquete público de dispositivo (`DeviceBundle`, 146 bytes)

Lo genera el dispositivo (`keygen`); no contiene secretos.

| Campo | Tamaño |
| --- | --- |
| `version` = 1 | u8 |
| `kind` = 3 | u8 |
| `device_id` | [16] |
| `noise_pubkey` (X25519) | [32] |
| `identity_pubkey` (Ed25519) | [32] |
| `proof` | [64] |

`proof = Ed25519(identity_sk, "atenea-one/device-bundle/v1" ‖ bytes[0..82])`. Demuestra posesión de la clave de identidad y enlaza ambas claves públicas; no autentica al dispositivo ante el emisor. El emisor **debe verificar la huella por otro canal** antes de firmar: `BLAKE2s-256(bundle)[0..16]` en hexadecimal agrupado. No hay enrollment automático en MVP-0.

### 2.2 Certificado de emisor (`IssuerCert`, 178 bytes)

Firmado por la clave raíz (ADR-005).

| Campo | Tamaño |
| --- | --- |
| `version` = 1 | u8 |
| `kind` = 1 | u8 |
| `root_key_id` | [8] |
| `network_id` | [16] |
| `issuer_serial` | u64 |
| `not_before` | u64 |
| `not_after` | u64 |
| `issuer_pubkey` (Ed25519) | [32] |
| `next_anchor` (Ed25519 de una raíz futura) | [32] |
| `signature` | [64] |

`signature = Ed25519(root_sk, "atenea-one/issuer-cert/v1" ‖ bytes[0..114])`. `next_anchor` todo ceros = sin ancla futura (una clave Ed25519 de ceros no es válida como ancla).

### 2.3 Configuración de dispositivo (`DeviceConfig`, cuerpo)

Firmada por el emisor; **una por dispositivo**.

| Campo | Tipo | Regla |
| --- | --- | --- |
| `version` | u8 | 1 |
| `kind` | u8 | 2 |
| `flags` | u8 | 0 |
| `network_id` | [16] | = certificado |
| `device_id` | [16] | |
| `issuer_serial` | u64 | = certificado |
| `config_version` | u64 | Monótona por (`network_id`, `device_id`), > 0 |
| `not_before`, `not_after` | u64, u64 | `not_after − not_before` ≤ 3600 s |
| `noise_pubkey` | [32] | X25519 del dispositivo (ADR-001) |
| `identity_pubkey` | [32] | Ed25519 del dispositivo |
| `mtu` | u16 | 1280 ≤ mtu ≤ 1500 (por defecto 1400) |
| `tunnel_addrs` | vec<Addr> (u8) | 1 a 4; direcciones propias en el túnel |
| `network_prefixes` | vec<Prefix> (u8) | 1 a 4; toda dirección de la configuración debe caer dentro de alguno |
| `peers` | vec<Peer> (u16) | 0 a 256; ordenados por `device_id` |
| `rules` | vec<Rule> (u16) | 0 a 256; ordenadas por su codificación; sin duplicados |

**Peer:** `device_id [16]`, `noise_pubkey [32]`, `flags u8 = 0`, `addrs vec<Addr> (u8) 1..4`, `endpoints vec<Endpoint> (u8) 0..4`.
Los `endpoints` son **pistas no autenticadas**: no demuestran alcanzabilidad ni autorizan tráfico (fase 4).

**Rule** (solo permisos; todo lo no permitido se deniega):

| Campo | Tipo |
| --- | --- |
| `src` | `Selector` |
| `dst` | `Selector` |
| `proto` | u8: 0 = cualquiera, 1 = ICMP, 6 = TCP, 17 = UDP, 58 = ICMPv6 |
| `port_min`, `port_max` | u16, u16 |

`Selector`: `kind(u8)` = 1 cualquiera (sin datos), 2 dispositivo (`device_id [16]`: él mismo o un peer), 3 prefijo (`Prefix`). Puertos: solo con `proto` 6 o 17, con `port_min ≤ port_max`; `(0,0)` = cualquier puerto; para otros `proto` deben ser `(0,0)`.

### 2.4 Fichero (`SignedConfig`)

| Campo | Tamaño |
| --- | --- |
| `magic` = `"AOCF"` | [4] |
| `version` = 1 | u8 |
| `cert` | [178] (`IssuerCert`) |
| `body_len` | u32 (≤ 131072) |
| `body` | [`body_len`] (`DeviceConfig`) |
| `signature` | [64] |

`signature = Ed25519(issuer_sk, "atenea-one/config/v1" ‖ body)`.

## 3. Verificación

El agente ejecuta estos pasos **en orden**; el primer fallo rechaza el fichero entero con el error indicado y no modifica el estado. No se aplica nada parcialmente.

| # | Comprobación | Error |
| --- | --- | --- |
| 1 | Tamaño de fichero y `body_len` dentro de límites | `TooLarge` |
| 2 | `magic`, `version`, `kind` conocidos | `UnsupportedVersion` |
| 3 | Parseo estricto (orden, duplicados, reservados, rangos, sin sobrantes) | `Malformed` |
| 4 | `cert.root_key_id` coincide con alguno de los ≤ 2 anclas locales | `UnknownRoot` |
| 5 | Firma de la raíz sobre el certificado | `BadCertSignature` |
| 6 | Ventana del certificado: `not_before − 30 ≤ now < not_after + 30`; `not_after − not_before` ≤ 90 días | `CertExpired` / `CertNotYetValid` |
| 7 | `cert.issuer_serial` ≥ mayor serial persistido | `IssuerRollback` |
| 8 | Firma del emisor sobre el cuerpo | `BadConfigSignature` |
| 9 | `body.network_id` = `cert.network_id`, `body.issuer_serial` = `cert.issuer_serial`, y `network_id` = el de este agente | `ScopeMismatch` |
| 10 | `body.device_id`, `noise_pubkey` e `identity_pubkey` = las claves locales del dispositivo | `WrongDevice` |
| 11 | `config_version` > mayor persistida; igual y fichero idéntico = no-op | `ConfigRollback` |
| 12 | Ventana de la configuración con tolerancia de 30 s | `Expired` / `NotYetValid` |
| 13 | Anomalía de reloj: `now ≥ max_time_observed − 30` | `ClockAnomaly` |
| 14 | Reglas estructurales: direcciones dentro de `network_prefixes`; sin solapes entre direcciones de peers ni con las propias; ningún peer con las claves o el `device_id` propios; claves de peers únicas; selectores `device` que referencian a uno mismo o a un peer | `InvalidStructure` |

Si la verificación pasa:

1. Se **persiste** el estado nuevo (escritura atómica: fichero temporal + `fsync` + `rename` + `fsync` del directorio) **antes** de aplicarlo.
2. Se aplica: destruir sesiones con peers eliminados o cuya clave cambió (ADR-002), cargar ACL, direcciones y peers.
3. Si `cert.next_anchor` no es cero se añade como segundo ancla (máximo 2). Si el `root_key_id` del certificado que verificó este fichero corresponde al ancla más reciente, el ancla anterior se elimina.

Si la persistencia falla, no se aplica y se mantiene la configuración anterior.

### Estado persistente (0600, directorio de estado del servicio)

`network_id`, `device_id`, anclas de confianza (≤ 2), `max_config_version`, `max_issuer_serial`, `max_time_observed`. Este último se actualiza al cargar y como mucho una vez por minuto. La ausencia de estado con anclas válidas equivale a primer arranque; la corrupción del estado = denegar y exigir reinicialización explícita (no se recrea en silencio).

### Durante la operación

- **Lease:** si no hay configuración vigente (`now ≥ not_after + 30`) se deniega todo y se destruyen sesiones (ADR-002). Recarga al detectar un fichero nuevo y como mínimo cada 10 s.
- **Plano de datos (por paquete IP dentro del túnel):**
  1. **Origen:** la IP origen del paquete interno debe estar en `addrs` del peer del que llegó (o en `tunnel_addrs` para tráfico saliente local); si no, se descarta.
  2. **Destino:** debe ser una dirección propia (v1 no reenvía ni enruta subredes).
  3. **ACL:** se evalúa en origen (salida) **y** en destino (entrada) con la configuración local de cada uno; debe haber alguna regla que permita (`src`, `dst`, `proto`, puertos). Sin coincidencia = descarte.
  4. Se admite ICMP/ICMPv6 de error (por ejemplo, tamaño excesivo) solo si una regla lo permite; el agente genera localmente los "paquete demasiado grande".

## 4. Errores y registro

Los códigos de la tabla de verificación son estables y se exponen por IPC y en el registro con el `network_id`, `config_version` y el motivo. **No** se registran claves ni el contenido de la configuración. Un rechazo no revoca la configuración vigente (sigue hasta su lease); cada rechazo se cuenta en una métrica.

## 5. Evolución

- v1 solo acepta `version = 1`. Una versión posterior aceptará la vigente y la anterior durante una ventana de migración; versión o `kind` desconocidos se rechazan, nunca se ignoran.
- Las siguientes estructuras se añaden con `version = 2`: relays y regiones (fase 5), coordinator y endpoints autenticados (fase 4), rutas de subred y DNS (fase 7), L2 (fase 10). No existen en v1.
- La codificación estricta evita ambigüedad entre implementaciones; no se admiten campos opcionales no declarados.

## 6. Verificación del contrato

La implementación de referencia (fase 3) debe incluir, con claves sintéticas en `tests/fixtures/`:

- Un vector válido de cada mensaje y vectores negativos, uno por error de la tabla de §3.
- Prueba de ida y vuelta (parsear → codificar = bytes originales) y rechazo de cada variación de orden, duplicado, reservado y sobrante.
- Fuzzing del parser de `SignedConfig` y de `DeviceBundle`.
- Prueba de que una firma de un tipo no valida como otro (dominios distintos).
- Pruebas de rollback (`config_version`, `issuer_serial`), reloj y reinicio con estado persistido; fallo de persistencia sin aplicación parcial.

Hasta que existan, el contrato no está verificado: es una especificación escrita, sin implementación ni vectores.

## 7. Puntos para la revisión externa

- La cabecera `version`/`kind` del cuerpo y el fichero no están firmadas por separado: el cuerpo las incluye y la firma lo cubre; `magic`/`version` del fichero solo despachan.
- El `proof` del `DeviceBundle` no prueba posesión de la clave X25519; el enlace real es la comprobación de huella del emisor.
- `next_anchor` va en el certificado, firmado por la raíz (no por el emisor): un emisor comprometido no puede instalar una raíz atacante. Revisar que la lógica de rotación de anclas no abra un camino de degradación.
