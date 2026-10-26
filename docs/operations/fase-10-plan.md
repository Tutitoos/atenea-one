# Fase 10: plan de implementación por hitos (L2 y funciones avanzadas)

- **Estado:** ADR-010 **aceptado** con las decisiones D1–D7 el 2026-10-26; **ejecución de este plan EN PAUSA** hasta recibir el informe de la auditoría externa (fase 11) y triar sus hallazgos críticos/altos. Sin código, sin laboratorios ejecutados y sin resultados: las pruebas de salida son **exigencias**, no evidencia. El tramo B (M6: Monaco, topología avanzada, selección regional) también espera, por ser trabajo de producto sobre una base aún sin auditar.
- **Basado en:** `PLAN.md` fase 10 («L2 es un hito propio; no activarlo por defecto ni anunciar equivalencia completa con ZeroTier por soportar TAP»), [l2-v1](../protocol/l2-v1.md), [wire-v2](../protocol/wire-v2.md), [signed-config-v2](../protocol/signed-config-v2.md), [matriz de aceptación](matriz-de-aceptacion.md), laboratorio existente [`tests/network-lab`](../../tests/network-lab/README.md).
- **Contexto:** el proyecto está en fase 11 (auditoría externa pendiente). Cada hito L2 es **aditivo**: no cambia wire-v2, relay-v2 ni la verificación v1/v2; las redes L3 existentes deben seguir pasando sus pruebas actuales sin modificación (regresión obligatoria en cada hito).
- **Convención:** `VERIFICADO` = con salida real (ninguno todavía); `INFERIDO`; `SIN COMPROBAR`.

## 1. Alcance y separación en dos tramos

La fase 10 del plan agrupa dos cosas **independientes**; este documento las separa para que cada una sea entregable y verificable por separado:

| Tramo | Contenido | Hitos | Dependencias |
| --- | --- | --- | --- |
| **A. L2 Ethernet** | Viabilidad por SO, TAP, formato de trama, MAC, ARP/ND, difusión acotada, bucles y tormentas, aislamiento, ACL L2, mDNS/DHCP | M0–M5, M7 | ADR-010 aceptado; parser v3 en `crates/identity` |
| **B. Funciones avanzadas** | Editor Monaco validado y versionado para políticas, topología avanzada, selección regional por latencia/pérdida/carga, evaluación de Quinn/QUIC (solo con mediciones; sin compresión inicial) | M6 | Control plane y panel (fases 3/3B); **no** depende de L2 |

El tramo B no bloquea ni es bloqueado por el A. Si el responsable prioriza solo uno, el otro se pospone sin reabrir este plan.

## 2. Reglas de entorno para quien implemente

Las del responsable para este entorno: sin Docker, sin descargas ni instalaciones (el Mac del usuario pierde Internet con descargas); solo compilación con `CARGO_BUILD_JOBS=2 cargo … --offline --locked`. **Los laboratorios Docker y los runners de GitHub se ejecutan fuera de este entorno** (CI o el servidor de laboratorio del responsable); cada hito distingue lo que se puede probar en un Mac (pruebas unitarias/propiedades/fuzz con `--offline`) de lo que exige Linux real. Repo público: sin secretos, alias ni rutas privadas. No usar «seguro», «auditado» ni «listo para producción» en ningún texto de la fase.

## 3. Hitos del tramo A (L2)

Orden pensado para que cada hito sea entregable, verificable y revertible; nada activa L2 por defecto en ningún hito.

### M0: Decisión y contratos (sin código de producto)

- **Entregables:** ADR-010 aceptado (o modificado) con D1–D7 registradas; `l2-v1.md` pasa a normativo; ADR-010 y `l2-v1` actualizados con las decisiones; `threat-model.md` recibe T31–T45 con su estado «Exigido»; frase de posicionamiento aprobada.
- **Prueba de salida:** el responsable registra cada decisión con fecha; los tres documentos no contienen «Propuesto» en lo aceptado; enlaces relativos válidos (script de solo lectura de este plan, §8).
- **Laboratorio:** ninguno.

### M1: Linux TAP + trama + MAC firmada entre dos agentes

