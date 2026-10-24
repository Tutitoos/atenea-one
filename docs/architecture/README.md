# Decisiones de arquitectura (ADR)

| ADR | Decisión | Estado |
| --- | --- | --- |
| [001](ADR-001-noise-ik.md) | Patrón de handshake: Noise IK | Aceptado |
| [002](ADR-002-revocacion-lease.md) | Revocación mediante leases de configuración | Aceptado |
| [003](ADR-003-cifrado-antirreplay.md) | Transporte cifrado con snow, antirreplay y rekey propios | Aceptado |
| [004](ADR-004-entrada-443.md) | Entrada TCP 443 compartida entre panel y relay | Propuesto (antes de la fase 5) |
| [005](ADR-005-custodia-clave-raiz.md) | Custodia de la clave raíz y emisores delegados | Aceptado |
| 006 | Licencia (Apache-2.0 aplicada en el primer commit) | Pendiente de ratificar |

Contratos: [Configuración firmada v1](../protocol/signed-config-v1.md) (propuesto).

"Propuesto" = recomendación redactada, sin aceptar. Un ADR pasa a "Aceptado" cuando se registra la decisión del responsable; nada que dependa de él se implementa antes.
