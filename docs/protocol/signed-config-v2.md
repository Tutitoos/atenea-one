# Contrato de configuración firmada v2 (fase 7): rutas de subred y DNS

- **Estado:** Aceptado (2026-10-25, por el responsable; implementado y verificado en laboratorio, fase 7). Sin revisión criptográfica externa.
- **Basado en:** `signed-config-v1.md` (todo lo no mencionado aquí no cambia), ADR-007.
- **Alcance:** solo el cuerpo `DeviceConfig`. `IssuerCert`, `DeviceBundle` y el envoltorio `SignedConfig` (`magic`, `version` = 1) no cambian.

## 1. Compatibilidad y versión mínima

- El cuerpo pasa a poder llevar `version` = 1 o 2. **El emisor emite `version = 1` (bytes idénticos a los de hoy) cuando la configuración no tiene rutas ni DNS, y `version = 2` solo cuando hace falta.** Un agente solo-v1 sigue funcionando mientras una red no use subredes ni DNS.
- El agente nuevo acepta 1 y 2. Codificación única: un cuerpo `version = 2` cuyos campos nuevos estén todos vacíos es `Malformed` (debería ser v1); un cuerpo v1 con bytes sobrantes sigue siéndolo.
- Una versión desconocida (≠ 1, 2) es `UnsupportedVersion`, como antes. La firma, dominio de firma y pasos 1–13 de verificación no cambian.

## 2. Estructuras nuevas (solo en `version = 2`)

Tipos de v1 (`Addr`, `Prefix`, listas ordenadas ascendentes por su codificación y sin duplicados) sin cambios.

**Peer v2:** `device_id [16]`, `noise_pubkey [32]`, `flags u8 = 0`, `addrs vec<Addr>(u8) 1..4`, `endpoints vec<Endpoint>(u8) 0..4`, **`routes vec<Prefix>(u8) 0..8`** (subredes que se alcanzan a través de ese par).

**Cuerpo v2:** los campos de v1 (con `Peer` v2) y, tras `rules`:

| Campo | Tipo | Regla |
| --- | --- | --- |
| `advertised` | `vec<AdvRoute>(u8)` 0..8 | Subredes que **este dispositivo** sirve (aprobadas). Ordenadas por `prefix` |
| `dns_servers` | `vec<Addr>(u8)` 0..3 | Ordenadas |
| `dns_domains` | `vec<Domain>(u8)` 0..8 | Ordenados por bytes ASCII, sin duplicados |

`AdvRoute` = `Prefix` + `flags u8` (bit 0 = `snat`; el resto debe ser 0). `Domain` = `len u8` (1..=253) + `len` bytes ASCII en minúsculas, etiquetas LDH de 1..63 caracteres separadas por `.`, sin punto final, sin `.` solo ni etiqueta vacía.

## 3. Reglas estructurales adicionales (paso 14, `InvalidStructure`)

1. Todo prefijo de `routes` y `advertised` es **privado** (RFC 1918, `100.64.0.0/10`, ULA `fc00::/7`) con longitud ≥ 8 (IPv4) o ≥ 32 (IPv6); nunca por defecto, loopback, enlace local, multicast ni documentación.
2. Ningún prefijo de `routes`/`advertised` se solapa (contención o igualdad) con otro de `routes` (de cualquier par), con `advertised` ni con `network_prefixes`.
3. `dns_servers`: cada dirección está dentro de `network_prefixes`, de alguna `routes` o de `advertised`.
4. Un selector de regla `Prefix` puede estar contenido en `network_prefixes`, en una `routes` o en `advertised` (en v1 solo en `network_prefixes`).
5. Un dispositivo con `advertised` no vacío no puede tener `snat` distinto de lo firmado: el agente aplica exactamente el bit firmado.

## 4. Plano de datos (sustituye a los puntos 1–2 de «Durante la operación»)

1. **Entrada** (desde un par): origen en `addrs` o `routes` de **ese** par; destino una dirección propia o dentro de `advertised`. En otro caso se descarta (nunca se reenvía a otro par).
2. **Salida** (desde la TUN): origen propio o dentro de `advertised` (tráfico de vuelta); el par destino es el de la dirección exacta o, si no hay, el del **prefijo más largo** de `routes`. Sin coincidencia: descarte.
3. ACL en ambos extremos con los selectores de la configuración local, sin cambios.

## 5. Vectores y pruebas exigidas

Un vector válido v2 y uno negativo por regla nueva en `tests/fixtures/identity/`; ida y vuelta; cuerpo v2 vacío (`Malformed`); v1 válido de la fase 3 sigue aceptándose byte a byte; solapes; rutas públicas; `0.0.0.0/0` y `::/0`; prefijos demasiado cortos; dominios inválidos (`.`, mayúsculas, etiqueta > 63, guiones al borde); orden y duplicados; fuzz del parser v2 y de `Domain`.

## 6. Tipos Rust (normativos para los componentes)

```rust
// crates/identity
pub struct CfgPeer { /* …campos actuales… */ pub routes: Vec<Prefix> }
pub struct AdvertisedRoute { pub prefix: Prefix, pub snat: bool }
pub struct DnsConfig { pub servers: Vec<IpAddr>, pub domains: Vec<String> }
pub struct DeviceConfig { /* …campos actuales… */ pub advertised: Vec<AdvertisedRoute>, pub dns: DnsConfig }
```
`DeviceConfig::encode()` elige la versión según §1; `parse` acepta ambas.

## Resoluciones de implementación

- Validación reutilizable en `atenea-identity`: `Prefix::valid_route()` exige que el prefijo canónico completo esté contenido en uno de los rangos privados autorizados, no solo que su primera dirección sea privada; `contains_prefix()`, `overlaps()` y `contains()` comparten la aritmética de prefijos. `valid_domain()` valida los dominios ASCII LDH. Los errores de estas relaciones son `InvalidStructure`; orden, contadores, dominios y flags inválidos son `Malformed`.
- `atenea-policies` expone `peer_route_contains()`, `advertised_contains()` y `peer_for_destination()`. La búsqueda prioriza direcciones exactas y después el prefijo más largo; los selectores de dispositivo conservan su significado v1 (direcciones exactas), y los de prefijo permiten explícitamente las subredes.
- `atenea issue` acepta `routes` en cada `[[peer]]`, `[[advertise]]` con `prefix` y `snat` obligatorio, y `[dns]` con `servers` y `domains`. Las colecciones ausentes son vacías; no se normalizan dominios inválidos ni se infiere SNAT.
- Una lista de dominios sin servidores, o servidores sin dominios, activa v2: el contrato no exige que ambas listas estén presentes. El agente conserva el valor SNAT firmado; no existe un segundo valor local que el verificador pueda comparar.
- Vectores `v2_valid.bin` y `v2_invalid_*.bin` contienen el fichero firmado completo; `v2_malformed_*.bin` contienen cuerpos para ejercitar el parser estricto. Los vectores v1 permanecen idénticos. El objetivo `config_v2_parse` comprueba ida y vuelta y también dominios sobre los bytes de entrada.