- **Alcance:** `crates/identity` parser/verificación de `DeviceConfig` v3 (§2 de l2-v1) y emisión con `atenea issue` (`mode = "l2"`, MAC asignada); `crates/tunnel` nuevo tipo TAP Linux separado de `Tun` (`IFF_TAP | IFF_NO_PI`, contrato «trama completa», sin tocar `TunDevice`); `apps/agent` pipeline de recepción/emisión de §3 de l2-v1 (longitud, EtherType, MAC origen, destino, IP origen ∈ `addrs`, `mtu − 14` en `configure_link`, MAC en el TAP, detección de `master`, `UnsupportedL2Mode` en macOS/Windows); tabla estática; contadores; **sin** ARP/ND locales todavía (el laboratorio instala vecinos permanentes `ip neigh … permanent`; ver §5), **sin** ACL L2 (se evalúa solo la ACL L3 existente), `flood = 0`.
- **Pruebas de salida:**
  1. Vectores v3 (válido + un negativo por regla) y v1/v2 byte a byte sin cambios; fuzz `config_v3_parse` y `l2_frame_parse` con corpus de partida; propiedad round-trip.
  2. Dos agentes Linux en el laboratorio de red: `ping` IPv4 e IPv6 por **TAP** con la MAC firmada, TCP/UDP íntegros, MTU `mtu−14` (con 1400: paquete 1386 pasa, 1387 no), parada limpia sin interfaz residual.
  3. Ataques: tramas con MAC origen de otro par, EtherType LLDP/STP/0x8100, trama de `mtu+1`, runt (< 14), destino unicast ajeno: todos descartados con el contador esperado; ningún dato llega a la pila IP.
  4. `bridged = true` al enlazar el TAP a un `bridge` (T35).
  5. Un agente v1/v2 recibe la configuración v3: `UnsupportedVersion`; un agente nuevo en macOS/Windows: `UnsupportedL2Mode` sin cambiar estado (CI macOS/Windows existentes).
  6. Regresión: pruebas L3 y e2e existentes sin cambios.
- **Amenazas cubiertas:** T31, T35, T36 (estática), T38, T41, T42, T45 (descarte).
- **Laboratorios:** extensión de `tests/network-lab` (contenedor privilegiado, `/dev/net/tun`); job de CI Linux con `sudo`/namespaces (INFERIDO; SIN COMPROBAR que `ubuntu-latest` permita crear TAP y bridges: el laboratorio actual ya usa contenedor privilegiado, véase su README).

### M2: ARP/ND locales, bucles y tormentas

- **Alcance:** respuesta local ARP/NS (l2-v1 §7), descarte de ARP/ND/RA/DHCP recibidos de pares, `l2_ctl_pps`; difusión/multicast **descartados** (`flood = 0`) con contadores; eco por hash y cuarentena (l2-v1 §11); límites de colas y de tasa (§12); eliminación de los vecinos permanentes de M1.
- **Pruebas de salida:**
  1. `ping` entre hosts **sin** vecinos permanentes: ARP/ND resueltos localmente; captura en el cable sin ARP/ND (control positivo: sí aparecen en el TAP).
  2. Par hostil envía ARP/NA/RA/DHCP forjados: caché ARP/neigh de la víctima inalterada; 0 ofertas DHCP.
  3. Tormenta: difusión máxima desde el TAP y desde un par: CPU/RAM del agente dentro del presupuesto medido, otros pares sin degradación visible; `flood_dropped` y `l2ctl_ratelimited` crecen.
  4. Bucle deliberado (TAP unido a un puente con retorno): `loop_suspected`/`loop_quarantine` observados y tráfico acotado; sin crecimiento sostenido.
  5. Fuzz de ARP/ND/IPv6 interno.
- **Amenazas cubiertas:** T32, T33, T34, T39.
- **Laboratorios:** ampliar con herramientas de generación de tramas. Las herramientas ya presentes en la imagen o en `crates/testpeer` (INFERIDO: puede fabricar paquetes) tienen prioridad; **añadir herramientas a la imagen exige descarga al construirla** y debe hacerse en un entorno con Internet, no en el Mac del responsable.

### M3: ACL L2, aislamiento entre redes y coexistencia L3

