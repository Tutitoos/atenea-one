# Infraestructura reproducible — fase 9

## Alcance y topología

ADR-009 propone un nodo principal IPv4+IPv6 con HAProxy L4, Caddy, PostgreSQL,
Dex, control plane, coordinator, relay y STUN, y un relay secundario en **otro
proveedor**. El principal no tiene failover automático: coordinator es efímero;
el control plane y la BD se recuperan desde copia. Los túneles establecidos
persisten solo mientras su configuración conserve el lease 300 s + tolerancia
30 s (ADR-002). La raíz privada nunca se instala en servidores (ADR-005).

Los ejemplos públicos usan `panel.example.test`, `relay.example.test`,
`relay2.example.test`, `192.0.2.0/24` y `2001:db8::/32`. No son destinos reales.
La elección y contratación del proveedor pertenece al responsable: ningún módulo
entregado contrata servidores ni este procedimiento autoriza despliegues reales.

Las mediciones de infraestructura publicadas abajo son anteriores al cutover wire-v2/relay-v2. El ejemplo y los fixtures actuales incluyen `relay_id` persistente; su nueva ejecución y la negociación ALPN `/2` quedan sin comprobar durante este cambio.

## Orden de instalación

1. Elegir proveedor principal y otro para el secundario; Debian 13, SSH por clave,
   consola de recuperación y reloj sincronizado. Confirmar IPv4 e IPv6.
2. Revisar el plan Tofu local. Adaptar explícitamente los módulos al proveedor
   autorizado; crear los registros A/AAAA para panel, relay y relay2. No publicar
   AAAA sin ruta IPv6 operativa. Esperar propagación antes de solicitar TLS.
3. Construir/verificar el release y su manifiesto firmado. Anotar referencias
   `repositorio@sha256:...`; no usar tags ni `build:` en despliegue.
4. Preparar los secretos y configuración según el Compose de
   `deploy/compose/prod/compose.yaml` y su bootstrap de laboratorio. En producción
   usar certificados confiables y un OIDC real, no contraseñas de laboratorio.
5. Copiar `infra/ansible/inventory.example.yml` a un inventario privado excluido
   (`inventory.yml`); definir claves públicas SSH y redes administrativas. Validar
   la huella SSH fuera de banda y poblar `known_hosts`: no se desactiva su comprobación.
6. Ejecutar `site.yml` desde `infra/ansible/` con `-i inventory.yml`, primero con
   acceso por consola disponible. La clave administrativa se instala **antes**
   de prohibir root/contraseña; el usuario administrativo tiene sudo por clave.
7. Ejecutar `relay.yml` sobre el segundo nodo, configurar el coordinator principal
   con ambos endpoints y sus `relay_id` distintos en `ATENEA_COORD_RELAYS` y comprobar conmutación desde fuera.
8. Configurar el hook `observability_agent_tasks` con una ruta de tareas del
   exportador revisada por el operador; por defecto no instala otro servicio.
   Habilitar y ensayar las copias antes de una actualización.

El controlador Ansible también se ejecuta en contenedor: construir
`docker build -t atenea-infra-tools tests/infra`, montar el repo, inventario,
`known_hosts`, clave SSH y clave age como ficheros privados de solo lectura, y
establecer `SOPS_AGE_KEY_FILE` a su ruta **dentro del contenedor**. No montar el
socket Docker del Mac. Las herramientas instaladas en nodos son paquetes Debian;
no se instala nada en el anfitrión de administración.

### Variables del despliegue

- `atenea_compose_source`: directorio completo de `deploy/compose/prod`; el rol lo
  copia a `atenea_stack_dir` (predeterminado `/opt/atenea`) sin modificar el origen.
- `atenea_config_source`: directorio **público** que contiene `config/` y `certs/`;
  en el principal: `coordinator.toml`, `relay.toml`, `certs/relay.pem`.
- `atenea_env`: mapa no secreto para `.env`: `ATENEA_RUNTIME_DIR` igual a
  `atenea_stack_dir`, `PANEL_HOST`, `RELAY_HOST` y `CADDY_TLS` (correo ACME en producción).
- `atenea_stack_images`: mapa **completo servicio → referencia inmutable**:
  `haproxy`, `caddy`, `postgres`, `dex`, `control-plane`, `coordinator`, `relay`.
  El secundario recibe únicamente `relay` y `stun`. Se genera
  `compose.override.yaml`; `.env` recibe las referencias requeridas por Compose.
  Un release usa `repositorio@sha256:...`; el laboratorio admite IDs locales
  `sha256:...` obtenidos con `docker image inspect`, como `tests/stack/upgrade.sh`.
  No hay modo dry-run. Las imágenes deben estar precargadas en el nodo;
  `pull_policy: never` impide descargas implícitas y falla si falta una imagen.

