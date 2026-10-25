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
   con ambos endpoints en `ATENEA_COORD_RELAYS` y comprobar conmutación desde fuera.
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
- `atenea_env`: mapa de valores no secretos para `.env`, incluidos
  `ATENEA_RUNTIME_DIR` igual a `atenea_stack_dir`, `PANEL_HOST`, `RELAY_HOST`,
  `CADDY_TLS` (correo ACME en producción), `CONTROL_PLANE_IMAGE`,
  `COORDINATOR_IMAGE`, `RELAY_IMAGE`. Estas últimas referencias deben incluir digest.
- `atenea_secrets_source`: fichero SOPS local del controlador, esquema
  `infra/secrets/secrets.example.yaml`. La plantilla no es un fichero desplegable:
  sustituir sus marcadores, generar material de verdad y cifrarlo.
- `atenea_stack_mode=compose`: despliega; `dry-run` copia/renderiza/descifra y
  ejecuta `docker compose config` pero **nunca hace pull/up**. No simula salud.

SOPS descifra en memoria del controlador (`no_log`); cada secreto se copia a
`secrets/` con modo **0400**, uid/gid 65532 salvo los secretos PostgreSQL
`postgres-password` y `postgres-init.sql` (999). Directorios privados 0700,
`.env` 0600. El fichero cifrado y la identidad age no se copian al nodo. No usar
`--diff` ni depuración de Ansible con material real. El esquema no acepta nombres
con barras ni nombres que incluyan `root`; sigue siendo responsabilidad del
operador no disfrazar una raíz como otro secreto.

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
En `atenea_env` necesita `RELAY_IMAGE`, `COORDINATOR_IMAGE` y `ATENEA_RUNTIME_DIR`.
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
tras verificar una copia restaurable, nuevas referencias por digest en `atenea_env`
y `atenea_health_url` accesible desde el nodo (endpoint interno, no puerto público
adicional). Guarda el `.env` realmente desplegado, aplica `pull` y
`up -d --wait --wait-timeout 180`, y exige HTTP 200. Si falla, restaura el manifiesto
anterior, hace `up -d --wait`, vuelve a comprobar HTTP y termina con error para no
ocultar la actualización fallida. En relay, elegir un endpoint de salud interno
realmente alcanzable desde el host; no habilitar HTTP de coordinator hacia Internet.

El rollback cambia **digests**, no deshace datos, configuración pública ni secretos.
No combinar una rotación incompatible con un cambio de versión. El rol no inventa
migraciones: ejecutar las del release con credencial administrativa separada y
respetar expand/contract; no borrar columnas hasta retirar la versión anterior.
La salud HTTP/Compose no demuestra compatibilidad de datos: usar además el ensayo
de upgrade/restauración de operaciones. No se afirma haber ensayado este playbook
contra un release publicado.

## Ensayo reproducible y límites

Ejecutar `bash tests/infra/run.sh`. Usa Debian 13 con systemd, contenedor
privilegiado, cgroup v2, directorios efímeros y Docker anidado; no monta el socket
del anfitrión ni usa sus volúmenes de laboratorio. Genera inventario y claves SSH/age
en runtime, cifra/descifra/rota con SOPS real, aplica roles base+docker+atenea_stack,
comprueba configuración SSH efectiva, nftables, modo 0400 y segunda convergencia
con `changed=0`. Ejecuta syntax-check, ansible-lint, yamllint y Tofu
`fmt -check`, `init`, `validate`, `plan` local.
Después configura el rol `relay_node` en el mismo contenedor (no representa otro
proveedor), comprueba que TCP 80 desaparece de las reglas cargadas, solo instala
`relay.key` en su directorio y vuelve a exigir una segunda convergencia sin cambios.

Resultado real del ensayo final (2026-10-25, Docker Desktop, 82,41 s):

```text
ansible-lint: Passed: 0 failure(s), 0 warning(s); profile production
principal segunda ejecución: changed=0 unreachable=0 failed=0
relay segunda ejecución:     changed=0 unreachable=0 failed=0
SSH, nftables deny-by-default, secretos 0400 e idempotencia: OK
Relay secundario: puertos limitados, sin secretos de firma: OK
Docker: 26.1.5+dfsg1; Docker Compose: 2.26.1-4
tofu validate: Success! The configuration is valid.
tofu plan: Plan: 2 to add, 0 to change, 0 to destroy.
INFRA OK: SOPS/age real, lint, base+Docker real, Compose dry-run;
imágenes de aplicación NO arrancadas.
```

`ansible-playbook --syntax-check` de los tres playbooks, yamllint y
`tofu fmt -check -recursive` terminaron también con código cero. Se consumió el
Compose de producción presente en el repo, no el fallback mínimo. El primer plan
crea exclusivamente dos recursos `terraform_data` locales, no servidores ni DNS.
Hubo intentos previos abortados al descargar paquetes de `deb.debian.org` con
`Unable to connect`; un reintento limpio completó el ensayo sin modificar las
reglas ni suprimir errores. La causa de esos fallos intermitentes de descarga
no está establecida: el ensayo necesita conectividad con los registros y APT,
y no se afirma estabilidad offline ni una tasa de éxito de instalaciones.

El modo del stack en este ensayo es **dry-run**: consume la estructura del Compose
de producción si está presente, pero utiliza referencias de imagen sintéticas de
ensayo, no ejecuta aplicaciones, ACME ni migraciones. Si falta Compose muestra
expresamente el fallback mínimo y falta de integración. Docker sí se instala y
arranca realmente; se comprueba `docker info` y `docker compose version`.

**SIN COMPROBAR:** contratación/proveedor real, DNS real/RFC2136, backend remoto y
bloqueos, consola/SSH remoto, NTP real del VPS, firewall externo, alcanzabilidad
IPv4/IPv6 UDP/TCP 443 y UDP 3478, ACME público y su renovación, arranque/health y
rollback del stack de producción desde Ansible, conmutación entre proveedores.
No se ha tocado ninguna infraestructura real ni publicado imágenes.
