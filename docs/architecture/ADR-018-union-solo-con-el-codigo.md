<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-018: Unirse con solo el código (conectividad en el código, ancla automática y `--up`)

- **Estado:** **Aceptado** por el responsable el 2026-11-03 con las tres recomendaciones de § 8. Implementadas y probadas en laboratorio las fases 1 a 7; la 8 (piloto) y la release v0.1.9 esperan su orden.
- **Relacionados:** ADR-013 (redes múltiples y unión por código; §12 añadió `--stun-host`/`--relay-ca`), ADR-005 (custodia de la raíz), ADR-009 (operación), ADR-016 (políticas mínimas), [código de red v1](../protocol/join-code-v1.md), [coordinator v1](../protocol/coordinator-v1.md).

## 1. Contexto

La meta del producto es: instalar el paquete, ejecutar `sudo atenea network join <CÓDIGO>`, aprobar el dispositivo y que funcione. En el piloto real (2026-11), unir tres máquinas a dos redes nuevas exigió además pasos manuales que **no** son decisiones de diseño sino huecos:

| Hueco observado | Dónde está hoy |
| --- | --- |
| `join` no escribe `[coordinator]`; hay que pasar `--stun-host` y `--relay-ca` (v0.1.8) | El código de unión (`url`, `network_id`, `root_anchor`, `name`, token opcional; `crates/identity/src/join_code.rs`) no lleva ni el STUN ni la CA del relay. |
| El coordinator rechaza (`403 invalid_config`) a una red nueva hasta que el **operador edita `coordinator.toml`** (`anchors`) y reinicia el contenedor | `apps/coordinator/src/lib.rs` valida cada anuncio con `inspect_config(&file, &s.anchors, now)`; `anchors` sale solo de la configuración estática. |
| `join` y `up` son dos pasos | `network join` termina diciendo «Siguiente paso: sudo atenea network up …». |
| Copiar a mano la CA privada del relay a cada equipo | El relay del piloto usa «Atenea Stack CA», una CA propia. |

Lo que **no** es un hueco y se conserva: la aprobación del dispositivo por huella (ADR-013), la raíz propia de cada red custodiada fuera de línea (ADR-005) y las políticas de acceso mínimas (ADR-016).

## 2. Decisión

1. **El código de unión lleva la conectividad.** Se añade un bloque opcional con el **host STUN** y el **SHA-256 de la CA del relay**. La CA en sí **no** viaja en el código (≈530 bytes de PEM no caben en el límite de 512 caracteres): el agente la descarga del control plane y la **valida contra el hash del código**.
2. **`atenea network join` escribe `[coordinator]` y la CA solo con el código**, y acepta `--up` para arrancar la red al terminar. Las opciones `--stun-host` y `--relay-ca` de la v0.1.8 **se conservan** y tienen prioridad sobre el código (el operador manda).
3. **El coordinator obtiene las raíces de confianza del control plane**, además de las estáticas, sin reiniciarse. Es **opt-in** (`anchors_source`) y **desactivado por defecto**.
4. **No cambia:** la aprobación manual por huella, el formato firmado de la configuración, el protocolo del relay/coordinator, las políticas ni la emisión del certificado de emisor.

## 3. Detalle

### 3.1 Código de unión (formato)

- Mismo prefijo `ATENEA1` y `version = 1`. Se definen **dos bits de `flags`**: el **bit 1** («lleva el bloque de conectividad») y el **bit 2** («el bloque lleva además el hash de la CA»). Hoy el decodificador rechaza cualquier bit reservado (`ReservedFlags`), así que **un CLI antiguo rechaza un código nuevo con un error claro** en lugar de interpretarlo mal. El bit 2 sin el bit 1 es inválido.
- Si el bit 1 está activo, tras el nombre y antes del token: `len_stun(1) stun_host [| relay_ca_sha256(32)]` (el hash solo con el bit 2: así un relay con CA pública no lo necesita). `stun_host`: 1..=100 bytes de `A–Z a–z 0–9 . -` (nombre o IPv4, sin puerto; los puertos son 3478 y 3479, como exige el agente), **la misma regla que `--stun-host` del CLI**. El checksum sigue cubriendo todo el payload.
- Longitud: se mantiene el máximo de **512 caracteres** del texto canónico (con guiones). Con el bloque el peor caso no cabe, así que `encode` devuelve `TooLong` en lugar de emitir un código que el decodificador rechazaría, y **`decode` rechaza también lo que `encode` no emitiría** (invariante del fuzz: lo aceptado se re-emite y vuelve igual). Medido: el vector completo (URL de 28 bytes, nombre de 14, STUN de 18, token de 22 y hash de la CA) ocupa 343 caracteres; solo STUN, 227.
- **Compatibilidad:** un código sin los bits nuevos es byte a byte idéntico al de la v0.1.8 (los vectores existentes no cambian).
- El contrato `join-code-v1.md`, los vectores de prueba y las semillas del fuzz (`fuzz/seeds/join_code_parse`) se actualizan con la fase 1.

