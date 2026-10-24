# Inventario de dependencias y licencias (fase 0)

- **Fecha de consulta:** 2026-10-24 (crates.io y npm). Son las últimas versiones estables **observadas**; no se ha compilado ni resuelto ninguna. Las versiones se fijan con el primer `Cargo.lock` / `pnpm-lock.yaml` de cada componente.
- **Licencia del proyecto:** Apache-2.0 (ADR-006, pendiente de ratificar). Todas las licencias de abajo son permisivas y compatibles con su redistribución, salvo lo marcado.

## Rust

| Crate | Versión observada | Licencia | Uso | Notas |
| --- | --- | --- | --- | --- |
| `snow` | 0.10.0 | Apache-2.0 OR MIT | Noise IK, `StatelessTransportState` (ADR-001/003) | Confirmar compilación del patrón y de la conversión `TryFrom<HandshakeState>` |
| `ed25519-dalek` | 3.0.0 | BSD-3-Clause | Firmas (contrato, ADR-005) | Confirmar API de verificación estricta |
| `x25519-dalek` | 3.0.0 | BSD-3-Clause | Claves estáticas de dispositivo | Comprobar compatibilidad con el resolver de `snow` |
| `blake2` | 0.11.0 | MIT OR Apache-2.0 | `key_id` y huellas | Misma familia que el hash del patrón Noise |
| `tokio` | 1.54.1 | MIT | Runtime asíncrono | |
| `axum` | 0.8.9 | MIT | Control plane (fase 3B) | |
| `sqlx` | 0.9.0 | MIT OR Apache-2.0 | PostgreSQL (fase 3B) | |
| `utoipa` | 6.0.0 | MIT OR Apache-2.0 (a confirmar) | OpenAPI (fase 3B) | Licencia no consultada con certeza |
| `rustls` | 0.23.46 (estable) | Apache-2.0 OR ISC OR MIT | TLS del relay (fase 5) | crates.io muestra además una `0.24.0-dev`; usar la estable |
| `clap` | 4.6.7 | MIT OR Apache-2.0 | CLI | |
| `tracing`, `serde` | 0.1.44, 1.0.229 | MIT OR Apache-2.0 | Registro y serialización | |
| `tun` | 0.8.14 | **WTFPL** | Interfaz TUN Linux (fase 2) | Licencia permisiva no estándar: evaluar frente a `tun-rs` u otra, o la llamada directa a `/dev/net/tun`, antes de depender de ella |

## TypeScript / panel (fase 6)

| Paquete | Versión observada | Licencia |
| --- | --- | --- |
| `react-router` | 8.5.0 | MIT |
| `react` | 19.3.1 | MIT |
| `@tanstack/react-query` | 5.104.5 | MIT |
| `zod` | 4.6.7 | MIT |
| `tailwindcss` | 4.3.3 | MIT |
| `recharts` | 3.10.1 | MIT |
| `motion` | 14.6.0 | MIT |

Compatibilidad entre `react-router` 8.5.0, React 19.3.1 y el resto: **no verificada**; se comprueba al crear `apps/web`.

## Pendiente

- Wintun (licencia propia, redistribución; fase 8) y `utoipa`: revisar el texto de la licencia antes de adoptarlos.
- `tun` (WTFPL) **no se adoptó**: `atenea-tunnel` usa `/dev/net/tun` directamente con `libc` (código `unsafe` acotado y comentado).
- Añadir `cargo deny` / auditoría de dependencias (RustSec) al CI. Ya hay un `Cargo.lock` real: queda pendiente.
- **`ring` 0.17.14 llega a través de `snow` 0.10.0** con las características por defecto (visible con `cargo tree -p atenea-crypto -e features -i ring`). Su licencia es propia (mezcla ISC/OpenSSL), distinta de las demás. Hay que leerla y decidir si se acepta o se desactiva en `snow`.
- Resueltas y en uso (versiones del `Cargo.lock`): `snow` 0.10.0, `x25519-dalek` 3.0.0, `ed25519-dalek` 3.0.0, `blake2` 0.11.0 (identity) y 0.10 (dentro de snow), `getrandom` 0.3, `zeroize`, `tokio` 1.54, `clap` 4.6, `serde`/`serde_json`/`toml` 0.9, `tracing`, `proptest` 1, `libc` 0.2. No se han leído las licencias de las dependencias transitivas.
