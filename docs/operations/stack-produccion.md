# Stack de producción y ensayo local (fase 9)

## Arquitectura y frontera de confianza

`deploy/compose/prod/compose.yaml` contiene siete servicios: HAProxy, Caddy, PostgreSQL 17, Dex, control plane (con panel estático), coordinator y relay. `backend` es una red `internal: true`; `edge` permite entrada/salida. PostgreSQL y Dex no publican puertos. El control plane y coordinator solo reciben tráfico HTTP por la red de contenedores, detrás de Caddy.

| Puerto host | Destino |
| --- | --- |
| TCP 80 | Caddy, HTTP-01 y redirección a HTTPS |
| TCP 443 | HAProxy: SNI del relay **y** ALPN `atenea-relay/1` → relay; todo lo demás → Caddy |
| UDP 443 | Relay |
| UDP 3478 | STUN del coordinator; segundo socket 3479 interno |

HAProxy no termina TLS y **no** acepta PROXY del exterior. Solo el backend relay recibe `send-proxy-v2`. El relay conserva la validación de origen loopback: comparte únicamente el namespace de red de HAProxy (`network_mode: service:haproxy`), escucha TLS en `127.0.0.1:8443` y no publica ese puerto. No se ha ampliado confianza a toda la subred Docker. El contenedor relay conserva su propio usuario, filesystem, secretos y límites. Los overlays de métricas deben conectar **HAProxy**, no relay, a su red de scraping.

Los binarios se construyen `--locked` con Rust 1.99 fijado por digest. El panel se construye con pnpm frozen-lockfile dentro de Node y se copia a `/usr/share/atenea/web`, servido mediante `ATENEA_CP_WEB_DIR`. Los tres runtimes distroless usan UID/GID 65532, sin shell. Todos los servicios tienen límites, `read_only`, `cap_drop: ALL`, `no-new-privileges`, reinicio y healthcheck; solo listeners bajos reciben `NET_BIND_SERVICE`. PostgreSQL usa UID 999; Caddy conserva el usuario de su imagen para escribir su volumen privado. Los healthchecks Rust realizan HTTP real o conexión al socket Unix, sin instalar herramientas en runtime.

Volúmenes: `pgdata`, `caddy-data`, `atenea-state`. Dex del piloto mantiene estado efímero en memoria y usuarios/clientes declarados; reiniciar Dex invalida el estado transitorio del proveedor. `atenea-state` queda reservado como almacenamiento de operación, no convierte Dex en HA ni persiste su memoria.

## Bootstrap administrativo

Requisitos: Docker/Compose y las imágenes de laboratorio ya disponibles. No se instala Rust, Node, OpenSSL, Python ni herramientas de infraestructura en el anfitrión.

```sh
for service in control-plane coordinator relay; do
  docker build -t atenea/$service:local -f deploy/images/$service/Dockerfile .
done
docker build --target tools -t atenea/tools:local -f deploy/images/control-plane/Dockerfile .
bash deploy/compose/prod/bootstrap.sh
ENV_FILE="$PWD/deploy/compose/prod/.runtime/stack.env"
docker compose --env-file "$ENV_FILE" -f deploy/compose/prod/compose.yaml up -d --wait postgres
bash deploy/compose/prod/migrate.sh "$ENV_FILE"
docker compose --env-file "$ENV_FILE" -f deploy/compose/prod/compose.yaml up -d --wait
```

El bootstrap rehúsa sobrescribir un runtime existente. Genera contraseñas aleatorias, cliente OIDC, clave de emisor, semilla independiente de firma relay y certificado TLS de laboratorio; ejecuta `atenea root-init` y `atenea-coordinator --print-relay-key` en contenedores. Solo la **clave pública raíz** entra en las anclas del coordinator. La raíz y la CA privada permanecen en `offline/`, que jamás se monta en los servicios. No copie `offline/` al servidor. En producción transfiera los secretos por el procedimiento SOPS y sustituya el certificado efímero del relay por uno válido para `RELAY_HOST`.

