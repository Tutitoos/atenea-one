# Control plane

Monolito Axum y PostgreSQL: OIDC, sesiones, organizaciones, RBAC, IPAM, enrolamiento y emisión de configuraciones firmadas. Contrato: [`control-plane-v1`](../../docs/api/control-plane-v1.md). No almacena claves privadas de dispositivos ni la clave raíz.

## Configuración y arranque

PostgreSQL 17 es la versión del entorno de integración. Termine TLS en un proxy de confianza y mantenga el puerto de PostgreSQL fuera de Internet. Registre en el proveedor OIDC el callback exacto y habilite Authorization Code; MFA corresponde al proveedor.

Puede usar un TOML indicado por `ATENEA_CP_CONFIG`; sus claves son los nombres de la tabla en minúsculas. Las variables `ATENEA_CP_*` prevalecen sobre el fichero.

| Sufijo de variable | Valor / función |
| --- | --- |
| `DATABASE_URL`, `DATABASE_URL_FILE` | URL PostgreSQL del rol de aplicación, obligatoria; alternativa por fichero de secreto |
| `LISTEN` | `127.0.0.1:8080` |
| `METRICS_LISTEN` | `127.0.0.1:9101`; `GET /metrics` en listener separado sin autenticación; nunca publicar/proxificar. [Operación](../../docs/operations/observabilidad.md) |
| `PUBLIC_URL` | URL pública que se entrega en `join`; valor local predeterminado `http://127.0.0.1:8080` |
| `OIDC_ISSUER` | URL del emisor; discovery y JWKS validan los ID tokens |
| `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `OIDC_CLIENT_SECRET_FILE` | Cliente OIDC registrado; secreto directo o por fichero |
| `OIDC_REDIRECT_URL` | URL pública exacta de `/auth/callback` |
| `ISSUER_KEY_FILE` | Fichero regular **0400 o 0600**, sin permisos de grupo/otros, semilla hex del emisor (32 bytes) |
| `CONFIG_VALIDITY_SECS` | `300`; máximo `3600`, coherente con el lease del agente |
| `DEV_INSECURE_COOKIES` | `false`; `true` solo para HTTP local, emite aviso |
| `ENROLL_PER_MINUTE` | `10`; token bucket por IP de origen |
| `LOGIN_PER_MINUTE` | `20`; intentos de `GET /auth/login` por IP cliente (`429 rate_limited`); además máximo 10 000 flujos OIDC vigentes |
| `DEVICE_PER_MINUTE` | `30`; peticiones firmadas por minuto y dispositivo (`config`, `status`, `routes`), comprobado tras verificar firma y antes de bloquear filas |
| `TRUSTED_PROXIES` | Lista de CIDR, p. ej. `["172.30.84.2/32"]`; vacía por defecto. Solo si el peer TCP pertenece a ella se usa `X-Real-IP` como IP cliente; en otro caso se ignora toda cabecera |
| `APPLICATION_ROLE` | Rol que recibe permisos limitados al aplicar migraciones |
| `WEB_DIR` | Opcional: directorio `apps/web/build/client`; sirve SPA y assets desde este mismo origen |

**IP cliente tras proxy.** El despliegue de referencia (HAProxy → Caddy → control plane) pasa la IP real con PROXY v2 y Caddy fija `X-Real-IP` con `header_up`, sustituyendo cualquier valor del cliente. La red interna dedicada `proxy` usa `172.30.84.0/29`: HAProxy ocupa `.3` (único origen PROXY v2 admitido por Caddy) y Caddy `.2` (único proxy admitido por el control plane). Los aliases `caddy-proxy` y `control-plane-proxy` pertenecen solo a esa red. Si el rango se solapa con el entorno, ajuste conjuntamente `ATENEA_PROXY_SUBNET`, `ATENEA_HAPROXY_PROXY_IP` y `ATENEA_CADDY_PROXY_IP` antes de actualizar el despliegue; las listas de confianza siguen esas direcciones exactas, no rangos privados completos. Fuera de compose configure `TRUSTED_PROXIES` con los peers concretos del proxy. Sin proxy confiable, todas sus peticiones comparten el cubo del peer TCP.

**IdP y MFA.** MFA corresponde al proveedor OIDC: producción exige un IdP con MFA. El Dex del ejemplo usa contraseña estática de un factor y no demuestra MFA; el control plane no comprueba `amr`/`acr`.

**Higiene en memoria y BD.** Cachés de nonces y cubos de tasa limitadas a 65 536 entradas (al llenarse se rechaza con `429`, sin expulsar nonces vigentes). El muestreador purga cada minuto `oidc_flows` y `sessions` caducados. `config.issue` se audita como máximo una vez por dispositivo y hora; cada emisión queda en `configuration_versions`.

**CIDR de red (F7).** `0004_network_cidr_private.sql` añade CHECK `NOT VALID` (privados ADR-007, IPv4 ≥ /8, IPv6 ≥ /32): se aplica a altas, sin invalidar redes existentes. Para validar las existentes (ADR-009 §7) ejecute `ALTER TABLE networks VALIDATE CONSTRAINT networks_ipv4_private` y `networks_ipv6_private`; falla sin modificar datos si hay una red fuera de rango, que debe migrarse manualmente. Mientras tanto, la emisión rechaza con `409 invalid_network_prefix` una red heredada inválida.

`DATABASE_URL_FILE` y `OIDC_CLIENT_SECRET_FILE` leen el fichero UTF-8 y eliminan únicamente CR/LF finales; un fichero vacío o ilegible impide arrancar, sin incluir su contenido en el error. La variable directa prevalece si se configura también; en contenedores configure solo `*_FILE`, con propietario UID del servicio y modo 0400.

Ejemplo sintético (no incluya secretos reales en el repositorio):

```sh
export ATENEA_CP_DATABASE_URL='postgres://atenea_app:CAMBIAR@127.0.0.1/atenea'
export ATENEA_CP_PUBLIC_URL='https://control.example.test'
export ATENEA_CP_OIDC_ISSUER='https://login.example.test'
export ATENEA_CP_OIDC_CLIENT_ID='atenea-control-plane'
export ATENEA_CP_OIDC_CLIENT_SECRET='CAMBIAR'
export ATENEA_CP_OIDC_REDIRECT_URL='https://control.example.test/auth/callback'
export ATENEA_CP_ISSUER_KEY_FILE='/ruta/privada/issuer.key'
RUST_LOG=info cargo run -p atenea-control-plane
```

`GET /healthz` comprueba la conexión a la base. Los registros no deben contener tokens, cookies, claves ni cuerpos de configuración. Use cookies seguras en producción: `HttpOnly`, `Secure`, `SameSite=Lax`; sesión absoluta de ocho horas e inactividad máxima de una hora. Para escribir con sesión, obtenga `csrf_token` de `GET /api/v1/session` y envíelo en `X-CSRF-Token`.

## Migraciones y permisos

El arranque normal **no** migra. Aplique SQLx con una conexión administrativa propietaria del esquema y `--migrate`; `APPLICATION_ROLE` concede al rol de aplicación los permisos necesarios y le quita `UPDATE`, `DELETE` y `TRUNCATE` de auditoría. No ejecute el servicio habitual como propietario ni superusuario: estos eluden la protección de permisos.

```sh
ATENEA_CP_DATABASE_URL='postgres://migrador:CAMBIAR@127.0.0.1/atenea' \
ATENEA_CP_APPLICATION_ROLE=atenea_app \
cargo run -p atenea-control-plane -- --migrate
```

`--migrate` aplica las migraciones y termina, sin arrancar el servidor. Arranque después el servicio con la URL del rol limitado. El servicio rechaza roles con permisos de modificación destructiva de auditoría. El administrador de PostgreSQL debe configurar separadamente la contraseña/autenticación del rol; no publique ese secreto. Las migraciones versionadas están en `migrations/`. Haga copia y ensaye restauración antes de actualizar producción.

El release actual aplica **0001–0005**. Según ADR-009 §7, migre **antes de desplegar el binario nuevo**, con N todavía sobre el esquema ampliado de N+1; el rollback cambia binarios, no deshace el esquema. `0005_ip_allocations_device_family.sql` añade `ip_allocations_device_family(device_id,family) INCLUDE(addr)` mediante `CREATE INDEX IF NOT EXISTS` normal dentro de la transacción SQLx. La creación bloquea brevemente las escrituras de `ip_allocations`: reserve una ventana pequeña antes del despliegue. Se conservan ambos índices únicos anteriores; no cambia IPAM ni la API. No use `CREATE INDEX CONCURRENTLY` dentro de esa transacción.

## Emisor y certificado por red

1. En la estación offline, `atenea root-init --out /ruta/offline/root.key` crea la raíz e imprime el ancla pública. Guarde esa raíz **fuera del control plane**.
2. Prepare la semilla del emisor antes del primer arranque: en un directorio privado vacío de la estación de administración, `umask 077; openssl rand -hex 32 > issuer.key`. El RNG del sistema genera los 32 bytes; no escriba una clave a mano. Copie esta clave al servidor en un fichero 0400 (0600 también admitido), propietario del usuario del servicio, y configure `ISSUER_KEY_FILE`. Arranque el servicio, cree la organización y la red por API con el ancla pública de la raíz; obtendrá el UUID de red.
3. En la estación offline certifique esa misma clave: `atenea issuer-cert --root /ruta/offline/root.key --issuer-key /ruta/privada/issuer.key --network-id HEX32 --serial 1 --valid-days 30 --out /ruta/privada/issuer.cert`. `HEX32` es el UUID de red sin guiones. La raíz nunca se copia al servidor.
4. Cargue `issuer.cert` (178 bytes) por `PUT /api/v1/networks/UUID/issuer-cert`, cuerpo `application/octet-stream`, sesión admin/owner y CSRF. Se verifica red, raíz, firma y vigencia.
5. Para otra red del mismo proceso repita `issuer-cert` con la misma `issuer.key`, el nuevo UUID y un serial adecuado. `atenea issuer-init` también genera clave y certificado juntos cuando ya se conoce el identificador de red; no sustituya por accidente la clave del emisor de un proceso que sirve otras redes.

La emisión responde `503 issuer_unavailable` sin certificado adecuado o si este no cubre el lease completo. Renueve antes del vencimiento; no amplíe el lease para compensar una pérdida del canal de control. La configuración incluye solo dispositivos aprobados relacionados por políticas, junto con las reglas inversas necesarias para ACL sin estado.

**Antes de crear políticas: ACL sin estado y bidireccionalidad efectiva.** Cada
política genera una regla inversa con el mismo protocolo, selectores intercambiados
y **sin restricción de puertos**. Una política A→B `tcp/22` permite también a B
iniciar tráfico TCP a A en cualquier puerto, no solo responder a conexiones de A.
Revise ambos sentidos (también para subredes): estas políticas no expresan acceso
estrictamente unidireccional ni retorno limitado a conexiones ya iniciadas.

Cree un token de enrolamiento desde la red y entregue `join` por un canal seguro: contiene URL, token de un uso y ancla de confianza. El operador genera las claves con `atenea device-init --dir DIR` y ejecuta `atenea enroll --url URL --token TOKEN --dir DIR --name NOMBRE`. Compare la huella del bundle por otro canal antes de aprobar. Nunca coloque tokens reales en historial compartido ni documentación.

## Panel, informes y métricas

Tras compilar `apps/web`, configure `ATENEA_CP_WEB_DIR` con su `build/client`.
Mantenga `PUBLIC_URL` igual al origen público del navegador: el WebSocket de
`/api/v1/networks/{net}/events` rechaza cualquier otro `Origin`. El proxy debe
reenviar el upgrade WebSocket y permitir conexiones persistentes/ping cada 25 s.
Se admiten ocho conexiones por usuario y 64 por proceso; no se configura CORS.
El panel usa la misma sesión OIDC, CSRF y permisos que REST. Sin `WEB_DIR`,
las rutas no API siguen devolviendo el error JSON habitual.

`0002_panel.sql` añade informes y muestras; aplique `--migrate` con
`APPLICATION_ROLE` para conceder permisos sobre las tablas nuevas sin permitir
modificar auditoría. No ejecute varias instancias: las secuencias, conexiones y
notificaciones son locales al proceso.

Los agentes con `[sync]` y lease vigente envían cada 15 s un informe firmado
a `POST /api/v1/device/status`. Los informes no contienen IPs externas ni claves,
solo UUID de pares, caminos, edad de recepción, RTT y nombre del relay conocido.
Los fallos no bloquean el agente y aumentan `sync.report_errors` en su estado IPC.
El inventario distingue `online` (<90 s), `stale` (<600 s), `offline` y `unknown`;
es información declarada por el dispositivo, no una comprobación independiente.
No hay catálogo adicional de relays: se deriva de los nombres observados.

La tarea de muestreo toma una muestra al arrancar y cada 60 s a partir de los
informes de dispositivos aprobados, conserva siete días y se detiene limpiamente
con el proceso. `GET /api/v1/networks/{net}/metrics` devuelve datos reales o una
lista vacía, nunca puntos sintetizados. Un fallo de muestreo emite un aviso sin
datos sensibles; compruebe PostgreSQL y las migraciones si faltan muestras.
Las reglas de clasificación y caché, y las cabeceras de seguridad del contenido,
figuran en §10 del contrato.

## Subredes y DNS (fase 7)

Aplique `0003_routes_dns.sql` con `--migrate` y el rol administrativo antes de
actualizar el servicio. No hay dependencias nuevas. El router declara hasta ocho
CIDR privados canónicos mediante `POST /api/v1/device/routes` (firma de dispositivo,
una petición cada cinco segundos). La declaración no autoriza tráfico:
un admin debe aprobar cada ruta desde `/api/v1/networks/{net}/routes/{id}/approve`
con `{"snat":true}` o `{"snat":false}` explícito. Las aprobaciones se serializan
bajo bloqueo de la red y rechazan solapes con el túnel u otras rutas aprobadas.
Sin SNAT configure rutas de retorno en la LAN; con SNAT se pierde la atribución
del cliente en los registros de los hosts internos.

Las políticas pueden referirse a prefijos contenidos en una ruta aprobada.
Solo sus pares seleccionados reciben la ruta; retirarla o rechazarla elimina
su selección en la siguiente emisión sin romper las políticas huérfanas.
Revocar el dispositivo rechaza sus rutas. Las configuraciones sin novedades
siguen siendo v1, byte a byte; las que incluyen rutas o DNS son v2.

`PUT /api/v1/networks/{net}/dns` configura hasta tres servidores alcanzables
y ocho dominios de enrutamiento, por ejemplo
`{"servers":["10.77.0.1"],"domains":["lab.internal"]}`. No se captura DNS general.
Cada emisión omite los servidores que no caben en sus rutas autorizadas.
El inventario conserva `report.forwarding_ok`, `report.dns` y `report.routes`
declarados por el agente; una ruta `conflict` exige resolver el solape local,
no forzar su instalación. Los eventos `changed/routes` y `changed/dns`
requieren resincronización REST. Contrato completo y resoluciones: §11.

## Integración automatizada

```sh
apps/control-plane/tests/run.sh
# O una instancia PostgreSQL de pruebas existente:
ATENEA_TEST_DATABASE_URL='postgres://test:CAMBIAR@127.0.0.1/postgres' \
  apps/control-plane/tests/run.sh