SOPS descifra en memoria del controlador (`no_log`); cada secreto se copia a
`secrets/` con modo **0400**, uid/gid 65532 salvo los secretos PostgreSQL
`postgres-password` y `postgres-init.sql` (999). Directorios privados 0700,
`.env` 0600. El fichero cifrado y la identidad age no se copian al nodo. No usar
`--diff` ni depuración de Ansible con material real. El esquema no acepta nombres
con barras ni nombres que incluyan `root`; sigue siendo responsabilidad del
operador no disfrazar una raíz como otro secreto.
La configuración y los certificados **públicos** se instalan 0644 y sus
directorios 0755: no se preserva el umask privado del controlador. Un certificado
`relay.pem` 0600 de root provoca un fallo de arranque bajo UID 65532 aunque la
clave privada tenga dueño/modo correctos; la clave permanece exclusivamente en
`secrets/relay.key` 0400.

### Relay secundario

`relay_node` consume `infra/ansible/relay-compose/compose.yaml`: únicamente `relay`
y `stun`, sin PostgreSQL, Dex ni claves privadas de firma de configuración o tokens.
`stun` ejecuta el binario coordinator con anclas **públicas**, sin
`relay_signing_key_file`, sin relays y HTTP en loopback del contenedor; no es un
segundo coordinator público. Se publica solo UDP 3478; su segundo puerto STUN
3479 es interno. El servicio relay sirve TLS directamente en TCP 443,
`proxy_protocol=false`, UDP 443. No necesita HAProxy ni Caddy.

Preparar `config/stun.toml`, `config/relay.toml` a partir de los `.example`,
`certs/relay.pem` y un SOPS separado cuyo único secreto sea `files.relay.key`.
Generar una sola vez por relay un identificador de 16 bytes con RNG del SO,
codificarlo en 32 hex minúsculos no cero y conservarlo como `relay_id` en su TOML
(equivalente a `ATENEA_RELAY_ID` en el proceso). Copiar **ese mismo valor** a la
entrada del coordinator; el secundario nunca reutiliza el ID del principal.
Ansible copia la configuración del operador: no genera ni renueva IDs al converger,
reiniciar Docker o rotar TLS. Mantenerlos en la copia de configuración pública.
El relay usa exclusivamente ALPN `atenea-relay/2`; actualizar todos los binarios
y configuraciones juntos, sin compatibilidad v1. Una concesión lleva un token
de 189 bytes por relay, no un token compartido entre principal y secundario.
En `atenea_env` necesita `ATENEA_RUNTIME_DIR`; `atenea_stack_images` fija `relay`
y `stun` por digest y el rol deriva `RELAY_IMAGE` y `COORDINATOR_IMAGE`.
El rol rechaza los servicios del principal y otros secretos **antes de arrancar**.
Los certificados de relay2 requieren renovación del operador, por DNS-01 u otro
canal que no abra puertos adicionales; no hay automatización ACME del secundario.

## Firewall y reloj

`base` instala chrony y unattended-upgrades. La tabla `inet atenea_host` es
propiedad del rol; se reemplaza atómicamente sin vaciar tablas ajenas de Docker.
Input y forward tienen política drop, IPv4+IPv6, respuestas established/related,
loopback e ICMP/ICMPv6 (necesario para PMTU y descubrimiento IPv6).

| Nodo | TCP entrante | UDP entrante |
| --- | --- | --- |
| Principal | 22 solo redes administrativas; 80; 443 | 443; 3478 |
| Relay secundario | 22 solo redes administrativas; 443 | 443; 3478 |

El control también se aplica al forwarding/DNAT: Docker puede eludir INPUT al
publicar puertos, por lo que se valida el **puerto original** de los flujos DNAT
antes de las cadenas Docker. Se permite egreso de los bridges Docker y sus
respuestas; ninguna aplicación puede publicar 5432/8080/8081 hacia Internet por
el mero hecho de usar `ports:`. No se añade al administrador al grupo docker.
El firewall externo debe repetir este contrato en ambos protocolos IP.

`atenea_extra_udp_ports` (lista, vacía por defecto) añade puertos UDP de entrada al principal o al relay. Un nodo que ejecuta además un **agente** necesita abrir el `51820/udp` (su puerto de escucha por defecto) para tener camino directo por Internet: con el firewall del rol, un paquete no solicitado a ese puerto se descarta. Solo afecta a la cadena `input` (el agente corre en el host, no en un contenedor); el `forward` de Docker no cambia. `tests/infra` cubre las dos ramas: el principal del ensayo abre el 51820 y el relay comprueba el comportamiento por defecto.

## OpenTofu y estado

`infra/tofu/modules/server` y `dns_records` son interfaces tipadas con
`terraform_data`: guardan una especificación y exponen salidas, **no crean VPS ni
DNS**. `examples/local` usa el backend local y provider incorporado
`terraform.io/builtin/terraform`, sin credenciales. `examples/rfc2136` es el único
ejemplo de proveedor concreto: `hashicorp/dns` 3.4.3, actualización RFC 2136 con
TSIG HMAC-SHA256. No se ha ejecutado contra un DNS real.