- **Alcance:** `L2Rule`/`L2Selector` en `crates/identity` (ya en el parser de M1) y evaluación `allows_l2` en `crates/policies` aplicada antes de `Policy::allows`; defaults de CLI/API/panel (`mode` explícito); matriz de documentación y frase de posicionamiento (D7).
- **Pruebas de salida:**
  1. Matriz de ACL: por EtherType, MAC, OUI, dispositivo; denegación por defecto; regla inversa; ambas ACL (L2 y L3) necesarias.
  2. **Aislamiento:** dos redes L2 con **la misma MAC** en un host (y bajo relay): 0 tramas cruzadas, tablas y cubos independientes (T37).
  3. **Coexistencia L3:** una red `l3` y una red `l2` en laboratorio con los mismos hosts y rutas ADR-007 en la `l3`: ambas funcionan; una red L2 rechaza `routes`/`advertised` (T35).
  4. Revisión de documentación: ninguna frase de equivalencia; `mode = "l2"` nunca por defecto (T43, T44).
- **Amenazas cubiertas:** T31 (reglas), T37, T43, T44.

### M4: macOS y Windows (según D1/D2)

- **Por defecto (D1-A, D2-A): no hay implementación.** El hito se cierra con documentación: la matriz de aceptación y ADR-008 indican «L2 no soportado en macOS/Windows» y la prueba de rechazo (`UnsupportedL2Mode`) ya cubierta en M1.
- **Si D1-B:** spike `feth`+BPF en macOS **etiquetado experimental**, con prueba de que se crean y se limpian interfaces tras `kill -9` y límites documentados; sin entrega a usuarios finales.
- **Si D1-C o D2-B/C:** requiere nuevo ADR (cliente NetworkExtension con Developer ID, o adaptador TAP-Windows6 con revisión de licencia GPL-2.0 y firma EV/atestación), runners reales de GitHub y máquinas del responsable (ADR-008 sigue sin matriz de interoperabilidad). No se planifica en detalle hasta decidirlo.
- **Prueba de salida (si se ejecuta):** tráfico Ethernet entre Linux y el SO nuevo, MTU, aislamiento de IPC y limpieza; igual criterio que ADR-008 §5.

### M5: mDNS y DHCP (según D7)

- **Alcance por defecto:** `flood = 1` (mDNS) con presupuesto y replicación acotada (l2-v1 §10); **sin DHCP**.
- **Pruebas de salida:** descubrimiento mDNS entre N pares con ACL L2/L3; no reenvío entre pares; tasa respetada (`flood_ratelimited`); sin reflexión hacia otras redes ni LAN; malla de N bajo relay respeta las cuotas de relay-v2 (T40); tormenta mDNS contenida.
- **Si el responsable acepta DHCP:** nuevo documento y pruebas (concesiones, agotamiento, servidor falso, T45) antes de cualquier código; se sale de este plan.

### M7: Cierre del tramo A

- **Alcance:** límites de rendimiento **medidos y documentados** (throughput y latencia TAP↔TAP por P2P y por relay, pps de tramas pequeñas, CPU por trama, coste de replicación con N pares) con el método de [carga-control-metodologia](carga-control-metodologia.md) y [rendimiento-y-presupuestos](rendimiento-y-presupuestos.md); actualización de la matriz de aceptación, de `threat-model.md`, de ADR-008/README, de la guía de instalación Linux y de las advertencias de `PLAN.md`; preparación del paquete de [auditoría](../audit/README.md) con el alcance L2 (la revisión externa en curso no lo incluye).
- **Prueba de salida:** todos los criterios del plan para L2 (§5) con evidencia enlazada; ninguna cifra sin su comando y su resultado.

## 4. Hito del tramo B (M6): funciones avanzadas

Independiente de L2; su contenido son cuatro piezas, con salida propia cada una:

| Pieza | Alcance mínimo | Prueba de salida |
| --- | --- | --- |
| **Editor Monaco de políticas** | Esquema de política **versionado** y validado en servidor con las mismas reglas que `crates/identity` (denegación por defecto, selectores válidos, límites); el editor es una vista del esquema: no hay camino para guardar texto sin validar | «Editor incapaz de guardar políticas inválidas o saltarse autorización» (plan): pruebas de API con cargas inválidas (rechazo), RBAC (usuario sin permiso no guarda ni emite), versionado y rollback; pruebas de accesibilidad del panel |
| **Topología avanzada** | Vistas derivadas de datos reales ya expuestos (presencia, caminos, relays) | Datos reales vs. vista, sin estado inventado; regresión del panel |
| **Selección regional por latencia/pérdida/carga** | Recoger métricas primero; elegir relay por medición con histéresis (relay-v2 §7) | Laboratorio con `netem` (latencia/pérdida/carga): elección estable, sin oscilación, y caída del mejor relay |
| **Quinn/QUIC** | **Solo** si una medición documenta una necesidad concreta (por ejemplo, HOL en el fallback TCP); sin compresión inicial | Informe de mediciones previas + decisión del responsable; si no hay necesidad, queda **no implementado** |

