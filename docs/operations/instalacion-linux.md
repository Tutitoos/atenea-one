# Instalación Linux (fase 7)

**Instalar primero en una VM Linux de Proxmox, nunca en el host Proxmox.** Esta fase requiere aceptación en laboratorio; no se ha validado en un Proxmox real. Usar Debian 13 con systemd, cgroup v2 y `/dev/net/tun`; paquetes para amd64 y arm64. No ejecutar el arnés privilegiado en una máquina con cargas de producción.

Para estar en varias redes a la vez y unirse con un código (`atenea network join`), véase [Varias redes con la CLI](redes-multiples-cli.md).

Desde ADR-013 fase 3, el `.deb` instala la plantilla `atenea-network@<UUID>.service`. `sudo atenea network up <UUID> --backend system` la habilita y arranca para que vuelva tras reiniciar; `down` la deshabilita y para. La unidad heredada `atenea-agent.service` y `/etc/atenea/agent.toml` continúan igual y pueden coexistir con las redes nuevas. Durante una actualización `dpkg -i` vuelven únicamente las instancias previamente activas. Desinstalar/purgar para todas las instancias y limpia sus recursos, pero conserva claves, configuraciones y directorios de red; solo `network leave` borra una identidad. Véanse los [servicios por red](redes-multiples-cli.md#servicios-por-red-y-compatibilidad).

## Verificar la descarga

El `.deb` que se descarga de GitHub Releases debe comprobarse **antes** de instalarlo: `SHA256SUMS` no va firmado, así que no prueba procedencia (quien sustituya el `.deb` en el release sustituye también ese fichero). La comprobación válida es la del manifiesto firmado (Ed25519), con `scripts/release/` del repositorio (el del tag del release, si ya incluye `--published`), en un directorio que contenga solo lo descargado (el `.deb`, `release-manifest.json` y `release-manifest.json.sig`):

```sh
bash scripts/release/verify.sh --published DESCARGAS /ruta/confiable/release.pub COMMIT_DEL_TAG VERSION
sudo apt install ./DESCARGAS/atenea-cli-*-linux-*.deb
```

La clave pública del responsable, el commit del tag y la versión se obtienen por un canal independiente del release; la clave que viaja junto al instalador no es confianza. La verificación pasa solo si la firma es válida, el commit y la versión coinciden y cada fichero descargado figura en el manifiesto con su tamaño y SHA-256; un fichero alterado o ajeno la hace fallar. Requiere Python 3.11 o posterior y OpenSSL 3 con Ed25519. Detalle y límites: [release.md](release.md#verificar-un-release-publicado).

## Construcción e instalación

La compilación se hace fuera del empaquetador. `packaging/deb/build.sh` recibe un directorio con los ejecutables Linux `atenea` y `atenea-agent`, la versión Debian, la arquitectura y el fichero de salida:

```sh
bash packaging/deb/build.sh /ruta/binarios-linux 0.7.0 arm64 /tmp/atenea_0.7.0_arm64.deb
sudo apt install /tmp/atenea_0.7.0_arm64.deb
```

Usar `amd64` para x86-64. Quien invoca garantiza arquitectura y ABI de los binarios (compilar para Debian 13; no empaquetar ejecutables macOS). Se necesitan `dpkg-deb` y utilidades GNU en el entorno constructor. El paquete depende de systemd, nftables, iproute2, bibliotecas de ejecución y certificados CA; **sugiere** systemd-resolved (no se instala solo; solo hace falta para el DNS dividido). No compila Rust ni descarga código. Para instalar con `dpkg -i`, preinstalar esas dependencias.

La instalación crea **solo el grupo** de sistema `atenea`, no un usuario: el agente corre como root. Crea `/etc/atenea` root:atenea 0750 y `/var/lib/atenea` root:atenea 0711: atravesable pero no listable, para que `atenea network list` y la aplicación de escritorio lean sin root los metadatos públicos de cada red (`networks/<uuid>/network.json`); hasta la `0.1.25` era 0700 y sin root no se veía ninguna red. Las claves, el `agent.toml` y el estado conservan sus modos (0600, directorios 0700); un fichero que el administrador deje en `/var/lib/atenea` con permisos de lectura para todos pasa a ser legible por quien conozca su ruta. Ver el estado de una red sin root exige además pertenecer al grupo `atenea` (`/run/atenea` es 0750). No crea claves, no instala un TOML activo, no habilita ni arranca el servicio, tampoco al actualizar. Los ejemplos quedan en `/usr/share/doc/atenea/`.

## Primer arranque y enrolamiento

1. Crear una red y un token en el panel existente con una cuenta `admin` u `owner`. En **Tipo**, **Dispositivo** es el valor por defecto (`role: "device"`); el administrador puede elegir **Servidor** (`role: "server"`) para que el dispositivo que consuma el token herede ese rol. En la API `POST /api/v1/networks/{net}/enrollment-tokens`, omitir `role` equivale a `device`. Ni la app ni el dispositivo eligen el rol durante el enrolamiento. Recibir su `join` y verificar el ancla raíz por otro canal. Nunca generar ni guardar la raíz o el emisor en el router.
   El rol solo sirve para presentación: no concede permisos, no cambia la conectividad y no forma parte de la configuración firmada. Después, solo `admin` u `owner` pueden cambiarlo desde **Dispositivos → Cambiar tipo**, previa confirmación; el cambio queda auditado como `device.role`, con el valor anterior y el nuevo.
2. Como root, generar las claves del dispositivo:

   ```sh
   sudo atenea device-init --dir /var/lib/atenea/device
   ```

3. Ejecutar `atenea enroll --url https://control.example.test --token "$TOKEN" --dir /var/lib/atenea/device --name router-laboratorio` como root. `--name` es opcional: sin él, el dispositivo se llama como el equipo (`/proc/sys/kernel/hostname`) y sigue el nombre si se cambia; con él, el nombre queda fijado a mano (ADR-012). Es un dominio sintético: sustituirlo. El token es de un uso; no guardarlo en scripts, historial ni registros. La CLI actual lo recibe por argumento: puede ser visible en la lista de procesos; hacerlo en una máquina de administración confiable.
4. El administrador compara la huella pública por otro canal y aprueba el dispositivo en el panel. El enrolamiento no equivale a aprobación de subredes.
5. Guardar temporalmente el `join` recibido en un fichero root 0600 y generar el TOML:

   ```sh
   sudo atenea agent-config --join /root/join.json \
     --dir /var/lib/atenea/device --out /etc/atenea/agent.toml \
     --state-dir /var/lib/atenea/state --config-file /var/lib/atenea/config.bin \
     --ipc-socket /run/atenea/control.sock \
     --status-socket /run/atenea/status.sock \
     --status-gid "$(getent group atenea | cut -d: -f3)"
   sudo chmod 0600 /etc/atenea/agent.toml
   sudo systemctl enable --now atenea-agent.service
   sudo atenea status --socket /run/atenea/control.sock
   ```

Eliminar el `join` temporal cuando deje de ser necesario (puede contener el token). La sincronización descarga la configuración firmada; sin configuración vigente el servicio puede estar activo pero el tráfico queda denegado. Consultar el estado y `journalctl -u atenea-agent`; no interpretar `active` como autorización de tráfico.

**Migración al modelo de redes (ADR-013 §10).** La instalación heredada de esta sección se puede importar como una red más con `sudo atenea network migrate-legacy` (detalles, reversión manual y cambios de ruta de sockets en [redes múltiples](redes-multiples-cli.md#migrar-la-instalación-heredada-adr-013-10)). Tras migrar no existe `/etc/atenea/agent.toml`: `atenea-agent.service` queda deshabilitada y su `ConditionPathExists` impide que arranque; el `.deb` no la reactiva al actualizarse (`prerm` solo recuerda las unidades activas). Quien nunca migra no nota ningún cambio.

## Unidad y permisos locales

La unidad usa `run --config`; `ExecStopPost` ejecuta `cleanup --config` incluso tras una caída. Solo permite `CAP_NET_ADMIN` (no `CAP_NET_RAW`), `/dev/net/tun`, familias Unix/IPv4/IPv6/netlink y escrituras en `/var/lib/atenea` y `/run/atenea`. `ProtectKernelTunables=yes` impide escribir sysctls pero conserva su lectura. `PrivateTmp` da un temporal privado. `/run/atenea` es root:atenea 0750; desaparece al parar.

- Control `/run/atenea/control.sock`: root, 0600; solo root puede parar o recargar.
- Estado `/run/atenea/status.sock`: root:atenea, 0660, con `status_gid` numérico obtenido localmente. Solo acepta `status`; pertenecer al grupo **no** concede control. Un usuario externo al grupo no puede conectarse.
- Para autorizar lectura: `sudo usermod -aG atenea USUARIO`; volver a iniciar sesión. Usar `atenea status --socket /run/atenea/status.sock` sin sudo. No conceder el grupo a usuarios que no deban conocer la topología.

No usar `atenea up` simultáneamente con systemd. Parar con `systemctl stop atenea-agent`, pues `down` podría provocar el reinicio según el modo de salida. No cambiar rutas de estado/runtime fuera de los directorios permitidos sin revisar el endurecimiento.

El agente ejecuta `/usr/sbin/ip`, `/usr/sbin/nft` y `/usr/bin/resolvectl` con entorno vacío (sin `PATH` heredado). Cada ejecutable y su directorio deben ser de root y no escribibles por grupo/otros; si no, el agente se niega a lanzarlo. Los directorios de los sockets (`/run/atenea`) deben cumplir lo mismo: el socket se crea en un subdirectorio temporal privado, se verifica y se renombra al destino; un enlace simbólico o fichero ajeno en esa ruta se rechaza.

La unidad añade filtro de syscalls `@system-service`, arquitectura nativa, `ProtectHostname`, `ProtectProc=invisible` y `PrivateMounts`. Si una distribución instala los binarios en otra ruta, fijarla con `Environment=ATENEA_IP=/ruta/absoluta` (también `ATENEA_NFT`, `ATENEA_RESOLVECTL`) en un drop-in de systemd, nunca por `PATH`; se aplican las mismas comprobaciones de propietario y permisos. Una syscall denegada por el filtro devuelve `EPERM` y queda en el diario.

## Claves y credenciales de systemd

Por defecto las claves son ficheros hex de root 0600 bajo `/var/lib/atenea/device`; no hacerlos legibles al grupo. Un respaldo debe incluir claves, ancla y estado antirretroceso; no borrar el estado para eludir un error de verificación.

Para evitar claves en claro persistentes, la fase 7 acepta `noise_key_file = "credential:noise"` e `identity_key_file = "credential:identity"`. Crear un *drop-in* con `systemctl edit atenea-agent`:

```ini
[Service]
LoadCredential=noise:/var/lib/atenea/device/noise.key
LoadCredential=identity:/var/lib/atenea/device/identity.key
```

`LoadCredential` copia claves en claro a la credencial privada del servicio; **no cifra** el original. Para cifrado en reposo usar en su lugar `LoadCredentialEncrypted`:

```sh
sudo install -d -m 0700 /etc/credstore.encrypted
sudo systemd-creds encrypt --name=noise --with-key=tpm2 /var/lib/atenea/device/noise.key /etc/credstore.encrypted/atenea-noise
sudo systemd-creds encrypt --name=identity --with-key=tpm2 /var/lib/atenea/device/identity.key /etc/credstore.encrypted/atenea-identity
```

```ini
[Service]
LoadCredentialEncrypted=noise:/etc/credstore.encrypted/atenea-noise
LoadCredentialEncrypted=identity:/etc/credstore.encrypted/atenea-identity
```

Comprobar `systemd-creds has-tpm2`: una VM necesita un TPM virtual configurado. Sin TPM2, `--with-key=host` vincula al secreto local del host, no ofrece la misma protección. Diseñar y probar la recuperación antes de retirar originales: perder TPM/clave del host puede impedir arrancar. Las credenciales descifradas existen solo durante el servicio bajo `$CREDENTIALS_DIRECTORY`, con acceso restringido. Tras modificar TOML/drop-in: `systemctl daemon-reload` y `systemctl restart atenea-agent`. Verificar funcionamiento antes de borrar de forma explícita los originales; no incluirlos en imágenes, Git ni el paquete.

## Actualización y desinstalación

Respaldar claves y estado de forma cifrada, instalar el `.deb` nuevo y arrancar explícitamente con `systemctl start atenea-agent`; la actualización para el servicio y no lo reinicia automáticamente. La habilitación previa no se altera durante una actualización.

```sh
sudo systemctl disable --now atenea-agent
sudo apt remove atenea
# También es posible:
sudo apt purge atenea
```

Ni `remove` ni **`purge` borran claves, TOML, estado ni grupo**. Conservarlos evita una pérdida irreversible; el grupo se conserva para no dejar GID huérfanos en otras ACL. Para borrado definitivo, revocar primero el dispositivo en el panel, detener el servicio, confirmar el respaldo y decidir explícitamente qué eliminar: `/etc/atenea`, `/var/lib/atenea`, sus copias y el grupo `atenea` (solo si no lo usa nadie). No hay un borrado automático ni una pregunta interactiva durante `dpkg`.

El agente no activa forwarding global, no cambia la ruta por defecto y **no edita `/etc/resolv.conf`**. DNS se aplica solo por enlace con systemd-resolved; si no está disponible se informa `unsupported`. El paquete no instala ni habilita resolved ni cambia el mecanismo DNS del host automáticamente: para el DNS dividido hay que instalarlo y habilitarlo a mano (véase la trampa de DNS más abajo si el equipo es un LXC). Véase [router de subred](router-de-subred.md).

## Arnés Docker de empaquetado

Ejecutar `bash tests/packaging/run.sh` desde el repositorio cuando el agente de fase 7 esté disponible. Reutiliza la imagen `atenea-lab` y volumen `atenea-lab-target` del laboratorio de red, compila release dentro de ese laboratorio, crea una imagen Debian 13 con systemd como PID 1 e instala el paquete. No instala nada en macOS. La imagen y el contenedor propios se eliminan al salir; no se borra el volumen de compilación.

Requiere Docker Linux con **cgroup v2**, `--privileged`, `--cgroupns=host`, montaje escribible de `/sys/fs/cgroup` y tmpfs para `/run`, `/run/lock` y `/tmp`. En Docker Desktop estas opciones afectan a su VM Linux; habilitar el motor Linux, recursos suficientes y una versión que permita cgroups escribibles y systemd. Si Enhanced Container Isolation o una política del motor los bloquea, usar una VM Linux de laboratorio compatible; no relajar la seguridad del anfitrión de producción. Si falta espacio o aparece un sistema de archivos de solo lectura, detenerse y corregir el entorno; no purgar volúmenes ajenos.

El arnés comprueba instalación, permisos, grupo, unidad, puntuación `systemd-analyze security`, arranque con configuración firmada sintética, lectura/mutación de IPC, SIGKILL y reinicio sin interfaz/tabla residual y retirada/purge conservadores. Si resolved arranca, prueba `resolvectl dns/domain` sobre un enlace dummy y la retirada al borrarlo; esto **no sustituye** el E2E de DNS firmado del agente. Si no arranca, lo comunica expresamente. Las claves de prueba nacen en el contenedor y desaparecen con él.

### Comprobación estática del paquete

En Debian 13 arm64, systemd 257.13: `bash -n` y ShellCheck 0.10.0 sin incidencias, construcción con binarios previamente compilados del volumen de laboratorio y `dpkg-deb --info/--contents` correctos. `systemd-analyze verify` aceptó la unidad **antes** del endurecimiento adicional de syscalls/procfs, con exposición **4.0 (OK)**; esa puntuación no se ha recalculado y el arnés debe repetirse. Es una evaluación estática, no una garantía de aislamiento ni una prueba de arranque. La ejecución completa con PID 1, IPC, SIGKILL y resolved queda a cargo del propietario con el arnés cuando esté integrado el agente.

## Cliente en un contenedor LXC de Proxmox (verificado 2026-10-27)

Un Debian 13 en un LXC **sin privilegios** funciona como cliente. Requisitos del contenedor: `features: nesting=1` (la unidad aplica sandboxing de systemd) y pasar el TUN del anfitrión con dos líneas en `/etc/pve/lxc/<id>.conf`: `lxc.cgroup2.devices.allow: c 10:200 rwm` y `lxc.mount.entry: /dev/net/tun dev/net/tun none bind,create=file`. El anfitrión debe tener `tun` y `nf_tables` cargados. Con 2 núcleos, 4 GB de RAM y 30 GB de disco, el agente arranca, crea `atenea0`, establece camino directo con un Mac de la misma LAN y con un VPS por Internet y mantiene la sesión. El `.deb` se instala igual que en una máquina normal (hash comprobado contra el manifiesto firmado).

**Trampa de DNS.** Hasta la `0.1.23` el paquete *recomendaba* `systemd-resolved` y `apt` lo instalaba solo; ahora solo lo *sugiere* y no se instala. Quien lo instale a mano (para el DNS dividido) en un LXC debe saber que la red la configura Proxmox, no `systemd-networkd`, así que `resolved` arranca **sin DNS aguas arriba** y toma `/etc/resolv.conf` (resolver local `127.0.0.53`): deja de resolver cualquier nombre y el enrolamiento falla con «no se pudo contactar con el control plane». Solución: un fichero `/etc/systemd/resolved.conf.d/10-lan.conf` con `[Resolve]`, `DNS=<router>` y `FallbackDNS=1.1.1.1 8.8.8.8`, y reiniciar `systemd-resolved`. **En equipos que lo recibieron por la recomendación antigua**, tras actualizar `apt autoremove` propondrá quitarlo (junto con `libnss-resolve` y `libnss-myhostname`): en una prueba con Debian 13 en un contenedor, quitarlo devolvió `/etc/resolv.conf` a un fichero normal y los nombres siguieron resolviéndose, pero el DNS dividido deja de aplicarse. Para conservarlo: `apt-mark manual systemd-resolved`. Además, la primera sincronización tras enrolar y arrancar dio una vez `acceso denegado` y se recuperó sola en el siguiente ciclo de 60 s (causa sin determinar; con otro cliente no ocurrió).

**IPv6 desactivado en el equipo.** Con `net.ipv6.conf.default.disable_ipv6=1` (o un núcleo sin IPv6) la interfaz `atenea0` nace sin IPv6 y el núcleo rechaza la dirección IPv6 del túnel («IPv6 is disabled on this device»). Hasta la `0.1.24` el agente rechazaba entonces la configuración entera (`interface_failed`) y la red no se levantaba; el rodeo era poner `net.ipv6.conf.default.disable_ipv6=0` en un fichero de `/etc/sysctl.d/`. Desde la versión siguiente, si esa dirección falla **y** el equipo tiene IPv6 desactivado en la interfaz, el agente avisa en el registro («IPv6 desactivado en atenea0; la red funciona solo por IPv4») y continúa con la dirección IPv4: el equipo es alcanzable por IPv4 y no por la dirección IPv6 del túnel. Cualquier otro fallo al asignar una dirección, y cualquier fallo de la dirección IPv4, siguen rechazando la configuración.
