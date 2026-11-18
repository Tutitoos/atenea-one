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
| [010](ADR-010-l2-ethernet.md) | L2 Ethernet opt-in por red (fase 10): viabilidad por SO, modelo y trama | Aceptado (implementación en pausa hasta la auditoría) |
| [011](ADR-011-aplicacion-de-escritorio.md) | Aplicación de escritorio (Tauri 2): estado de las redes y operaciones con autorización del sistema | Aceptado (revisado por ADR-013; §1, §4 y §5 sustituidos por ADR-019) |
| [012](ADR-012-nombres-de-dispositivo.md) | Nombres de dispositivo: metadatos de presentación sin firmar | Aceptado |
| [013](ADR-013-redes-multiples-y-union-por-codigo.md) | Varias redes por dispositivo y unión por código | Aceptado (fases 1–4 implementadas; Windows pendiente) |
| [014](ADR-014-servidores-roles-y-redes-por-servidor.md) | Servidores con varias redes, rol de servidor y pulido de la aplicación | Aceptado (rol, información del servidor y estado desplegados con la v0.1.6; falta una segunda red real con pares reales) |
| [015](ADR-015-bloque-b-miembros-y-renombrado.md) | Bloque B del panel: privacidad de miembros, renombrado e información del servidor | Aceptado (desplegado con la v0.1.6) |
| [016](ADR-016-politicas-de-acceso-minimas.md) | Políticas de acceso mínimas para el piloto y para redes con terceros | Aceptado (aplicado en el piloto el 2026-11-01) |
| [017](ADR-017-redes-por-instancia-en-windows.md) | Redes por instancia en Windows (servicio, adaptador y tubería por red; UAC desde la app) | Aceptado (sin implementar) |
| [018](ADR-018-union-solo-con-el-codigo.md) | Unirse con solo el código: conectividad en el código, anclas automáticas en el coordinator y `join --up` | Aceptado el 2026-11-03 (plan de la v0.1.9; sin implementar) |
| [019](ADR-019-aplicacion-de-escritorio-wails.md) | Aplicación de escritorio con Wails v3 (sustituye a Tauri 2) | Decidido el 2026-11-03; implementado en `main` (esqueleto, bandeja y avisos, empaquetado sin firma); asistente de primera ejecución en ADR-021 |
| [020](ADR-020-token-de-servicio-de-solo-lectura.md) | Token de servicio de solo lectura para asistentes de IA (`ai-atenea-one`) | Aceptado e implementado el 2026-11-03 (sin desplegar) |
| [021](ADR-021-asistente-de-primera-ejecucion.md) | Asistente de primera ejecución de la aplicación de escritorio (modelo de privilegios) | Aceptado el 2026-11-04 (implementación en curso) |
| [022](ADR-022-instalar-el-cliente-con-la-aplicacion.md) | Que la aplicación de escritorio traiga el cliente (instalador integrado) | Aceptado e implementado el 2026-11-04, opción O2; publicado en la v0.1.15 |
| [023](ADR-023-instalador-unico-de-windows.md) | Un único instalador de Windows (agente, CLI, Wintun y aplicación) | Aceptado el 2026-11-06 con las ocho recomendaciones (sin implementar; nada probado en Windows real) |
| [024](ADR-024-servicios-expuestos-por-dispositivo.md) | Servicios expuestos por dispositivo: publicarlos y abrirlos con un clic | Aceptado el 2026-11-08 con las cuatro recomendaciones (fases 2 a 5 publicadas en la 0.1.22; fase 6, el panel, implementada y sin publicar) |
| [025](ADR-025-versiones-instaladas-por-dispositivo.md) | Versiones instaladas de cada dispositivo (agente, CLI y aplicación) | Aceptado el 2026-11-16 con las decisiones D1, D2 y las tres propuestas P1 a P3 (fases 2 a 6 implementadas; publicadas en la 0.1.32 y desplegadas salvo en el Windows apagado) |
| [026](ADR-026-administracion-delegada-por-dispositivo.md) | Administración delegada por dispositivo: permisos por red con alcance, desde la CLI y la aplicación | Aceptado el 2026-11-17 con las cinco recomendaciones (fases 2 a 7 implementadas; publicadas en la 0.1.33 y desplegadas en el piloto; ensayo de retroceso `rollback-0013` y [guía](../operations/administracion-delegada.md); la aplicación no hace escrituras en Windows) |
| [027](ADR-027-concesion-delegada-de-permisos.md) | Concesión delegada de permisos: administrar permisos desde un dispositivo con `write:grants` | Aceptado el 2026-11-18 con las seis decisiones D1 a D6 (implementado; publicado en la 0.1.34 y desplegado en el piloto; falta ejercer `write:grants` allí; modifica una regla de ADR-026) |
| [028](ADR-028-cuentas-propias-y-redes-por-cuenta.md) | Cuentas propias, redes por cuenta e invitaciones: raíz bajo custodia del usuario, registro local y GitHub, organización personal e invitación por enlace | Aceptado el 2026-11-18 con las nueve decisiones D1 a D9 (control plane, panel y `--issuer-pub` en `main`; sin publicar; [guía](../operations/cuentas-y-redes-compartidas.md)) |
| [029](ADR-029-redes-gestionadas-por-el-servidor.md) | Redes gestionadas por el servidor: crear una red con solo un nombre, raíz cifrada en el servidor y certificado automático | Aceptado el 2026-11-18 con las seis decisiones D1 a D6 (implementación en curso; modifica ADR-005 para las redes gestionadas) |

Contratos: [Configuración firmada v1](../protocol/signed-config-v1.md) (aceptado). Seguridad: [Modelo de amenazas](../security/threat-model.md) (aceptado). [Inventario de dependencias](dependencies.md).

Paquete de entrega para la [auditoría independiente](../audit/README.md) (alcance, arquitectura, evidencia y reproducción; revisión externa pendiente).

"Propuesto" = recomendación redactada, sin aceptar. Un ADR pasa a "Aceptado" cuando se registra la decisión del responsable; nada que dependa de él se implementa antes.
