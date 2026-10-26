# Decisiones de arquitectura (ADR)

| ADR | Decisión | Estado |
| --- | --- | --- |
| [001](ADR-001-noise-ik.md) | Patrón de handshake: Noise IK | Aceptado |
| [002](ADR-002-revocacion-lease.md) | Revocación mediante leases de configuración | Aceptado |
| [003](ADR-003-cifrado-antirreplay.md) | Transporte cifrado con snow, antirreplay y rekey propios | Aceptado |
| [004](ADR-004-entrada-443.md) | Entrada TCP 443 compartida entre panel y relay | Aceptado |
| [005](ADR-005-custodia-clave-raiz.md) | Custodia de la clave raíz y emisores delegados | Aceptado |
| [006](ADR-006-licencia.md) | Licencia del proyecto: Apache-2.0 | Aceptado |
| [007](ADR-007-subredes-dns-privilegios.md) | Subredes, DNS dividido y modelo de privilegios | Aceptado |
| [008](ADR-008-clientes-windows-macos.md) | Clientes macOS y Windows (solo cliente; excepción Wintun) | Aceptado |
| [009](ADR-009-operacion-y-releases.md) | Operación, disponibilidad, secretos y releases | Aceptado |

Contratos: [Configuración firmada v1](../protocol/signed-config-v1.md) (aceptado). Seguridad: [Modelo de amenazas](../security/threat-model.md) (aceptado). [Inventario de dependencias](dependencies.md).

Paquete de entrega para la [auditoría independiente](../audit/README.md) (alcance, arquitectura, evidencia y reproducción; revisión externa pendiente).

"Propuesto" = recomendación redactada, sin aceptar. Un ADR pasa a "Aceptado" cuando se registra la decisión del responsable; nada que dependa de él se implementa antes.