```

Sin URL, el script exige Docker y arranca/reutiliza `atenea-control-plane-tests-pg17` con imagen `postgres:17`, puerto local dinámico y credenciales exclusivamente sintéticas. No borra el contenedor. Si hay URL, la conexión debe permitir `CREATE DATABASE`, `CREATE ROLE` y `SET ROLE`; use una instancia aislada, nunca producción. El script falla si no puede obtener una base real: no existe sustituto en memoria.

Cada prueba asíncrona crea una **base PostgreSQL independiente**, aplica migraciones y usa el router Axum con `tower::ServiceExt::oneshot`. Los helpers insertan usuario y sesión con hashes reales. Se elimina la base al terminar correctamente; un pánico puede dejar bases `atenea_test_*` y un rol `atenea_test_role_*` que el administrador puede retirar cuando no haya pruebas en marcha. `cargo test` directo sin URL omite la integración con diagnóstico explícito; esto **no demuestra** cobertura PostgreSQL. CI debe usar `tests/run.sh` para impedir esa omisión.

Cobertura implementada de §8:

- Aislamiento entre organizaciones, ocultación `404`, permisos member/admin y último owner.
- Tokens inexistentes, caducados y usados; doce consumos concurrentes con exactamente un ganador.
- Aprobaciones concurrentes, unicidad SQL de direcciones, agotamiento de CIDR, reutilización tras revocar y límite de 256 dispositivos aun con direcciones libres.
- Prueba de posesión inválida, colisiones de identidad/Noise/UUID y rollback del consumo del token.
- Fichero de configuración aceptado por `atenea_identity::verify_config` con claves y estado locales; versiones crecientes, políticas inversas, eliminación de pares revocados/pendientes y al borrar políticas; errores de emisor, firma, reloj y replay.
- Fronteras adicionales: CIDR solapados, selectores y puertos inválidos, prefijos fuera de red, desbordamiento de reglas compiladas con rollback, certificados fuera de red/raíz/vigencia o con firma inválida y límite de tasa independiente por IP.
- CSRF ausente/incorrecto, expiración absoluta, inactividad y logout.
- Proveedor OIDC HTTP local con discovery, JWKS y token **firmado RS256 por jsonwebtoken**: PKCE S256, state/cookie de navegador, issuer, audience, nonce, expiración, firma y flujo de un uso. La clave RSA de 2048 bits se genera en memoria una vez por proceso de pruebas; no se almacena ni se publica ninguna clave privada.
- Eventos de todas las escrituras enumeradas y denegación real de UPDATE/DELETE con `SET ROLE` sobre el rol limitado.
- Igualdad del OpenAPI publicado con `openapi()` generado. Regeneración: `cargo run -p atenea-control-plane -- --openapi > docs/api/openapi.json`.
- Panel: inventario con IPs e informe firmado, validación y límite de tasa; muestreo/retención con reloj explícito; aislamiento de métricas y eventos; WebSocket TCP real con hello/cambios/Origin/límites; SPA, rutas reservadas, CSP y caché.

## E2E de laboratorio

Las pruebas anteriores no arrancan agentes, TUN ni tráfico. El E2E con control plane real, PostgreSQL 17, Dex (OIDC) y dos agentes con TUN en namespaces es `tests/network-lab/run-e2e.sh` (Docker; contenedores desechables). Comprueba enrolamiento, aprobación, políticas, sincronización, tráfico, revocación (peers y propio agente) y pérdida del canal de control. Resultados y límites: [`e2e-fase3b`](../../docs/operations/e2e-fase3b.md). No forma parte del CI (necesita contenedores privilegiados).