`ATENEA_RUNTIME_DIR` permite generar el material en otra ruta absoluta. El directorio predeterminado `.runtime/` y los runtimes de ensayo están ignorados; `.dockerignore` impide enviarlos al contexto de build. `stack.env` solo contiene nombres, rutas y referencias inmutables de imagen, nunca contraseñas. Las imágenes locales se fijan por ID SHA-256 de Docker; un release publicado debe sustituirlos por `repositorio@sha256:digest` del manifiesto firmado, sin publicar nada durante este ensayo. `.env.example` muestra exclusivamente ejemplos sintéticos.

### Secretos y permisos

Compose monta secretos **por fichero**, no como valores de entorno. El bootstrap asigna UID 65532 (999 para PostgreSQL) y modo **0400**. Docker Compose local usa bind mounts y no aplica remapeo `uid/gid/mode` de Swarm: el fichero fuente debe tener los permisos correctos. En Linux compruébelos antes de arrancar.

El control plane ahora lee `ATENEA_CP_DATABASE_URL_FILE` y `ATENEA_CP_OIDC_CLIENT_SECRET_FILE`; elimina solo CR/LF finales, rechaza fichero vacío/ilegible y no imprime su contenido. Las variables directas conservan precedencia si se configuran ambas: no las configure en contenedores. Emisor y coordinator reutilizan `ISSUER_KEY_FILE` y `RELAY_SIGNING_KEY_FILE`. Su validación admite **0400 o 0600**, y rechaza permisos de grupo/otros y ejecución (tests para 0440/0640/0644/0604/0700). Los servicios no tienen acceso a la URL de migración.

PostgreSQL crea `atenea_migrator`, propietario de BD/esquema, y `atenea_app`, rol limitado. `migrate.sh` monta la URL de migrador solo en el proceso efímero `--migrate`, aplica SQLx y concede permisos mediante `APPLICATION_ROLE=atenea_app`. El servidor normal rechaza permisos destructivos de auditoría. Los scripts de init solo se aplican sobre un `pgdata` vacío; cambiar un fichero no rota automáticamente las contraseñas de una BD existente.

### HTTPS y OIDC

Para laboratorio `CADDY_TLS=internal`; el navegador debe confiar en la CA de Caddy o el runner debe declararlo como laboratorio. Para producción use el correo ACME como `CADDY_TLS`, DNS correcto para `PANEL_HOST` y TCP80 accesible. Caddy usa HTTP-01 porque HAProxy ocupa 443. Registre el callback exacto `https://PANEL_HOST/auth/callback`. Dex se publica bajo `/dex`; `/coord/` sirve el coordinator. No active cookies inseguras.

Crear organización/red y certificar el emisor son operaciones posteriores: use la raíz offline con `atenea issuer-cert` para el UUID real de red y suba solo el certificado por el panel. Los agentes conservan claves locales y ancla pública propia.

## Actualización y rollback

1. Haga copia de BD y secretos y verifique restauración antes de actualizar.
2. Conserve `stack.env`, manifiesto y digests de N. No use etiquetas móviles.
3. Aplique migraciones con el contenedor efímero y rol migrador de N+1, mientras N sigue atendiendo.
4. Sustituya las referencias de imagen por los digests de N+1 y haga `docker compose up -d --wait`.
5. En rollback restaure los **digests N**, no la BD anterior: no ejecute down migrations destructivas. Verifique datos y permisos.

`tests/stack/upgrade.sh` crea un worktree detached del commit `bb6d63d538e64021d78d64a77012a4ebe4b53e18` (N, migraciones 0001/0002), construye N y N+1, usa PostgreSQL real y datos/configuración firmada reales, aplica 0003, comprueba lecturas/escrituras de N+1, ejecuta N sobre ese esquema y hace rollback por digest local. Conserva fingerprint de datos, cinco escrituras HTTP y DNS añadido por N+1. La migración 0003 es aditiva: no fue necesario modificarla. La ventana exige **migrar antes de sustituir binarios**: no se afirma que N+1 opere antes de aplicar 0003.

