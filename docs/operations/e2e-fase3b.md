# Fase 3B — E2E con control plane real

- **Fecha:** 2026-10-24
- **Entorno:** contenedores desechables en Docker Desktop (aarch64): PostgreSQL 17, Dex 2.41.1 (OIDC, usuarios sintéticos públicos), el control plane como proceso y dos agentes con TUN real en namespaces unidos por un puente. Sin NAT.
- **Reproducir:** `tests/network-lab/run-e2e.sh` (salida distinta de cero si falla algún caso). Valores de laboratorio: validez de configuración 40 s, sondeo del agente cada 10 s, tolerancia 30 s. El piloto usa 300 s / 60 s / 30 s (ADR-002).

## Resultados (última ejecución)

| Caso | Resultado |
| --- | --- |
| Migraciones con el rol de aplicación limitado (sin `UPDATE`/`DELETE` sobre auditoría) y servicio arrancado con ese rol | correcto |
| Inicio de sesión de administrador con OIDC real (Code + PKCE contra Dex) | correcto |
| Organización, red (IPv4 e IPv6), certificado de emisor firmado offline por la raíz y subido al control plane | correcto; el control plane lo valida (red, raíz, firma, vigencia) |
| Enrolamiento con token de un solo uso; el mismo token no se reutiliza | correcto |
| El dispositivo queda `pending`; el inventario muestra su huella; la aprobación asigna IPv4+IPv6 por IPAM | correcto (A=10.77.0.1, B=10.77.0.2 / fd77::2) |
| Políticas: ICMP libre y TCP A→B 9000-9100 (con reglas inversas generadas) | correcto |
| Agentes sincronizan, verifican con `verify_config` y aplican la configuración del control plane | ambos con lease vigente |
| Ping IPv4 e IPv6; TCP 2 MB íntegro A→B por la política; UDP sin política descartado en origen | correcto |
| Auditoría de la red: `device.enroll`, `device.approve`, `policy.create`, `config.issue` | presentes |
| Revocación de A en el control plane: B deja de aceptar a A | a los **7 s** (sondeo de B ≤ 10 s + recarga) |
| A recibe `403` del control plane tras revocarlo | contador `sync_denied` ≥ 1 |
| El propio A se corta al caducar su última configuración | a los **64 s** de la revocación (≤ 40 + 30 s tras su última emisión) |
| Pérdida del canal de control: el control plane se detiene | B se corta solo a los **69 s** (`sync_errors` ≥ 1) |
| El control plane vuelve | B recupera una configuración vigente |

## Qué no demuestra

- Tiempos con los valores del piloto (300/60/30): se escalan a ≈ 60 s para peers y ≈ 330 s para el agente aislado, pero no se midieron.
- Una sola instancia del control plane; sin alta disponibilidad ni copias de seguridad de PostgreSQL (fase 9). La caché de nonces es en memoria.
- HTTP sin TLS dentro del laboratorio. En producción el enrolamiento y la sincronización requieren HTTPS (el token de enrolamiento viaja en claro en HTTP).
- Un único usuario administrador de Dex; el RBAC multi-usuario se prueba en las pruebas de integración del control plane (16 pruebas contra PostgreSQL real), no en el E2E.
- Sin NAT, sin endpoints descubiertos: los `endpoints` los fija el administrador (fase 4 los sustituye).
- La API de inventario **no expone las direcciones IP asignadas** a cada dispositivo; el laboratorio las leyó de la interfaz del agente. El panel (fase 6) lo necesita.
- El `agent-config` genera la sincronización con el intervalo por defecto; el laboratorio añadió `interval_secs = 10` a mano.
- La revisión criptográfica externa sigue pendiente.