Antes de uso compartido seleccionar explícitamente un backend remoto cifrado con
versionado, acceso mínimo y bloqueo soportado (p. ej. S3 con mecanismo de bloqueo
compatible con la versión Tofu elegida). Migrar mediante `tofu init -migrate-state`
solo tras copia y autorización; no escribir contraseñas en configuración de backend
ni `-backend-config` versionado. Un backend local no coordina operadores en máquinas
distintas. Serializar los applies hasta haber ensayado el bloqueo remoto.
El estado puede contener secretos aunque una salida sea `sensitive`: tratarlo como
secreto, respaldarlo cifrado y jamás subirlo. `.terraform/`, estados, planes,
`*.tfvars` e inventarios privados están excluidos. Los lockfiles de providers no
son secretos; guardar el del proveedor elegido tras resolverlo y revisar hashes.

## Rotación SOPS/age

1. Generar una identidad age offline (`age-keygen` dentro del contenedor) en medio
   privado; guardar copias cifradas fuera de línea. Solo su destinatario público
   entra en la configuración SOPS privada basada en `infra/secrets/.sops.yaml`.
2. Cifrar un YAML del esquema `files` con `sops encrypt --age DESTINATARIO`;
   borrar el claro de forma acorde al medio. No introducir secretos ni identidades
   reales en este repositorio público, ni siquiera cifrados sin acuerdo expreso.
3. Para rotar acceso: añadir destinatario nuevo, montar la identidad antigua y
   ejecutar `bash infra/secrets/rotate.sh fichero.sops.yaml configuracion.yaml`
   **en el contenedor**. El script hace `updatekeys`, rota la clave de datos,
   verifica descifrado y reemplaza el cifrado con rename; ante fallo conserva el
   original. Retirar destinatarios anteriores y repetir, comprobando que el nuevo
   destinatario puede descifrar antes de retirar la última copia recuperable.
4. Rotar el destinatario age o la clave de datos **no rota** contraseñas, emisor,
   semilla del coordinator ni certificados TLS. Generar valores nuevos, actualizarlos
   en los servicios implicados, cifrar y redesplegar. Rotar emisor exige nuevo
   certificado offline con serial mayor (ADR-005); renovar la raíz requiere su
   procedimiento de dos anclas o reincorporación, no este script.

## Actualización y rollback

Ejecutar `upgrade.yml` serialmente con `atenea_backup_verified=true` únicamente
tras verificar una copia restaurable y precargar los nuevos digests de
`atenea_stack_images`. Guarda `.env` y `compose.override.yaml` realmente desplegados.
Arranca PostgreSQL, ejecuta un contenedor efímero `control-plane --migrate` con
`migration-database-url` montado y `ATENEA_CP_APPLICATION_ROLE=atenea_app`, igual que
`deploy/compose/prod/migrate.sh`, **antes** de sustituir las aplicaciones.
Después ejecuta `up -d --wait --wait-timeout 180` y exige todos los healthchecks
(ocho servicios en el principal, incluido `netns`; dos en el secundario). El plazo es configurable
mediante `atenea_stack_health_timeout`. En error publica el estado de Compose;
los logs se consultan localmente para no volcar secretos en la salida de Ansible.
El mapa por digest `atenea_stack_images` debe incluir `netns` explícitamente en el
principal; normalmente reutiliza la referencia de `haproxy`. Las mediciones
históricas de siete servicios que siguen abajo preceden a esta corrección del
namespace, documentada en [Caídas y leases](caidas-y-leases.md).

El principal instala `atenea-stack.service` ligado por `PartOf/Requires/After` a
`docker.service`, habilitado en `docker.service.wants`. Espera la capa Caddy/BD/
control plane/Dex/coordinator y recrea netns/HAProxy/relay en orden Compose después
de cada arranque del motor. Detiene sus tres compartidores antes de parar Docker;
no elimina datos ni ejecuta migraciones en un reinicio. Ansible ejecuta los
handlers de Docker antes de desplegar y verificar salud, evitando un reinicio
diferido **después** de declarar el stack sano. Para retirar el principal,
deshabilite primero esta unidad; para reparar la entrada, reiníciela.

Se midieron tres `systemctl restart docker` fríos en el nodo anidado con la
unidad instalada: **36,753 / 36,817 / 37,038 s** hasta ocho servicios healthy,
probes correctos y mismo inode de namespace en netns/HAProxy/relay. Antes del
arreglo, uno de tres reinicios dejó los dos consumidores `exited/unhealthy`;
dockerd registró `cannot join network of a non running container`. El daemon de
esta reproducción fue Docker **26.1.5**, almacén clásico **vfs** (overlay2 no pudo
montarse), no se atribuye el fallo a overlay2. `live-restore=false` se usó solo
para forzar una restauración fría en diagnóstico; el rol conserva `true`, que
no sustituye la reconvergencia tras reboot/primer cambio del daemon.