M6 puede ir en paralelo a M1–M5 con un equipo distinto. No hay condición de salida compartida con L2.

## 5. Criterios de aceptación de la fase 10 (del plan) y su hito

| Criterio (`PLAN.md`, fase 10) | Hito | Notas |
| --- | --- | --- |
| Tráfico Ethernet real entre SO soportados | M1 (Linux), M4 si cambia D1 | v1 = Linux↔Linux |
| Descubrimiento y broadcast acotados | M2, M5 | ARP/ND locales (M2); mDNS (M5) |
| Ataques de MAC spoofing y tormentas contenidos | M1, M2 | T31, T33, T34 |
| Redes aisladas | M3 | T37 |
| Coexistencia L3 | M1, M3 | regresión L3 + red mixta de laboratorio |
| Límites de rendimiento documentados | M7 | solo con mediciones reales |
| Editor incapaz de guardar políticas inválidas o saltarse autorización | M6 | tramo B |
| Guardas del plan: L2 no por defecto ni equivalencia con ZeroTier | M0, M3, M7 | frase aprobada (D7) |

## 6. Laboratorios y CI necesarios (todos fuera de este entorno)

| Laboratorio | Para | Estado |
| --- | --- | --- |
| `tests/network-lab` extendido con `/dev/net/tun`, tres o más namespaces (A, B, C no autorizado, D hostil) y `bridge` deliberado | M1–M3 | **No existe**; el laboratorio actual usa un contenedor privilegiado con puente y `netem` (README, fase 2) |
| Laboratorio relay/malla de N | M2, M5 | Reutiliza el de fases 5/6 (INFERIDO) |
| Job de CI Linux privilegiado (`sudo` + `ip tuntap`) | M1–M3 | SIN COMPROBAR que el runner lo permita |
| Fuzz (`fuzz/fuzz_targets`): `config_v3_parse`, `l2_frame_parse`, ARP/ND | M1, M2 | Nuevos objetivos; corpus inicial |
| macOS/Windows: runners reales (si D1/D2 cambian) | M4 | Solo con decisión; hoy sin L2 |
| Medidas de rendimiento en máquinas reales | M7 | Pendiente de hardware del responsable |
| Entorno con Internet para construir imágenes con herramientas de tramas | M2 | Solo si `testpeer` no basta |

## 7. Documentación a actualizar en cada hito

M0: ADR-010, `l2-v1`, `threat-model.md`, `README` de ADR. M1: `PLAN.md` (estado de fase), `signed-config-v3` (o promoción de la sección de `l2-v1` a documento propio), `docs/api` (modo de red, asignación de MAC), instalación Linux. M3: matriz de aceptación, panel. M7: rendimiento, soporte y límites, auditoría. Todos: `docs/architecture/dependencies.md` si se añade alguna dependencia (la propuesta no requiere ninguna nueva: INFERIDO, no verificado).

## 8. Verificación de este documento

Los tres documentos de diseño se verifican con un script de **solo lectura** que comprueba la existencia de cada destino de enlace relativo; el resultado va en el informe de entrega. No ejecuta compilaciones, laboratorios ni pruebas.

## 9. Pendiente y sin comprobar

- Todas las pruebas de salida están **sin ejecutar**; ningún comportamiento descrito está implementado.
- Viabilidad de `ubuntu-latest` para TAP/bridge, herramientas de generación de tramas, rendimiento y presupuestos: **SIN COMPROBAR**.
- macOS/Windows: ADR-010 §1 (SIN COMPROBAR `feth`+BPF, `NEEthernetTunnelProvider`, licencia TAP-Windows6).
- Coste de calendario: no se estima (el plan prohíbe fijar fechas sin medir).