### 3.2 CA del relay

- Nuevo endpoint público de solo lectura `GET /api/v1/relay-ca` (PEM canónico de un único certificado, `Cache-Control: public, max-age=300`). El control plane lo sirve desde un fichero (`relay_ca_file`). **El interruptor del bloque es `stun_host`**: sin él, el servidor **no** emite el bloque (código idéntico al de la 0.1.8); con él y sin `relay_ca_file`, el bloque lleva solo el STUN (relay con CA pública) y `relay-ca` responde `404`. `relay_ca_file` sin `stun_host` es un error de configuración, y **el servidor no arranca** con una configuración a medias o ilegible. Implementado en la fase 2 (v0.1.9); detalle en [control-plane-v1 §17](../api/control-plane-v1.md#17-conectividad-en-el-código-de-unión-adr-018-fase-2).
- El agente descarga la CA **por HTTPS al servidor del código**, calcula `SHA-256` del certificado en DER y **solo la acepta si coincide con el hash del código**; si no, `join` falla antes de escribir nada. Así la confianza sigue anclada en el canal del código, como la raíz (sin TOFU) y no en el servidor.
- Se guarda en `<red>/device/relay-ca.pem` (0644, propiedad de root) y `relay_ca_file` apunta a ella: una CA por red, sin ficheros compartidos entre redes.
- **Alternativa de despliegue sin esta pieza:** si el relay usara un certificado de una CA pública, no haría falta CA ni hash (el bloque llevaría solo el STUN).

### 3.3 `join --up`

`--up` ejecuta lo mismo que `network up` al terminar con éxito (instancia iniciada, «sin configuración» hasta que se apruebe). Sin `--up` el comportamiento no cambia. No espera la aprobación.

### 3.4 Anclas automáticas en el coordinator

- Nuevo punto de lectura **interno** en el control plane: `GET /internal/v1/root-anchors` (el `root_anchor` en hex de cada red existente, **con su `network_id`**), en un **listener distinto y no publicado** (como `metrics_listen`), con `Authorization: Bearer` de un secreto de fichero compartido solo con el coordinator. **No se sirve por el Caddy/haproxy públicos** (hoy todo lo que no es `/v1/*` ni `/dex/*` va al control plane: una ruta `/internal/…` en el listener público quedaría expuesta).
- El coordinator consulta ese punto cada 60 s con `anchors_source = { url, secret_file, interval_secs }`, y valida cada anuncio contra las estáticas y **la raíz remota de la red que declara** (hasta la `0.1.25` usaba la unión de todas para cualquier red, lo que contradecía el § 4: quien registraba una red podía anunciar dispositivos de otra). Falla **estática**: si el control plane no responde, conserva la última lista buena; nunca baja a «sin anclas». Las estáticas siguen exigiéndose (≥ 1).
- Si `anchors_source` no está configurado, el comportamiento es el actual.

## 4. Amenazas y por qué es aceptable

El cambio de § 3.4 **quita una puerta que hoy cierra el operador**: hoy una red nueva no puede usar el coordinator ni emitir tokens de relay hasta que él añade su ancla; con la lectura automática, **cualquiera que pueda crear una red en el control plane** (el API acepta la raíz que aporta quien crea la red) obtiene rendezvous y relay para ella.

- **Qué NO permite:** no concede acceso a redes ajenas (cada anuncio se valida contra la raíz de **su** red y la configuración firmada), ni aprueba dispositivos, ni cambia políticas.
- **Qué sí:** consumo de recursos del coordinator y del relay por redes que el operador no ha revisado (denegación de servicio por reparto injusto, no por intrusión). Mitigaciones existentes: cupos del coordinator/relay **por `network_id`** y límites de tasa; mitigación nueva: la función es **opt-in** y se documenta que se active solo con creación de redes restringida a personas de confianza (hoy: un único propietario).
- **El control plane ya es la autoridad** de configuración de las redes que sirve; el coordinator ya depende de él para el lease. No añade una dependencia de confianza nueva, sí un **canal nuevo** (de ahí el listener separado y el secreto).
- La descarga de la CA (§ 3.2) añade una petición no autenticada de solo lectura de un certificado público: sin información sensible.
- No se añade ninguna primitiva criptográfica: SHA-256 (ya en el árbol de dependencias) para comparar un hash de certificado.

## 5. Alternativas rechazadas

- **Poner la CA completa en el código:** no cabe en 512 caracteres y alargaría un texto que se copia a mano.
- **Que el agente tome STUN/URL del servidor sin hash en el código:** daría al servidor el poder de elegir a qué STUN y a qué CA se confía; hoy no lo tiene (ADR-013 §12).
- **Aprobación automática de dispositivos:** debilita el control más importante del modelo; ya existe el código con token de un solo uso para quien quiera ese equilibrio.
- **Anclas en el coordinator por consulta directa a PostgreSQL:** acopla el coordinator al esquema y a una credencial de base de datos; el punto de lectura mínimo es más fácil de auditar.
- **Subir `version` a 2 / prefijo `ATENEA2`:** obligaría a mantener dos decodificadores y a reemitir todos los códigos; el bit reservado ya garantiza el rechazo limpio de los antiguos.

## 6. Compatibilidad y retroceso

| Combinación | Resultado |
| --- | --- |
| CLI nuevo + código sin bloque (servidor sin configurar) | Como la 0.1.8: `--stun-host`/`--relay-ca` explícitos, o aviso de «sin `[coordinator]`». |
| CLI antiguo + código con bloque | Error claro (`el código usa indicadores reservados`). El panel/API ofrece el código **sin bloque** con `?conectividad=0` para quien no pueda actualizar. |
| Coordinator nuevo sin `anchors_source` | Idéntico al actual. |
| Servidor nuevo, agentes antiguos | Sin cambios: el formato firmado y los endpoints del agente no varían. |
| Retroceso | Desconfigurar `anchors_source` y la CA en el servidor; el CLI antiguo sigue funcionando con códigos sin bloque. No hay migraciones de base de datos (la lista de anclas se lee de la columna existente `networks.root_anchor`). |

## 7. Plan de implementación (v0.1.9)

Orden por dependencia; cada fase termina verde (`fmt`, `clippy -D warnings`, tests) antes de la siguiente.

| Fase | Contenido | Ficheros principales | Prueba mínima |
| --- | --- | --- | --- |
| 1. Formato del código | Bit de conectividad, `encode`/`decode`, errores, vectores y documento | `crates/identity/src/join_code.rs`, `docs/protocol/join-code-v1.md`, `fuzz/` | Ida y vuelta con/sin bloque y con token; rechazo de bit reservado sin el nuevo; longitud máxima; el fuzz no entra en pánico |
| 2. Control plane | Ajustes `stun_host` y `relay_ca_file`, bloque en `join_code`, `GET /api/v1/relay-ca`, parámetro `?conectividad=0`, OpenAPI | `apps/control-plane/src/{config,networks,api,openapi}.rs`, `docs/api/openapi.json` | Integración contra PostgreSQL real: código con y sin bloque, endpoint de CA, `409` si el STUN no cabe |
| 3. CLI | Descarga y comprobación de la CA, escritura de `[coordinator]` desde el código, precedencia de las opciones, `--up` | `apps/cli/src/{enroll,network}.rs` | CA con hash distinto ⇒ error **sin dejar nada en disco**; sin bloque ⇒ comportamiento 0.1.8; `--up` arranca; opciones explícitas ganan |
| 4. Coordinator | `anchors_source`, sondeo, unión y falla estática | `apps/coordinator/src/{config,lib}.rs`, `docs/protocol/coordinator-v1.md` | Añadir una red ⇒ se acepta sin reinicio; control plane caído ⇒ se mantiene la última lista; secreto erróneo ⇒ no cambia nada |
| 5. Despliegue | Listener interno no publicado, secreto por fichero, montaje de la CA en el control plane, variables de Ansible/Compose, comprobación en `tests/infra/check.py` de que `/internal/` **no** responde por el Caddy público | `deploy/compose/prod/`, `infra/ansible/`, `Caddyfile`, `tests/infra/` | Ensayo de infraestructura principal y relay; subida 0.1.8 → 0.1.9 con rescue |
| 6. Panel y app | El diálogo de unión muestra `atenea network join <CÓDIGO> --up` y deja de pedir STUN/CA | `apps/web/app/join-network.tsx`, `apps/desktop/` (texto de ayuda) | Tests del panel; e2e del panel |
| 7. Laboratorio extremo a extremo | Crear red nueva, unir un dispositivo **solo con el código**, sin tocar ningún fichero del servidor | `tests/network-lab/run-two-networks.sh` (ampliado) | `DOS REDES: TODO OK` con la unión sin opciones |
| 8. Piloto | Red desechable: `join <CÓDIGO> --up` con un dispositivo real, aprobar, comprobar `direct`/relay y revocar con `leave` | (operación) | Observado y anotado en `v0.1.9.md` |

**Aceptación de la v0.1.9:** (a) en un servidor con la opción activada, unir un equipo a una red **recién creada** requiere solo instalar, `join <CÓDIGO> --up` y aprobar; (b) un servidor sin configurar se comporta como la 0.1.8; (c) `/internal/` no es alcanzable desde Internet (probado); (d) los CLI antiguos fallan con el error claro; (e) sin datos reales en el repo y con la documentación al día.

**Release:** el procedimiento habitual (ensayo manual del workflow, tag firmado, `verify.sh`, despliegue con copia completa previa, agentes actualizados), cada paso con autorización expresa. **Estimación de tamaño:** media; las fases 1–4 son código y pruebas, la 5 es la de más riesgo operativo.

## 8. Decisiones del responsable (2026-11-03)

Las tres **recomendaciones** quedaron aceptadas tal como se redactaron:

1. **Anclas automáticas (§ 3.4): sí, opt-in y apagadas por defecto**, activadas en el piloto. El operador debe activar `anchors_source` de forma explícita, y se documenta que solo se use con la creación de redes restringida a personas de confianza.
2. **CA del relay: hash en el código y descarga (§ 3.2)**, sin cambiar la infraestructura del relay. Pasar a una CA pública queda descartado por ahora (exigiría un certificado válido para el nombre del relay y su renovación).
3. **`--up`: solo bajo petición.** Sin la opción, `join` no arranca la red, como hasta ahora.

## 9. Fuera de alcance

Aprobación automática, cambios en el relay o en el protocolo de datos, Windows (ADR-017), descubrimiento de servidores sin código, y la interfaz de la aplicación de escritorio más allá del texto de ayuda.

## 10. Estado de la decisión

**Aceptado** el 2026-11-03 (§ 8). Fases 1–7 implementadas. Fase 6: el panel enseña `sudo atenea network join '<CÓDIGO>' --up` (también con el código con token) y lo que anuncia el código, o avisa de que hacen falta `--stun-host`/`--relay-ca` si el servidor no anuncia conectividad; la app de escritorio muestra la conectividad en la confirmación y sigue funcionando con un CLI 0.1.8 (campo ausente). **Código sin bloque en el panel (2026-11-03, tras la publicación de la 0.1.9):** el código de red y el formulario del token tienen una casilla «Código para un equipo con un CLI anterior a la 0.1.9 (sin conectividad)» que pide `?conectividad=0` y enseña el comando de unión de un CLI 0.1.8 (`--stun-host`, sin `--up`) y el paso `network up` aparte. **Va en el panel de la versión siguiente a la 0.1.9**: el panel se sirve desde la imagen del control plane y la 0.1.9 publicada no lo lleva. Fase 7: `tests/network-lab/two-networks.sh` une dos dispositivos a una red creada después de arrancar el coordinator solo con el código, con control negativo sin `anchors_source` (ver el README del laboratorio): 159/159. Fase 8 (piloto) pendiente. No se publica nada sin la autorización expresa del responsable.