Si falla, restaura ambos ficheros, vuelve a arrancar los digests anteriores,
comprueba todos sus healthchecks y termina **con error**, aunque el rollback
haya recuperado salud. No exige un puerto HTTP público adicional para medir salud.
El rollback cambia **digests**, no deshace datos, configuración pública ni secretos.
No combinar una rotación incompatible con un cambio de versión. Respetar
expand/contract; no borrar columnas hasta retirar la versión anterior.
La salud Compose no demuestra compatibilidad de datos: el ensayo específico de
`tests/stack/upgrade.sh` exige actualmente las migraciones **0001–0005** de N+1 sobre N (0001/0002) y conserva datos entre ambos binarios; la evidencia histórica siguiente cubrió 0003, no acredita todavía el laboratorio con 0005;
el ensayo Ansible usa imágenes locales y no prueba un release publicado.

## Ensayo reproducible y límites

Ejecutar `bash tests/infra/run.sh` con `atenea-infra:local` ya preaprovisionada
(o `INFRA_IMAGE` apuntando a otra imagen local construida con
`tests/infra/Dockerfile`). El runner no construye ni descarga herramientas:
si falta la imagen falla antes de arrancar el nodo. Construirla puede requerir
APT y registros; en un entorno sin red requiere autorización/preparación previa.
El Dockerfile incluye los paquetes base y Docker anidado para que los roles
encuentren todo instalado; el inventario de ensayo desactiva solo la actualización
de índices APT (`atenea_apt_update_cache=false`). Producción mantiene esa
actualización activada por defecto.
Las imágenes deben corresponder a las fuentes actuales, incluido el healthcheck
Rust del relay. `INFRA_RELAY_IMAGE` permite cargar una imagen local alternativa
para comprobarlo sin modificar la etiqueta protegida `atenea/relay:local`.
Los alias `atenea-infra-pin/haproxy:pinned` y `atenea-infra-pin/caddy:pinned` deben
estar preaprovisionados: el runner ya no crea ni cambia etiquetas del host.

Usa un único Debian 13 con systemd, privilegios y cgroup v2, `--network none`,
sin socket del anfitrión. Transfiere imágenes **locales** mediante
`docker save | docker exec -i ... docker load`, resuelve sus IDs SHA-256 en el
host y genera claves SSH/age, secretos válidos y certificados dentro del nodo.
La raíz offline nunca se copia al directorio desplegado.

El ensayo implementado comprueba:

- SOPS/age real y rotación con equivalencia del contenido descifrado.
- Syntax-check, `ansible-lint --offline --profile production`, yamllint y
  Tofu `fmt -check`, `init`, `validate`, `plan` local sin providers externos.
- Convergencia de `site.yml`, migraciones y ocho servicios reales healthy.
- Panel HTTPS verificado contra la CA interna de Caddy a través de HAProxy.
- SSH endurecido, nftables deny-by-default y puertos publicados exactos de
  Docker (`docker compose ps` + `docker inspect`), secretos 0400 con UID/GID
  999 o 65532 y segunda convergencia `changed=0`.
- Namespace obsoleto provocado por reiniciar solo netns: HAProxy/relay deben ser
  unhealthy y sus probes fallar; la unidad reconstruye los tres compartidores.
  Después, tres reinicios reales consecutivos de Docker exigen ocho healthy,
  mismo inode de red en los tres y probes correctos, sin recreación manual.
- Upgrade feliz a un digest local distinto (misma aplicación, metadatos nuevos).
  Otro digest existente pero incompatible como coordinator fuerza fallo de salud;
  exige error del playbook, restauración exacta del mapa/entorno y salud recuperada.
- Tras apagar el principal, `relay.yml` con relay+STUN reales, sus puertos
  restringidos, único secreto `relay.key` y segunda convergencia sin cambios.

El contenedor exterior y sus volúmenes anónimos se eliminan al salir; todos
los contenedores/redes/volúmenes del Docker anidado desaparecen con él. No se
tocan recursos de otros laboratorios ni se publica en GHCR.
Para diagnóstico, `KEEP_INFRA=1` conserva el contenedor tanto si falla como si pasa;
su nombre aparece en la salida. Eliminarlo después con `docker rm -fv NOMBRE`;
no usar esa opción como almacenamiento de secretos o copias.

### Resultado real (2026-10-25)

Una ejecución completa de
`KEEP_INFRA=1 INFRA_IMAGE=atenea-infra:local bash tests/infra/run.sh`
terminó con código **0** en **182,82 s** de pared (183 s en el contador del
script); la primera convergencia real consumió **44 s**, incluidas migraciones.
Se usó la imagen de herramientas preaprovisionada con autorización previa;
esa preparación no forma parte del tiempo del ensayo offline.