El upgrade local fuerza `RUSTUP_TOOLCHAIN=1.99.1` (ya instalada en `atenea-lab`), `CARGO_BUILD_JOBS=2`, `cargo --offline` y `--network none`; reutiliza `atenea-cargo-registry` y `atenea-lab-target`. Copia únicamente las fuentes Rust a un directorio efímero del compilador y actualiza sus mtimes para que Cargo no mezcle artefactos de N/N+1; no toca el árbol del operador. No instala dependencias ni reconstruye el panel. Un intento previo detectó que N pide 1.99.0: rustup intentó consultar `static.rust-lang.org`, pero la red deshabilitada lo bloqueó por DNS antes de descargar; se detuvo y se corrigió fijando la toolchain instalada.


## Reproducción del ensayo integrado

```sh
# Requiere imágenes :local ya construidas; el runner no instala ni compila dependencias.
tests/stack/run.sh
# Upgrade aislado y sin red en la compilación (cachés de laboratorio):
tests/stack/upgrade.sh
```

El runner no publica puertos del host. Arranca el Compose real más un overlay de laboratorio y resuelve `panel.stack.test` y `relay.stack.test` en la red de pruebas. Administra por API HTTPS a través de HAProxy/Caddy: OIDC por código real con Dex, cookie `Secure`, token CSRF y rechazo de escritura sin CSRF. El cliente confía exclusivamente en la CA efímera del runtime. El navegador queda cubierto por `tests/e2e-panel`, **no por este ensayo**: el intento con Chromium y el certificado Ed25519 de laboratorio devolvió `ERR_SSL_VERSION_OR_CIPHER_MISMATCH`, sin descargar componentes.

Los agentes reales usan TUN, namespaces y NAT. Solo en el laboratorio, enrolamiento/sincronización y el backchannel OIDC (discovery/token) usan HTTP interno aislado: el cliente Rust del control plane usa raíces públicas y no incorpora la CA efímera mediante `SSL_CERT_FILE`. El runner dirige las URL frontales de Dex a su proxy HTTPS; no valida un recorrido de navegador ni un issuer público completo. Panel/login frontal y relay mantienen TLS verificado contra la CA efímera; el relay pasa por HAProxy en 443 con su SNI. No se afirma validación TLS pública de todos los clientes.

La sonda prueba SNI+ALPN correcto, SNI desconocido, PROXYv2 forjado y que 8443 no es alcanzable desde `edge`; el recorrido crea red, enrola/aprueba dos agentes, transfiere 1MB por P2P, relay UDP y relay TLS443, y revoca un dispositivo. El runner imprime el tiempo real hasta salud de los siete servicios, incluidas migraciones. Limpia únicamente recursos con su nombre de proyecto; no elimina imágenes ni volúmenes de laboratorios existentes.

### Resultado integrado (2026-10-25)

Ejecución secuencial `tests/stack/run.sh`, código de salida 0, **76,60 s** de pared. Salida funcional completa (sin las líneas de progreso de creación/health de Compose):

```text
PASS docker compose config
PASS stack saludable en 29 s (incluye migraciones)
PASS SNI relay + ALPN atenea-relay/1 termina TLS en relay vía HAProxy
PASS SNI desconocido no negocia relay
PASS PROXYv2 forjado externo rechazado antes de TLS
PASS backend PROXY relay 8443 solo loopback
PASS OIDC Dex por código sobre HTTPS, cookie Secure y sesión real
PASS CSRF rechazado sin token; red, certificado y dos agentes CLI enrolados/aprobados con huellas
PASS direct: TCP 1000000 bytes con SHA-256 coincidente
PASS relay-udp: TCP 1000000 bytes con SHA-256 coincidente
PASS relay-tls: TCP 1000000 bytes con SHA-256 coincidente por HAProxy TLS443/SNI verificado
PASS revocación efectiva; recorrido API/agentes en 35.0 s
STACK E2E: TODO OK (API OIDC; navegador cubierto por tests/e2e-panel)
```

