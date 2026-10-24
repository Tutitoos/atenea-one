# Control plane

Monolito Axum y PostgreSQL: OIDC, sesiones, organizaciones, RBAC, IPAM, enrolamiento y emisión de configuraciones firmadas. Contrato: [`control-plane-v1`](../../docs/api/control-plane-v1.md). No almacena claves privadas de dispositivos ni la clave raíz.

## Configuración y arranque

PostgreSQL 17 es la versión del entorno de integración. Termine TLS en un proxy de confianza y mantenga el puerto de PostgreSQL fuera de Internet. Registre en el proveedor OIDC el callback exacto y habilite Authorization Code; MFA corresponde al proveedor.

Puede usar un TOML indicado por `ATENEA_CP_CONFIG`; sus claves son los nombres de la tabla en minúsculas. Las variables `ATENEA_CP_*` prevalecen sobre el fichero.

| Sufijo de variable | Valor / función |
| --- | --- |
| `DATABASE_URL` | URL PostgreSQL del rol de aplicación, obligatoria |
| `LISTEN` | `127.0.0.1:8080` |
| `PUBLIC_URL` | URL pública que se entrega en `join`; valor local predeterminado `http://127.0.0.1:8080` |
| `OIDC_ISSUER` | URL del emisor; discovery y JWKS validan los ID tokens |
| `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` | Cliente OIDC registrado |
| `OIDC_REDIRECT_URL` | URL pública exacta de `/auth/callback` |
| `ISSUER_KEY_FILE` | Fichero regular **0600**, semilla hex del emisor (32 bytes) |
| `CONFIG_VALIDITY_SECS` | `300`; máximo `3600`, coherente con el lease del agente |
| `DEV_INSECURE_COOKIES` | `false`; `true` solo para HTTP local, emite aviso |
| `ENROLL_PER_MINUTE` | `10`; token bucket por IP de origen |
| `APPLICATION_ROLE` | Rol que recibe permisos limitados al aplicar migraciones |

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

## Emisor y certificado por red

1. En la estación offline, `atenea root-init --out /ruta/offline/root.key` crea la raíz e imprime el ancla pública. Guarde esa raíz **fuera del control plane**.
2. Prepare la semilla del emisor antes del primer arranque: en un directorio privado vacío de la estación de administración, `umask 077; openssl rand -hex 32 > issuer.key`. El RNG del sistema genera los 32 bytes; no escriba una clave a mano. Copie esta clave al servidor en un fichero 0600 y configure `ISSUER_KEY_FILE`. Arranque el servicio, cree la organización y la red por API con el ancla pública de la raíz; obtendrá el UUID de red.
3. En la estación offline certifique esa misma clave: `atenea issuer-cert --root /ruta/offline/root.key --issuer-key /ruta/privada/issuer.key --network-id HEX32 --serial 1 --valid-days 30 --out /ruta/privada/issuer.cert`. `HEX32` es el UUID de red sin guiones. La raíz nunca se copia al servidor.
4. Cargue `issuer.cert` (178 bytes) por `PUT /api/v1/networks/UUID/issuer-cert`, cuerpo `application/octet-stream`, sesión admin/owner y CSRF. Se verifica red, raíz, firma y vigencia.
5. Para otra red del mismo proceso repita `issuer-cert` con la misma `issuer.key`, el nuevo UUID y un serial adecuado. `atenea issuer-init` también genera clave y certificado juntos cuando ya se conoce el identificador de red; no sustituya por accidente la clave del emisor de un proceso que sirve otras redes.

La emisión responde `503 issuer_unavailable` sin certificado adecuado o si este no cubre el lease completo. Renueve antes del vencimiento; no amplíe el lease para compensar una pérdida del canal de control. La configuración incluye solo dispositivos aprobados relacionados por políticas, junto con las reglas inversas necesarias para ACL sin estado.

Cree un token de enrolamiento desde la red y entregue `join` por un canal seguro: contiene URL, token de un uso y ancla de confianza. El operador genera las claves con `atenea device-init --dir DIR` y ejecuta `atenea enroll --url URL --token TOKEN --dir DIR --name NOMBRE`. Compare la huella del bundle por otro canal antes de aprobar. Nunca coloque tokens reales en historial compartido ni documentación.

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

## E2E de laboratorio

Las pruebas anteriores no arrancan agentes, TUN ni tráfico. El E2E con control plane real, PostgreSQL 17, Dex (OIDC) y dos agentes con TUN en namespaces es `tests/network-lab/run-e2e.sh` (Docker; contenedores desechables). Comprueba enrolamiento, aprobación, políticas, sincronización, tráfico, revocación (peers y propio agente) y pérdida del canal de control. Resultados y límites: [`e2e-fase3b`](../../docs/operations/e2e-fase3b.md). No forma parte del CI (necesita contenedores privilegiados).