```text
Passed: 0 failure(s), 0 warning(s) on 15 files. Profile 'production' was required, and it passed.
principal inicial:       ok=37 changed=20 unreachable=0 failed=0
principal convergencia:  ok=30 changed=0  unreachable=0 failed=0
PASS principal: 7 healthy; docker ps/inspect y nftables puertos exactos; secretos 0400/UID; SSH
PASS panel HTTPS tras HAProxy/Caddy con CA interna
upgrade feliz:           ok=23 changed=5  unreachable=0 failed=0
upgrade forzado:         ok=26 changed=6  unreachable=0 failed=1 rescued=2
Actualización fallida; rollback al mapa de digests anterior saludable.
PASS principal: 7 healthy
PASS upgrade feliz y upgrade fallido con rollback al mapa anterior
relay inicial:           ok=36 changed=9  unreachable=0 failed=0
relay convergencia:      ok=33 changed=0  unreachable=0 failed=0
PASS relay: 2 healthy; docker ps/inspect y nftables puertos exactos; secretos 0400/UID; SSH
Success! The configuration is valid.
Plan: 2 to add, 0 to change, 0 to destroy.
INFRA OK: stack real, migraciones, HTTPS, digests, upgrade/rollback, relay+STUN, secretos e idempotencia.
Ensayo terminado: 183 s; código=0; red externa deshabilitada
```

Syntax-check de los tres playbooks, yamllint, comprobación de modos públicos
0644/directorios 0755 y `tofu fmt -check -recursive` también terminaron sin error.
El fallo del upgrade forzado es intencionado: el runner exige tanto ese error
como la recuperación saludable y la restauración exacta de ambos manifiestos.
No se modificó el Compose productivo ni el mensaje genérico de error del relay.
El diagnóstico previo aisló `relay.pem` 0600 de root como causa del fallo:
cambiar únicamente su modo a 0644 recuperó los siete servicios; el rol corrige
ahora esos permisos explícitamente, sin relajar los secretos.

**Descargas durante esta ejecución: ninguna.** El nodo exterior tiene
`--network none`, los procesos auxiliares offline también y Compose prohíbe pull.
La imagen de herramientas (1,15 GB locales) y las etiquetas
`atenea-infra-pin/haproxy:pinned`, `atenea-infra-pin/caddy:pinned` se conservan
para repetir el ensayo; no son despliegues. Los dos contenedores propios usados
para diagnóstico y ejecución final se eliminaron; no quedan sus redes, volúmenes
ni contenedores anidados.

**SIN COMPROBAR:** contratación/proveedor real, DNS real/RFC2136, backend remoto y
bloqueos, consola/SSH remoto, NTP real del VPS, firewall externo, alcanzabilidad
IPv4/IPv6 UDP/TCP 443 y UDP 3478, ACME público y su renovación, conmutación entre
proveedores, y actualización desde un release publicado firmado.
Esta evidencia es de laboratorio: no tocó infraestructura real ni publicó imágenes.

### Revalidación tras gestionar el ciclo Docker (2026-10-25)

Dos ejecuciones completas **consecutivas y secuenciales** de
`INFRA_RELAY_IMAGE=atenea-stack-relay-health:local INFRA_IMAGE=atenea-infra:local bash tests/infra/run.sh`
terminaron con **código 0**, en **487 s / 476 s** según el runner
(488,57 / 476,16 s de pared exterior). Convergencia inicial: **57 / 55 s**.
Ambas segundas convergencias del principal y relay devolvieron
`ok=33 changed=0 failed=0`; ambas exigieron ocho servicios healthy en el principal,
upgrade feliz, fallo forzado con rollback, HTTPS verificado y relay+STUN.

Cada ejecución reinició solo netns para dejar los consumidores en la red vieja:
HAProxy y relay se volvieron **unhealthy**, sus probes devolvieron **1**, y
`systemctl restart atenea-stack.service` recuperó los ocho. Se observaron a
**170,036 / 168,235 s**, incluyendo los 30 reintentos de salud existentes y probes
finales; no son tiempos de detección instantánea ni de recuperación.
Los tres reinicios posteriores de Docker (`live-restore=true` productivo)
convergieron a ocho healthy en **26,205 / 26,222 / 26,149 s** y
**25,922 / 25,996 / 25,936 s**. Se comprobaron inodos iguales en los tres
compartidores y probes 0 en cada repetición; la unidad, no el script, recreó la entrada.

Salida final literal en las dos pasadas:

```text
PASS namespace obsoleto: probes fallan; unidad reconstruye entrada
PASS Docker restart after: 3 repeticiones reales
PASS panel HTTPS tras HAProxy/Caddy con CA interna
PASS upgrade feliz y upgrade fallido con rollback al mapa anterior
PASS relay: 2 healthy; docker ps/inspect y nftables puertos exactos; secretos 0400/UID; SSH; idempotencia si se solicita
INFRA OK: stack real, migraciones, HTTPS, digests, upgrade/rollback, relay+STUN, secretos e idempotencia.
```

