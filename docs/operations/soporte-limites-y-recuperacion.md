# Soporte, límites y recuperación

Corte: **2026-10-25**, revisión documental `d9ab7ec4b4af44dd022ab655b7747c16231a6f8b`. **LOCAL-V2** significa «ejecución local sobre el árbol de trabajo previo al commit d9ab7ec (HEAD 954a503)»: hubo ediciones de scripts entre tandas, sin comparación byte a byte con el commit. Solo la CI de GitHub se atribuye a `d9ab7ec`. Estado de aceptación y evidencia por caso: [matriz](matriz-de-aceptacion.md). **Producto en evaluación, sin auditoría independiente: no usar con datos sensibles.** No se ofrece SLA ni aceptación de producción por tener CI verde.

## Plataformas y versiones

| Plataforma | Alcance disponible | Evidencia y límite de soporte |
| --- | --- | --- |
| Linux | Agente/CLI y router de subred; paquete `.deb` **0.7.0**, Debian **13 arm64**, systemd y systemd-resolved | [Instalación Linux](instalacion-linux.md), [fase 7](e2e-fase7.md), [arnés de paquete](../../tests/packaging/run.sh). Probado realmente dentro de Docker con systemd: permisos, servicio, DNS, SIGKILL, reinicio, retirada; no es un VPS ni equipo físico. Otras distribuciones/arquitecturas no tienen esta aceptación de instalación. |
| macOS | **Solo cliente** utun, consumo de rutas privadas y DNS dividido; `.pkg` por arquitectura arm64/x86_64 | [Instalación macOS](instalacion-macos.md). Sin firma ni notarización; no sustituirlas por desactivar Gatekeeper. CI macOS verde en el runner de [40228501883](https://github.com/Tutitoos/atenea-one/actions/runs/40228501883), no instalación y ciclo launchd en máquinas físicas. No se promete una matriz de versiones de macOS no ensayada. |
| Windows | **Solo cliente**, ZIP, Windows PowerShell **5.1**, servicio SCM; backend Wintun **0.14.1** | [Instalación Windows](instalacion-windows.md). Atenea y scripts sin firma nativa; DLL oficial Wintun separada, descargada y verificada por SHA-256 y Authenticode. CI Windows verde en el mismo run, no prueba de Windows ARM64 ni equipos físicos. x64/ARM64 son objetivos del código/paquete, no ambos plataformas ya aceptadas. |
| Servicios | Stack Linux contenedorizado, PostgreSQL mayor **17** | [Stack](stack-produccion.md), [infraestructura](infraestructura.md), [release](release.md). Restore medido con PostgreSQL **17.11**, pgBackRest **2.59.3**, age **1.2.1**. Usar versiones/digests del manifiesto compatible; no inferir compatibilidad con otra versión mayor de PostgreSQL. |

No se fija una política LTS, fecha de fin de soporte o compatibilidad con releases anteriores sin evidencia. Para incidencias indicar artefacto, arquitectura, versión y commit; una compilación de desarrollo `unknown` no identifica un release verificable. Firma del manifiesto y firma nativa del instalador son garantías diferentes.

## Qué no se soporta

- **Wire-v1 y relay-v1**: cutover limpio a [wire-v2](../protocol/wire-v2.md) y [relay-v2](../protocol/relay-v2.md), ALPN exclusivo `atenea-relay/2`. No negociación mixta ni shim. HTTP API `/v1` y versiones de configuración firmada son espacios distintos del cable: no retirarlos solo por su nombre.
- Exit nodes, rutas por defecto o subredes públicas. Se admiten prefijos privados y se aplica el límite del contrato; el agente no convierte el túnel en salida general a Internet.
- **Servir/anunciar subredes en macOS/Windows**: solo consumen rutas aprobadas y DNS dividido. El router es Linux; no instala NAT/RRAS en clientes. No se promete L2/TAP/Ethernet por disponer de L3.
- Proxies HTTP y redes con inspección TLS corporativa. TLS en TCP 443 no garantiza atravesarlos; TCP sobre TCP puede degradar rendimiento. No se ofrece anonimato: relay/observador ven IPs, tamaños y tiempos.
- HA de control plane, coordinator, PostgreSQL o entrada HAProxy/Caddy, ni failover de BD/multirregión. Dos relays son un diseño de redundancia, no prueba de HA entre proveedores. Un segundo relay no mantiene renovaciones si cae el emisor.
- Capacidad de producción deducida del laboratorio, SLA de disponibilidad, restauración instantánea o recuperación automática de una raíz perdida/comprometida.

## Límites: configuración frente a medición

### Relay y coordinator

Valores **configurados**, no throughput garantizado; fuente: [tabla del relay](../../apps/relay/README.md), [coordinator](../protocol/coordinator-v1.md), [API de control](../api/control-plane-v1.md).

| Control | Valor predeterminado / regla |
| --- | --- |
| Asignaciones relay | `MAX_ALLOCATIONS=4096`, `MAX_PER_NETWORK=1024`, `MAX_PER_IP=32`; valores positivos y por red/IP no mayores que el total |
| DATA por asignación | **2000 paquetes/s**, **24 000 000 bit/s**, ráfaga de un segundo; renovar no reinicia cubos |
| Peticiones relay por origen | COOKIE **20/s**, ALLOC **10/s**; tabla de tasas **65536** entradas |
| TLS | **8 conexiones/IP**, **2048** totales; plazo PROXY+TLS **10 s**, primera asignación **5 s**, inactividad de lectura **90 s**; cola **256** mensajes/conexión |
| Tamaño | MTU predeterminada **1400**, rango firmado **1280–1500**; wire DATA máximo **1530 bytes**; cola llena descarta, no crece ilimitadamente |
| Origen para cuotas | IPv4 exacta/IPv4-mapped normalizada; IPv6 agrupado por **/64**; compartir NAT o /64 comparte presupuesto |
| Coordinator | **4096** presencias, **256** long-polls totales, máximo **2** por dispositivo; cola **16** eventos/**30 s**, hasta **8** candidatos; rendezvous **1/s por par**, **10/min por dispositivo** |
| Control plane | **256** dispositivos aprobados/red y hasta **256** políticas; overflow de reglas puede impedir otra política; no son objetivos de rendimiento medidos |

**Asignaciones no son pares.** En una malla N, todos por relay, se requieren **N·(N−1)** asignaciones totales (dos por par); si K comparten una IP, consume **K·(N−1)** por esa IP. Todos tras una IP: N=6 necesita **30**, N=7 **42**, N=8 **56**. Default 32 no aloja siete agentes de una malla completa por relay bajo el mismo origen. En `home`, siete de ocho detrás de NAT1 necesitan **49** en esa IP.

**MEDIDO**, [investigación multipar](caidas-y-leases.md#investigación-multipar-posterior), 2026-10-25, histórica pre-v2: N=8 bloqueado con default32 produjo **44 rechazos Full**; con `ATENEA_RELAY_MAX_PER_IP=64`, **28/28** pares y **56** asignaciones UDP, cero rechazos. LOCAL-V2 del mismo día ([arnés](../../tests/network-lab/run-multi-peer.sh), salida no versionada `/tmp/v2-multi8.txt`) completa **28/28** bloqueados en **63,2 s** con 64. Subir ese valor no aumenta TCP_PER_IP ni elimina cuotas COOKIE/ALLOC/coordinator: no es una recomendación de subir cuotas sin presupuestar antiabuso. El establecimiento multipar no sustituye el ensayo de carga/soak.

### Leases y cortes

Perfil piloto: **300 s de lease / 60 s de sondeo de renovación / 30 s de tolerancia**. El tráfico falla cerrado al alcanzar **`valid_until + 30 s`**, no dispone de 330 s nuevos desde cada fallo. Tiempo máximo nominal desde la última emisión: **330 s**; depende de cuánto lease quedaba al caer. Un reloj anómalo también puede denegar. Fuente: [caídas y leases](caidas-y-leases.md), [ADR-009](../architecture/ADR-009-operacion-y-releases.md).

**MEDIDO histórico pre-v2**, 2026-10-25, no repetido tras el cutover: cuatro caídas largas CP/BD del perfil piloto; primer KO enviado respecto al lease mínimo media **30,135 s**, peor **30,170 s**. Son estimaciones por sondas con hasta un segundo de timeout, no exactitud física subsegundo. Recuperación tras start: CP media/peor **30,060/32,208 s**, BD **28,511/28,516 s**. Revocación pedida después de volver: **59,879 s**. Hace falta revalidar este simulacro en v2; no se presenta como medición nueva.

**MEDIDO LOCAL-V2**, 2026-10-25, [lab base](../../tests/network-lab/run.sh), salida no versionada `/tmp/v2-tests_network-lab_run_sh.txt`: perfil reducido **40/10/30 s**, peer corta en **3 s**, dispositivo aislado falla cerrado a los **70 s** de la última configuración. No extrapolar esos 3 s al piloto.

### Recuperación: RPO/RTO

Objetivos **del piloto**, aceptados con [ADR-009](../architecture/ADR-009-operacion-y-releases.md) el 2026-10-26: RPO ≤24 h / RTO ≤2 h. No son un compromiso de servicio y aún no están medidos en un VPS. [Medición de restore](copias-y-recuperacion.md#mediciones-reales-2026-10-25), 2026-10-25, checkout base `b31de5621b0a3676b1ef137361fc01152c0455e4` con cambios de fase 9 sin commit; dos ciclos, datos pequeños, esquema 0001–0003:

| Escenario | Media | Peor caso |
| --- | ---: | ---: |
| Último WAL: RTO hasta healthz + configuración aceptada por dispositivo | **3.106 s** | **3.293 s** |
| Último WAL: RPO | **0 tx / 0 s** | **0 tx / 0 s** |
| PITR anterior al DELETE: RTO | **3.088 s** | **3.097 s** |
| Sin WAL posterior: pérdida | **3 tx / 1.543 s** | **3 tx / 1.630 s** |
| Recuperar emisor, emitir y aceptar en agente | **0.367 s** | **0.383 s** |

Hardware: **Mac17,4 arm64, 24 GiB RAM, 10 CPU lógicas**, Docker Desktop servidor **29.7.2 arm64**. **No es un VPS.** RPO cero se midió tras forzar archivado/check del WAL, no ante caída arbitraria. RTO excluye provisionar nodo, DNS, obtener copia remota y aprobación humana; no mide datos de producción. Sigue sin probarse pérdida simultánea del nodo y repositorio ni recuperación del esquema actual completo en proveedor. Un RTO menor de dos horas no evita el corte al expirar leases.

Límites medidos de rendimiento, solo en Docker 29.7.2 sobre Mac arm64 (**no equivalen a un VPS**; detalle y reproducción en [rendimiento y presupuestos](rendimiento-y-presupuestos.md)): túnel directo ≈1 Gbit/s y por relay ≈21,6 Mbit/s; un relay satura un núcleo hacia 41–55 Mbit/s agregados; 4096 asignaciones simultáneas por relay; coordinator medido hasta 10 000 presencias (≈26 KB de RSS por presencia); control plane con 10 000 dispositivos y 200 clientes ≈11 emisiones/s con p95 ≈19,5 s (**no es una experiencia interactiva aceptable a esa escala**; para el piloto de decenas a cientos de dispositivos por red no se espera saturación, no medido). Reconexión tras reiniciar el agente: hasta ≈25 s medida. La sesión de relay se recrea cada ≈300 s con un microcorte medido ≤0,41 s (hallazgo conocido, causa inferida: vida del token; mejora propuesta *make-before-break*, no implementada). Soak local de 6 y 12 min sin errores en estado estable salvo ese ciclo conocido; **el soak de 72 h no se ha ejecutado**: corresponde al piloto (`bash tests/load/soak.sh 72`).

## Actualización y rollback

Seguir [ADR-009 §7](../architecture/ADR-009-operacion-y-releases.md#7-actualización-y-vuelta-atrás), [stack](stack-produccion.md#actualización-y-rollback), [infraestructura](infraestructura.md#actualización-y-rollback) y [release](release.md):

1. Verificar firma del manifiesto contra clave pública obtenida por canal independiente, commit/versión esperados, hashes y digests. Guardar manifiesto, mapa de digests y configuración de N; hacer copia de BD/secretos y comprobar que se puede restaurar.
2. Migrar primero con rol migrador y binario de N+1, manteniendo N en servicio. Migraciones **expand/contract**: solo aditivas en esta ventana; **N debe funcionar sobre el esquema de N+1**. No se exige N+1 sobre esquema sin migrar. Ante checksum/migración desconocida, detenerse, no borrar historial SQLx.
3. Sustituir imágenes por digests N+1 y esperar salud; comprobar login, emisión y tráfico de un dispositivo ya enrolado, no solo healthz. No mezclar con una rotación incompatible de claves/configuración.
4. Si falla, restablecer **digests N**, no una BD vieja; no usar down migrations destructivas. Comprobar datos y escrituras producidas por N+1. El rollback de imagen no deshace secretos ni configuración pública.
5. Eliminar columnas/tablas solo en una versión posterior cuando ninguna versión todavía desplegable las use. Un rollback fuera de esa ventana necesita plan de datos propio, no revertir a ciegas.

Ensayo local del 2026-10-25, [upgrade](../../tests/stack/upgrade.sh): **39 s**, N sobre esquema ampliado y rollback conservando **cinco escrituras** y DNS N+1 (`/tmp/v2-upgrade.txt`, no versionado). La salida identifica N=`bb6d63d538e64021d78d64a77012a4ebe4b53e18` y N+1 **working-tree HEAD `954a50393b5f56061788052902e896593e601144`**, no el corte documental `d9ab7ec`; no demuestra identidad entre esos árboles. No certifica actualizar paquetes nativos firmados. Para clientes conservar identidad, configuración y estado antirrollback; seguir [Linux](instalacion-linux.md), [macOS](instalacion-macos.md), [Windows](instalacion-windows.md). Windows exige parar/desinstalar conservando datos antes de reinstalar; no forzar muerte/borrado si SCM no consigue detenerlo.

## Recuperación de datos y claves

### Base de datos y nodo

Aplicar [copias y recuperación](copias-y-recuperacion.md), no inicializar sobre datos que se van a recuperar ni ejecutar `down -v`. Aislar nodo anterior; cerrar escrituras; recuperar repositorio y secreto desde custodia separada; restaurar en volumen nuevo con versión compatible; verificar migraciones, integridad, conteos y una configuración aceptada por agente. Tras PITR, elevar emisión por encima del máximo conocido: **no rebajar versiones/seriales ni borrar estado de agentes**. Reanudar WAL en la timeline nueva y realizar completa antes de reabrir escrituras.

El repositorio pgBackRest por defecto está en el mismo nodo: no protege de perderlo entero. Antes de producción faltan destino off-node real, transferencia verificada y ensayo desde fuera del nodo. Conservar copia cifrada y secreto en ubicaciones independientes.

### Emisor y firma de tokens de relay

- Pérdida accidental: detener control plane, descifrar copia mediante `keys-restore.sh COPIA.age IDENTITY DESTINO_NUEVO` en estación aislada y directorio nuevo. El runbook valida nombres/rutas, usa directorio 0700 y ficheros 0600; el montaje productivo usa propietario/mode del servicio (0400 en Compose). Reponer emisor solo al control plane; `relay-signing.hex` es semilla privada del **coordinator**, no una clave E2E del relay. Los relays reciben sus claves **públicas** de verificación y clave TLS propia.
- Comprobar certificado original, ámbito, serial y vigencia; emitir con `atenea issue` y probar aceptación con ancla/estado existentes, después emisión por control plane. Eliminar material temporal y retirar identidad age; nunca guardarla junto a la copia ni en nodo.
- Sospecha de compromiso del emisor: **no restaurar y reutilizar la misma clave**. Desde raíz offline emitir emisor nuevo con serial mayor, distribuirlo y comprobar aceptación. Un agente aislado que no vio el serial nuevo puede confiar en el anterior hasta su caducidad; revocar no es instantáneo.
- Pérdida de semilla del coordinator: restaurar la copia en ese servicio y comprobar tokens de ambos relays. Si se comprometió, sustituirla y distribuir la clave pública nueva a los relays de forma coordinada; considerar aún válidos los tokens anteriores mientras se confíe en su clave/vigencia. No prometer revocación inmediata ni continuidad sin ensayo de la rotación. Conservar `relay_id` persistente, único y coincidente en relay/coordinator: no deriva del certificado, rotar TLS no lo cambia; cambiar audiencia exige tokens nuevos.

### Raíz y dispositivo

[ADR-005](../architecture/ADR-005-custodia-clave-raiz.md): raíz **fuera de todos los nodos**, laboratorio con age/scrypt y dos copias cifradas offline. Elección del token hardware pendiente del responsable antes del piloto. Dos destinatarios age son recuperación **OR**, no umbral ni doble aprobación.

La rotación prevista usa dos anclas, actual y siguiente, distribuidas bajo la raíz actual; el procedimiento end-to-end debe ensayarse antes de usarlo. Ante pérdida/compromiso de raíz no hay recuperación automática: generar raíz nueva en estación segura, reconstruir confianza y **reincorporar manualmente cada dispositivo por canal autenticado**. No introducir la raíz en el control plane ni desactivar pinning para recuperar servicio. La pérdida de identidad privada de un dispositivo también exige revocar la identidad anterior y enrolar una nueva; no copiar claves entre dispositivos.

### Reloj adelantado

Corregir reloj/NTP y comprobar la causa; parar el agente. Usar, con la ruta privada de configuración de la plataforma:

```sh
atenea-agent reset-clock --config /etc/atenea/agent.toml
```

[Semántica](../../apps/agent/README.md#transporte-estado-y-recuperación): borra **solo hora máxima observada**, conserva versión, serial y anclas. Reiniciar y comprobar configuración legítima vigente. No borrar `state.bin`, rebajar versión ni reemplazar trust anchors para eludir un rechazo. Un reloj fuera de tolerancia debe fallar cerrado, no habilitar tráfico indefinidamente.

## Revocación con control plane caído

Revocar en panel/API impide nuevas emisiones y retira el peer de configuraciones posteriores. Los agentes aplican autorización local firmada; ni panel desconectado ni coordinator sustituyen al emisor. Si control plane o BD caen, no hay renovación ni revocación online; **las peticiones fallidas no quedan encoladas**. Registrar la acción pendiente en el procedimiento del operador y ejecutarla explícitamente al recuperar, verificando estado y corte real. No anunciar éxito por pulsar un botón sin respuesta de API.

El tráfico establecido continúa únicamente hasta `valid_until +30 s`; a partir de ahí se deniega. Un coordinator caído impide nuevos rendezvous/tokens, pero no borra de inmediato sesiones ya autorizadas; tampoco garantiza renovación indefinida de relay. La recuperación de salud no demuestra que todos los pares hayan reconectado. Ante recreación de `netns`, recrear juntos HAProxy/relay; instalación solo Compose sin la unidad de ordenación no garantiza recuperación del motor: [stack](stack-produccion.md#reinicio-del-motor-y-recuperación-declarativa).

## Datos guardados y retención

- **PostgreSQL**: usuarios OIDC, organizaciones/membresías, redes, anclas/certificados **públicos**, bundles públicos/dispositivos, asignaciones, rutas/políticas, hashes de tokens/sesiones, versiones de configuración y auditoría. No guarda contraseñas OIDC ni privadas de dispositivo. Flujo PKCE/sesión tiene estado transitorio sensible; no publicar dumps. [Modelo de datos](../api/control-plane-v1.md).
- **Auditoría**: actor, fecha, organización/red, acción, objetivo y detalle; rol de aplicación solo inserta, sin UPDATE/DELETE. No hay plazo automático de borrado de auditoría/versiones documentado: persisten en BD y copias hasta política administrativa explícita. **Siete días de métricas no significa siete días de auditoría.** Sesiones/flujos OIDC caducados se purgan cada minuto; retención lógica y eliminación física/backups no equivalen.
- **Coordinator/relay**: presencia, nonces y asignaciones efímeros; claves privadas online en ficheros del servicio, no en logs. Agente conserva identidad local y estado antirrollback; desinstalar deliberadamente no destruye claves/configuración/estado.
- **Observabilidad**: Prometheus y Loki **7 días**, Docker **3 × 10 MB por servicio**, sin envío SaaS. Compactación no promete borrado instantáneo. Sin IP/red/dispositivo como etiquetas; métricas y listeners internos, nunca publicarlos por Caddy/HAProxy. Mantener `RUST_LOG=info`; trazas de bibliotecas, cabeceras y capturas pueden filtrar datos. Fuente: [observabilidad](observabilidad.md#logs-y-retención).
- **Copias**: siete completas diarias, catorce diferenciales si se lanzan; incrementales ligadas a completa y WAL necesario desde la más antigua. Copia local no reemplaza off-node; la retención completa no se ha observado durante siete días de reloj. Ver [copias](copias-y-recuperacion.md).

## Soporte y divulgación responsable

Para incidencias no sensibles, aportar versión/commit, SO/arquitectura, camino, resultado esperado/real y diagnóstico **sanitizado**; no adjuntar `join`, tokens, claves, dumps, capturas o inventarios reales a un issue público. No hay canal de guardia ni plazo contractual de respuesta acreditado.

Vulnerabilidades: seguir [SECURITY.md](../../SECURITY.md), **Security → Report a vulnerability** del repositorio en GitHub (aviso privado), no issue/PR público con detalles explotables. Incluir revisión, reproducción e impacto y coordinar divulgación/arreglo por ese canal; no se inventa correo ni garantía de tiempo de respuesta. Las excepciones de dependencias vencen para revisión **antes de 2027-01-23**, y el resto de límites/riesgos aceptados permanece en [modelo de amenazas](../security/threat-model.md) y [matriz](matriz-de-aceptacion.md#hallazgos-abiertos-y-riesgos-aceptados). La auditoría independiente sigue siendo bloqueante aunque las pruebas internas pasen.
