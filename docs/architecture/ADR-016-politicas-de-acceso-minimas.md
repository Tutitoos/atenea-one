<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-016: Políticas de acceso mínimas para el piloto y para redes con terceros

- **Estado:** Propuesto (2026-10-29). Decisión del responsable pendiente. No cambia código del agente ni del control plane; solo fija cómo se usan las políticas existentes y qué límites se aceptan.
- **Relacionados:** ADR-002, ADR-007, ADR-014 y `docs/protocol/signed-config-v1.md` §8.

## Contexto

La red del piloto usa una única política `any↔any` (todo protocolo, todo dispositivo). El propietario quiere abrirla a pocas personas de confianza. Con `any↔any`, **cualquier dispositivo aprobado en el futuro obtiene acceso total a todos los demás** y pasa a ser par de todos ellos; la aprobación del dispositivo es entonces la única barrera.

El motor de ACL del agente es **sin estado**: decide paquete a paquete (`crates/policies`), en el origen y en el destino. Para que las respuestas funcionen, el control plane añade a cada política su **regla inversa** con el mismo protocolo, selectores intercambiados y **sin puertos** (`apps/control-plane/src/issue.rs`, `compile`).

## Hechos medidos

Laboratorio con agentes reales y control plane real (`tests/network-lab/e2e-cp.sh`, sección «Políticas», 2026-10-29). Solo IPv4 para TCP y UDP; ICMP en v4 y v6 para `any`.

| Política creada | Observado |
| --- | --- |
| TCP A→B 9000–9100 | B→A al puerto 9500: **no se completa la conexión**, pero el SYN **sí pasa** la ACL de entrada de A y el escuchador no recibe nada; A descarta su propia respuesta en su ACL de salida. B no es frenado en origen. |
| UDP A→B 9600–9700 | A→B al 9650 llega. A→B al 9500 se frena en origen. **B→A al 9500 llega** (la inversa no lleva puertos): para UDP no hace falta respuesta, así que el límite de puertos solo protege un sentido. |
| ICMP `any↔any` | Un dispositivo recién aprobado alcanza a A y A a él, sin que nadie haya creado una política para él. |
| ICMP solo entre A y B (sustituye a `any↔any`) | El dispositivo nuevo deja de alcanzar a A y a B, no recibe **ningún par** en su configuración y A conserva solo a B. A y B siguen comunicándose. |

## Decisión propuesta

1. **No usar `any↔any` en una red con terceros.** Un dispositivo nuevo aprobado sin política queda sin pares y sin acceso (medido).
2. **Políticas por pares explícitos** (selector `device`), por protocolo y, en TCP y UDP, con el rango de puertos del servicio. ICMP y ICMPv6 son dos políticas por par.
3. **Aceptar los límites de la ACL actual** y no presentarlos como más de lo que son:
   - ICMP y UDP son efectivamente **bidireccionales**; el rango de puertos solo protege el sentido original.
   - En TCP la conexión solo se completa en el sentido permitido, pero los SYN de la parte «servidor→cliente» llegan al sistema operativo del cliente.
   - Cada persona o dispositivo nuevo exige crear sus políticas a mano (máximo 256 por red, ver contrato del control plane).
4. **Aplicación en el piloto, después del soak vigente y con autorización expresa:** crear primero las políticas explícitas, comprobar el tráfico real, y solo entonces borrar `any↔any`. La vuelta atrás es una sola creación de `any↔any`. Cada cambio llega a los agentes con su siguiente sincronización. Las políticas explícitas deben cubrir también el tráfico de las mediciones del propietario, o éstas fallarán.

Conjunto mínimo para el piloto actual (tres dispositivos: equipo del propietario `M`, servidor `V` y cliente de pruebas `T`): ICMP e ICMPv6 entre `M↔V`, `M↔T` y `T↔V` (6 políticas); TCP 22 de `M` a `V` y de `M` a `T` (2 políticas); más el TCP del tráfico de medición de quien lo use, en el sentido que corresponda.

## Alternativas no decididas

- **Seguimiento de conexiones en el agente** (permitir respuestas sin regla inversa): es lo único que haría unidireccional a UDP e ICMP y cerraría el SYN entrante. Exige cambiar el contrato firmado y su versionado (agentes anteriores no lo entienden), una tabla de flujos acotada y con caducidad, y tratar fragmentos, IPv6, ICMP y UDP. Necesita ADR propio y revisión de seguridad antes de escribir código.
- **Grupos o etiquetas como selector**: evita crear N políticas por dispositivo nuevo. Nuevo selector en el contrato firmado: mismo coste de ADR.
- **Usar el rol `server` como selector**: **descartado.** El rol es una etiqueta de presentación sin firmar (ADR-014) y no puede decidir autorización.

## Consecuencias

- Abrir la red a otra persona deja de ser «aprobar el dispositivo» y pasa a ser «aprobar y escribir sus políticas»: más seguro, más trabajo manual.
- El panel ya permite crear y borrar políticas por dispositivo; no hay cambios de producto.

## Límites de esta evidencia

Laboratorio sin el piloto real. No se midió UDP ni TCP por IPv6, ni el límite de 256 reglas con muchos pares, ni el comportamiento del panel con decenas de políticas, ni la aplicación en el piloto. No se ha cambiado ninguna política del piloto.
