# ADR-009: Operación, disponibilidad, secretos y releases (fase 9)

- **Estado:** **Aceptado** por el responsable el 2026-10-26. La arquitectura, la política de secretos, el contrato de migraciones y el proceso de release se aceptan tal como están. Los objetivos RPO ≤ 24 h y RTO ≤ 2 h se aceptan como **objetivos del piloto**, no como compromiso de servicio, y se confirman o corrigen con una medición real en un VPS (la medida hasta ahora es de Docker sobre Mac arm64 con datos pequeños). La publicación en GHCR/Releases y la clave de firma de releases siguen requiriendo decisión y autorización expresas.
- **Relacionados:** ADR-002 (revocación/lease), ADR-004 (entrada 443), ADR-005 (custodia de la clave raíz), ADR-007.

## Contexto

La fase 9 convierte el código en algo instalable, recuperable y actualizable. No se puede aprovisionar infraestructura real desde el entorno de desarrollo (sin credenciales de proveedor ni autorización de despliegue), así que todo lo que se entrega debe ser verificable de forma local o en runners desechables, y distinguir con claridad lo ensayado de lo pendiente.

## Decisiones

### 1. Topología

- **Nodo principal** (un VPS con IPv4+IPv6): HAProxy (TCP 443, L4, SNI según ADR-004), Caddy (TLS del panel; HTTP-01 en 80), control plane, coordinator, PostgreSQL, OIDC (Dex en el piloto), relay y STUN. Todo en Docker Compose con redes internas; solo HAProxy, Caddy (80) y los puertos UDP del relay/STUN publican en el host.
- **Nodo relay secundario** (proveedor distinto): solo `relay` + STUN + exportador de métricas. Sin base de datos, sin claves de firma de configuración. Se registra en `ATENEA_COORD_RELAYS` del coordinator.
- **Raíz**: la clave raíz **nunca** vive en ningún nodo (ADR-005). Los nodos tienen la clave de emisor (con caducidad) y la semilla de firma de tokens del relay.
- Nombres: `panel.<dominio>`, `relay.<dominio>` (y `relay2.<dominio>` para el segundo), con el dominio aportado por el operador en su inventario privado ([ADR-004](ADR-004-entrada-443.md)); registros solo DNS, sin proxy.

### 2. Disponibilidad: qué se promete y qué no

| Componente | HA en v1 | Comportamiento ante caída |
| --- | --- | --- |
| Relay | **Sí** (dos relays de proveedores distintos) | El agente conmuta de relay con el coordinator; caminos directos no se interrumpen |
| Coordinator | **No** (instancia única, estado efímero en memoria) | P2P y relay ya establecidos siguen mientras duren sus autorizaciones; no hay nuevos rendezvous ni nuevas asignaciones/tokens de relay hasta volver; presencia se reconstruye en ≤ un ciclo de announce (20 s en el agente actual) |
| Control plane + PostgreSQL | **No** (instancia única; restauración, no failover) | Los túneles siguen mientras la configuración firmada esté vigente (lease de 300 s en piloto); al caducar sin renovación el tráfico se corta (diseño de ADR-002). Es **fail-closed** y se mide |
| HAProxy/Caddy | **No** | Caen panel y relay TLS; UDP sigue |

Valkey **no** se introduce: el estado efímero del coordinator no se comparte entre instancias porque solo hay una; añadir caché sin diseño de HA no aporta disponibilidad. La HA de control plane/coordinator requiere un diseño propio (réplica de PostgreSQL, elección de líder, estado de presencia compartido) y queda fuera de v1; se documenta como límite.

### 3. Recuperación

Propuesta para el piloto: **RPO ≤ 24 h y RTO ≤ 2 h**. PostgreSQL con copia base diaria y archivo continuo de WAL (pgBackRest) cuando el simulacro demuestre un RPO mejor; en cualquier caso el objetivo es la **medición real publicada**, no la cifra. Las claves (emisor, semilla del relay, clave de aplicación) se restauran desde copia cifrada con `age` en medios offline; la **restauración de la clave de firma está ensayada**, no solo descrita.

### 4. Secretos

SOPS + age. Los ficheros cifrados viven en el repo **solo si** pertenecen al operador (el repo público contiene ejemplos sintéticos y un esquema; nunca valores reales ni la clave age). Los contenedores reciben secretos por ficheros montados (0400), nunca por variables de entorno visibles en `inspect` cuando el servicio admite `*_FILE`.

### 5. Observabilidad

Métricas Prometheus (`/metrics` en loopback/red interna) de control plane, coordinator y relay; Grafana y Loki. **Cardinalidad acotada**: etiquetas de conjunto cerrado (resultado, camino, código de error); nunca identificadores de dispositivo, de red, IP ni claves como etiqueta. Los registros no contienen tokens, claves, cuerpos de configuración ni correos. Hay prueba de que no los contienen.

### 6. Releases

Un release es un **manifiesto firmado** con: commit, versiones, digest de cada imagen, SBOM (CycloneDX) de cada imagen/binario, SHA-256 de cada artefacto (`.deb`, `.pkg`, ZIP de Windows, binarios) y firma. Imágenes fijadas **por digest** en Compose y Ansible, nunca por etiqueta. Publicar en GHCR/Releases requiere autorización explícita del responsable: el flujo de release se escribe y se ensaya localmente con claves efímeras; no se ejecuta contra GHCR sin esa autorización. La clave de firma de releases es una decisión del responsable (ADR-005 trata la clave raíz, no esta).

**Un solo mantenedor (2026-10-27).** El proyecto tiene un único mantenedor, así que el environment `release` exige un revisor obligatorio pero **no** «Prevent self-review»: una segunda cuenta del mismo responsable no aportaría separación real. Riesgo residual aceptado: quien controle la cuenta del mantenedor puede lanzar y aprobar un release. Lo limitan la política de despliegue solo para tags `v*`, el ruleset que impide mover o borrar esos tags, la atestación de procedencia verificada por fichero y commit y la clave de firma que nunca está en el repositorio. Si el proyecto tiene más mantenedores, se vuelve a exigir «Prevent self-review».

### 7. Actualización y vuelta atrás

Migraciones **expand/contract** con orden fijo: primero se migra el esquema (solo aditivo), después se despliega N+1. Obligación: **N funciona sobre el esquema de N+1** (es lo que hace posible el rollback al digest anterior sin restaurar la base). No se exige que N+1 funcione sobre el esquema de N, porque nunca se despliega antes de migrar. El borrado de columnas o tablas va una versión después de que ninguna versión desplegable las use. Rollback = redeploy del digest anterior; se ensaya con una migración real (0001/0002→0003), datos y escrituras (`tests/stack/upgrade.sh`).

## Fuera de alcance de v1

Alta disponibilidad de control plane/coordinator/PostgreSQL, Valkey, multi-región con selección por latencia, balanceo de HAProxy, WAF.

## Verificación

Instalación limpia desde inventario propio en contenedor systemd; funcionamiento sin servicios externos de VPN/STUN/identidad; caída de relay/coordinator/control plane con medición de leases; restauración de BD y clave de firma con RPO/RTO medidos; upgrade y rollback con migraciones; manifiesto de release verificado con la firma; instaladores probados, no solo imágenes. Lo no ensayado en infraestructura real (proveedor, DNS, firewall externo, UDP/TCP 443 alcanzables) se declara explícitamente como **sin comprobar**.