Ambos contenedores y sus volúmenes anónimos se eliminaron, igual que el nodo
retenido de diagnóstico. No se descargaron ni instalaron imágenes, crates,
paquetes o herramientas; Docker anidado estuvo `--network none`. No se alteraron
tags protegidos del host. El detalle frío antes/después y las limitaciones están
en [Caídas y leases](caidas-y-leases.md#ampliación-reinicio-del-motor-docker).

## Despliegue del piloto (2026-10-27)

Un único VPS Debian 13 (6 vCPU, 11,7 GiB de RAM), desplegado con este rol desde un contenedor de herramientas, con `v0.1.0` (imágenes por digest `linux/amd64`). Los nombres, direcciones, inventario y secretos reales viven en un inventario privado fuera del repositorio.

**Verificado en el nodo y desde Internet:** los ocho servicios `healthy`; migraciones 1–5; certificado del panel emitido por Let's Encrypt con HTTP-01 (el puerto 80 es alcanzable desde fuera); HTTPS con la cadena pública sin desactivar la verificación y redirección 80→443; relay por TLS 443 con su SNI y ALPN `atenea-relay/2` verificado contra la CA del piloto, y rechazo del ALPN `atenea-relay/1`; STUN 3478/udp y relay 443/udp (IPv4) con COOKIE válida y silencio ante un `relay_id` ajeno; login OIDC real (Dex) con cookie `Secure` y escritura sin CSRF rechazada; acceso SSH solo desde la red de administración, `root` y contraseña desactivados; segunda convergencia con `changed=0` (33 tareas correctas).

**Hallazgos del despliegue real:**

- El VPS traía Docker CE (repositorio de Docker) y el rol instala el motor de Debian: **no son compatibles**. Antes de converger se retiró Docker CE y su repositorio (0 contenedores, 0 volúmenes). El rol no gestiona esa migración.
- **IPv6 no se sirve:** Docker publica los puertos solo en IPv4 (`0.0.0.0`), por lo que un registro AAAA apunta a una dirección sin servicio aunque el nodo tenga IPv6. Este documento ya decía no publicar AAAA sin ruta IPv6 operativa: hay que publicar solo registros A mientras el stack no escuche en IPv6.
- Un `ANSIBLE_REMOTE_TEMP` fijo creado por `root` en la primera fase impide a otro usuario trabajar en la segunda: usar el predeterminado por usuario.
- El cortafuegos del rol no abre el UDP 41641 de Tailscale: si el acceso de administración va por Tailscale, funcionará por relay DERP, con más latencia.
- El certificado TLS del relay lo firma una CA propia del piloto (caduca a los 90 días, renovación manual): los agentes necesitan `relay_ca_file`.
- **El coordinator no era alcanzable por el proxy (corregido en `c05871c`).** El agente firma la ruta exacta de cada petición y el `Caddyfile` publicaba el coordinator en `/coord/*` quitando el prefijo, así que la firma no podía coincidir y el agente recibía `acceso denegado`. `tests/stack` no lo detectó porque el agente habla directo con el coordinator. Ahora se publica `/v1/*` sin reescribir la ruta y los agentes usan `coordinator.url = "https://PANEL_HOST"`.
- **Un cambio en un fichero montado no llegaba al contenedor (corregido).** `ansible.builtin.copy` reemplaza `Caddyfile`, `haproxy.cfg`, las configuraciones públicas y los secretos de forma atómica (inodo nuevo) y los contenedores seguían montando el inodo antiguo: el host mostraba la configuración nueva mientras Caddy servía la antigua, y `compose up -d` no recrea el servicio porque su definición no cambia. Hubo que recrear Caddy a mano. Ahora `apply.yml` ejecuta `docker compose up -d --force-recreate --wait` cuando la distribución, la configuración pública o los secretos cambian y ya había contenedores (en el primer despliegue no hace nada extra; un upgrade que solo cambia digests no lo dispara). Se recrean todos los servicios juntos para conservar la unión netns/HAProxy/relay, con la interrupción breve que eso implica. Regresión en `tests/infra/in-container.sh`: añade una marca a `coordinator.toml`, converge y exige que el contenedor en ejecución la vea y se haya recreado. Con el rol anterior ese test falla (verificado con el árbol de `HEAD`); con el arreglo, `tests/infra` completo termina en `INFRA OK` (554 s).
- **El STUN queda sin clasificar:** el Compose solo publica el 3478/udp y el agente necesita también el 3479 para distinguir mapeo independiente de dependiente; sin él informa `mapping = "unknown"` (degradado, no roto).

### Enrolamiento y caminos reales (2026-10-27)

Una organización y una red (`10.77.0.0/24` y una ULA `/64`) con el certificado de emisor firmado por la raíz offline (30 días), y **dos agentes de `v0.1.0`**: un Mac arm64 (binarios macOS del release, hash comprobado contra el manifiesto firmado) y la propia VPS (`.deb` del release, hash igual al del manifiesto). Ambos se enrolaron con token de un solo uso y se aprobaron comparando la huella del bundle.

| Camino | Resultado medido |
| --- | --- |
| Directo | Funciona, pero **por Tailscale**: los dos hosts están en la misma tailnet y el agente ofrece esa IP como candidato. Ping 30/30, 16 MiB con SHA-256 íntegro (≈58 Mbit/s), RTT ≈108–111 ms. No prueba conectividad por Internet público. |
| Directo por Internet (Tailscale bloqueado) | Con el firewall del rol por defecto **no se estableció** (ambos agentes sin camino 135 s). Tras abrir el `51820/udp` de entrada con `atenea_extra_udp_ports`, **se establece**: endpoints públicos en ambos lados, ping 20/20 sin pérdida, RTT ≈106 ms, 8 MiB íntegros en 1,54 s (≈44 Mbit/s); contadores nft: 6 325 paquetes (9,2 MB) del Mac a la VPS y 2 736 de vuelta por la IP pública. La causa era el firewall de entrada de la VPS, no el NAT doméstico. |
| Relay UDP | Ambos agentes en `relay-udp`: 6/6 descargas (64 KiB, 1 MiB y 8 MiB, dos rondas) con el **mismo SHA-256**; 8 MiB en 4,8–5,9 s (≈11–14 Mbit/s); ping 20/20 sin pérdida, RTT ≈111 ms. |
| Relay TLS (HAProxy 443, SNI + ALPN) | Ambos agentes en `relay-tls`: 6/6 descargas íntegras (8 MiB en 3,4–7,3 s); ping 20/20 sin pérdida, RTT medio ≈119 ms (máx. 224 ms). |

Método: tabla `nft` temporal en la VPS (retirada siempre al terminar) para bloquear el UDP 51820 de Tailscale y del Mac, y después el UDP 443 del Mac y de la VPS hacia sí misma, con la transferencia HTTP servida por el Mac a través del túnel. Un intento previo dio un falso «SHA-256 distinto» por un servidor HTTP viejo que contestaba 404 y por un error de la comparación en awk, no por el relay.

**Hallazgos de esta prueba:**

- **Un registro AAAA sin servicio rompe el relay.** El agente usa la primera dirección que resuelve (sin alternar entre familias), así que con AAAA apuntando a un nodo que solo escucha en IPv4, el relay TLS fallaba con `tcp_connect` en ambos agentes. Hasta que el stack escuche en IPv6, publicar solo registros A. Tras borrar los AAAA, el agente de la VPS recuperó el relay UDP; el del Mac solo tras pedir una concesión nueva (conserva la anterior mientras el camino directo funcione).
- **Un nodo con agente debe abrir su UDP de escucha.** Sin `51820/udp` abierto en el firewall del rol, dos agentes (uno de ellos tras un NAT doméstico) no conseguían camino directo por Internet y quedaban sin camino hasta tener relay; abierto el puerto, el directo se establece en segundos. Para agentes en equipos normales (sin firewall de entrada estricto) no hace falta.

### Tercer par, revocación y reinicio en frío (2026-10-27)

**Tercer par.** Un contenedor Linux arm64 en Docker Desktop del Mac del operador, con el agente y la CLI compilados sin red en el laboratorio desde el árbol de trabajo (el commit aparece como `unknown`; el código del agente es el de `v0.1.0`, los commits posteriores solo tocan infraestructura y documentación). Se enroló y se aprobó comparando la huella (`10.77.0.3`). Mallado de tres: tercer par → VPS 40/40 (RTT ≈109 ms, por el `51820/udp` abierto), tercer par → Mac 40/40 (RTT ≈1,3 ms por la red local) y Mac → tercer par 6/6. Un agente tarda hasta 60 s en conocer a un par nuevo (ciclo de sincronización). **Límite:** el tercer par comparte NAT con el Mac, así que no prueba tres redes distintas.

**Revocación** del tercer par con tráfico en marcha (muestreo cada 2 s, corte = 3 fallos seguidos): tercer par → VPS cortado a los **19 s**, Mac ↔ tercer par a los **49 s**. Dependen del ciclo de sincronización de 60 s de cada agente, no del lease de 330 s. El servidor negó la configuración al dispositivo revocado (`sync_denied`, «acceso denegado»), liberó su IP y el Mac y la VPS dejaron de listarlo como par. El agente revocado conserva su última configuración hasta que expire su lease, pero ningún par lo acepta. No se midió la revocación estando todos por relay ni con más pares.

**Reinicio en frío de la VPS** (`systemctl reboot`, sin intervención): visto desde el Mac, cae a los 20 s y a los **138 s** responden SSH por la red de administración, panel HTTPS, STUN, relay UDP y el túnel con el agente de la VPS, con 8 de 9 contenedores sanos; 9/9 a los 177 s (el muestreo durante la caída tiene una resolución de unos 30 s). Desde dentro (primer reinicio, registro de la VPS): userspace completo a los 23 s del arranque del kernel, Tailscale operativo a los 7 s y los nueve contenedores iniciados a los 20 s (`netns` → `haproxy` → `relay` en orden, gracias a `atenea-stack.service`). Tras cada arranque el planificador hace una copia completa nueva, el archivado de WAL continúa sin fallos y el agente vuelve a anunciarse. 0 unidades fallidas y el UDP 51820 persiste en `nftables`. El reinicio aplicó además el kernel pendiente (`reboot-required` limpio).

**SIN COMPROBAR:** el directo por Internet con ambos agentes tras NAT domésticos distintos (aquí uno de los dos tiene IP pública), revocación por relay o con más pares, restauración de copias en un VPS nuevo, soak de 72 h, ACME del relay, cobertura IPv6, el rendimiento más allá de 8 MiB, el efecto del firewall de entrada del proveedor más allá de los puertos probados y un reinicio con la red de administración caída.


**Cliente Linux en un LXC (tercera máquina).** Contenedor Debian 13 sin privilegios en un Proxmox doméstico (2 núcleos, 4 GB de RAM, 30 GB de disco) con el `.deb` de `v0.1.0`, enrolado como tercer par (`10.77.0.3`). Mallado de tres máquinas: contenedor ↔ Mac directo por la LAN (RTT ≈1 ms), contenedor ↔ VPS directo por Internet (≈110 ms), y Mac ↔ VPS directo; sin pérdida tras el primer apretón de manos (2–5 s). **Límite:** el contenedor comparte NAT con el Mac. Detalles de instalación y la trampa de DNS en [instalación Linux](instalacion-linux.md#cliente-en-un-contenedor-lxc-de-proxmox-verificado-2026-10-27).

**Hallazgo de herramientas de medición.** Un recolector lanzado por `launchd` con el Python de Homebrew no podía llegar a máquinas de la LAN (`No route to host`): el permiso de «Red local» de macOS bloquea a los binarios sin firma de Apple y sin interfaz, mientras que `ssh`, `ping` y el Python del sistema funcionan. Además, con más de un par `peers[0]` no es estable: hay que identificar al par por su clave pública.

### Actualización del piloto a `v0.1.1` (2026-10-27)

Se aplicó con `upgrade.yml` (usuario `operador`, `atenea_backup_verified=true`) tras tomar y **verificar** una copia completa (`backup-verify`, 35 s) y traerla al Mac. Solo cambiaron las tres imágenes propias, ya fijadas por el digest del `images.json` firmado de `v0.1.1`: se descargaron antes al nodo (`linux/amd64`, los IDs coinciden con los `config_digest` del release) y un ensayo con `--check --diff` mostró exactamente esas tres referencias y las tres variables del entorno. En modo comprobación el playbook acaba con un fallo en «Validar nombres de secretos» porque no descifra secretos: es un artefacto de `--check`, no del despliegue.

Resultado de la ejecución real: `ok=27 changed=5 failed=0 rescued=0` (sin rollback), 173 s. Los nueve contenedores quedaron `healthy`; los tres propios imprimen `0.1.1` y el commit `5e3a268`. La migración `0006_device_names` se aplicó y las cinco anteriores **conservan sus checksums**. HTTPS del panel 200; `GET /api/v1/device/peers` sin firma responde 401 igual que `config`. Los agentes `0.1.0` siguieron sincronizando sin errores contra el control plane nuevo (compatibilidad hacia atrás observada en vivo).

**Agentes.** Se actualizó el `.deb` (suma comprobada contra el manifiesto firmado) en el VPS y en el contenedor de pruebas. **Hallazgo:** `dpkg -i` sobre un agente en marcha lo **dejó parado** (`prerm upgrade` lo detenía y `postinst` no lo arrancaba); en el VPS el túnel estuvo caído hasta arrancarlo a mano. Corregido en `packaging/deb/{prerm,postinst}` (se recuerda si estaba activo y solo entonces se reinicia; una instalación nueva sigue sin arrancar nada): probado con un `systemctl` simulado en cuatro casos (actualización con agente activo y parado, instalación nueva y desinstalación), **no con PID 1 real ni con un paquete construido**, y no está en `v0.1.1`. Tras reiniciar los dos agentes a la vez, el par contenedor↔VPS tardó unos tres minutos en volver al camino directo (el Mac ↔ cada uno, menos de un minuto).

**Agente del Mac.** Lo actualizó el propio operador con `update.sh` (privado, con `sudo`): comprueba los dos binarios contra el manifiesto del release, los instala en `/Library/Atenea/bin` y lo deja como servicio de `launchd` con el lanzador del repo. Resultado observado: servicio `running`, agente `0.1.1 (5e3a268…)` y **el socket de estado legible sin `sudo`**, que valida en un Mac real y bajo `launchd` la corrección del lanzador (`status_gid` abre el directorio a su grupo). El tiempo sin túnel del Mac durante el cambio no se midió. No comprobado: arranque del servicio tras reiniciar el Mac y el flujo con el paquete `.pkg`.

**Nombres.** Con los agentes `0.1.1` el control plane recibió el nombre de sistema de los tres dispositivos activos (`devices.hostname`). Después se aplicó `use_system_name` a los tres por la API (acción con auditoría; los nombres tecleados a mano `host-*` se sustituyeron por el del sistema, y siguen siendo recuperables desde el panel). La aplicación de escritorio mostró de punta a punta el nombre propio, los de los pares y sus direcciones IPv4/IPv6 (comprobado con captura de la ventana).

**Sin comprobar:** el rollback automático de `upgrade.yml` (no se activó), la restauración de la copia previa a la migración y el despliegue desde cero en un host nuevo con esta versión.