La topología añade rutas de retorno efímeras en los namespaces de coordinator y HAProxy mediante contenedores auxiliares, sin alterar sus imágenes, permisos ni Compose productivo. Solo el tráfico al control plane recibe el SNAT exterior del runner; aplicarlo también a STUN ocultaba las NAT domésticas y solo permitía relay, no P2P.

También se ejecutó `docker compose config --quiet` para el base y para **base + backup + observability juntos**, con resultado `PASS docker compose config base y base+backup+observability`. Es validación de composición, no arranque ni ensayo de los servicios de esos overlays.


## Evidencia de upgrade y límites

Ejecución final secuencial `tests/stack/upgrade.sh` (2026-10-25), salida 0, **37,34 s** de pared; compilaciones N/N+1 de 14,56 s y 15,73 s. Salida funcional completa (sin progreso de compilador/BuildKit ni `INSERT` del fixture):

```text
N commit=bb6d63d538e64021d78d64a77012a4ebe4b53e18; N+1 working-tree HEAD=813030ad5e3760078fe237b882f7f48393bc0cbc
N digest=sha256:c5d32c7709fe62f98f3e95e198388a38e57a888c074cd9596d7364cfe1487838
N+1 digest=sha256:0708d04db9a4170d05aa1b690327df624beca1e52b39ecf17558a32581d43b45
PASS HTTP initial: sesión, red, dispositivo, política y escritura persistente
PASS N schema=1,2; datos=b1258c483921fd804085458eeaa3db4f
PASS HTTP next: sesión, red, dispositivo, política y escritura persistente + DNS 0003
PASS HTTP compatibility: sesión, red, dispositivo, política y escritura persistente
PASS N sobre schema N+1 (sin down migrations)
PASS HTTP before-rollback: sesión, red, dispositivo, política y escritura persistente
PASS HTTP rollback: sesión, red, dispositivo, política y escritura persistente
PASS rollback digest=sha256:c5d32c7709fe62f98f3e95e198388a38e57a888c074cd9596d7364cfe1487838; datos=b1258c483921fd804085458eeaa3db4f; 5 escrituras y DNS N+1 conservados
PASS upgrade/rollback PostgreSQL real; contenedores y worktree efímeros
PASS upgrade completo en 37 s (incluye ambas compilaciones offline)
```

La sesión de ese simulacro se siembra para probar compatibilidad de aplicación/esquema, no OIDC (el login real corresponde al ensayo integrado). No se publica ni descarga ningún release privado.

No se detectaron descargas de imágenes, crates, toolchains ni componentes: la consulta de rustup descrita arriba quedó bloqueada por `--network none`. BuildKit resolvió la base distroless ya local (0,0 s en el ensayo final), sin descargar capas. Un intento intermedio puso de manifiesto la caché cruzada Cargo N/N+1 y falló compilando antes de arrancar PostgreSQL; la copia efímera con mtimes renovados eliminó esa causa.

Al terminar, `docker ps -a` mostró únicamente `atenea-control-plane-tests-pg17`; `docker network ls`, solo `bridge`, `host`, `none`. No quedaron contenedores, redes ni volúmenes de las ejecuciones realizadas. Se conservaron las cachés `atenea-cargo-registry`, `atenea-lab-target`, `atenea-panel-modules`, `atenea-panel-pnpm`, `atenea-stack-modules`, `atenea-win-target`, `atenea-fuzz-*` y los volúmenes anónimos no atribuidos. También se conservaron los tres volúmenes **preexistentes** `atenea-stack-67476_{pgdata,caddy-data,atenea-state}` y su runtime, procedentes del trabajo cancelado: no pertenecían a estas ejecuciones. No se hizo prune, commit, push ni despliegue.

**SIN COMPROBAR:** DNS real, alcanzabilidad IPv4/IPv6 externa, firewall del proveedor, UDP3478/443 y TCP80/443 desde Internet, ACME real, certificado público del relay, TLS público del enrolamiento/sincronización, múltiples proveedores, HA real y rendimiento/carga. No se ha accedido a infraestructura real ni publicado en GHCR.
