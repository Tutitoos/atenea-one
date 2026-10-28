# Varias redes con la CLI (ADR-013 fases 2 y 3)

- **Estado:** fases 2 y 3 probadas en laboratorios Docker (Linux) y en la CI de macOS (runner desechable: instancia heredada + dos redes por `launchd`, commit `b81af18`); **nunca ejecutadas en el equipo del piloto**. Todos los valores de los ejemplos son sintéticos.
- **Referencias:** [ADR-013](../architecture/ADR-013-redes-multiples-y-union-por-codigo.md), [código de red v1](../protocol/join-code-v1.md), instalación en [Linux](instalacion-linux.md) y [macOS](instalacion-macos.md).
- **Alcance:** CLI Unix (Linux y macOS), sin interfaz gráfica. **Windows está aplazado**: `atenea network …` sigue devolviendo «no está soportado todavía en Windows»; servicio por red y adaptador Wintun por red requieren una fase propia con pruebas elevadas. El flujo de [instalación Windows](instalacion-windows.md) continúa con una sola red.

## Comandos

Todos aceptan `--root <dir>` (también `ATENEA_NETWORKS_DIR`). `join`, `up`, `down` y `leave` cambian el sistema y se ejecutan como root.
El indicador global `--backend auto|process|system` selecciona el gestor. `auto` usa systemd en Linux si es PID 1 y launchd en macOS; sin systemd conserva el arranque como proceso. `process` fuerza ese modo (laboratorios) y `system` fuerza el gestor nativo. Se conserva la selección en `network.json` (`service_backend`) y `list` la muestra como `backend`. Antes de cambiar de gestor hay que parar la instancia con su gestor actual.

```sh
atenea network inspect "ATENEA1-…" [--json]
sudo atenea network join "ATENEA1-…" [--name EQUIPO] [--yes] [--status-gid GID] [--insecure-http]
atenea network list [--json]
sudo atenea network up    <id|prefijo|sufijo|nombre>
sudo atenea network down  <id|prefijo|sufijo|nombre>
sudo atenea network leave <id|prefijo|sufijo|nombre> [--yes]
sudo atenea network up <uuid> --backend system    # habilita el servicio para el siguiente arranque
sudo atenea network up <uuid> --backend process   # laboratorio: proceso, sin servicio
```

- **`inspect <código>`**: decodifica y muestra nombre, servidor, UUID, huella de la raíz y si lleva token; no requiere root, no toca el disco ni usa la red. Nunca muestra el token. Con `--json` imprime una sola línea con este formato (valores del vector sintético de [código de red v1](../protocol/join-code-v1.md)): `{"name":"Red de ejemplo","server":"control.example.test","network_id":"01020304-0506-0708-090a-0b0c0d0e0f10","root_fingerprint":"6c4b-fd0b-2585-9aae-61ad-9159-3650-be21","has_token":false}`. Un código inválido termina con error `código no válido: …`, salida distinta de cero y sin nada en stdout.
- **`join <código>`**: decodifica el código (cualquier error termina sin tocar el disco), **muestra y pide confirmación** (nombre de la red, servidor, id, **huella de la raíz**, si el código lleva token o es de solicitud, y el aviso de que una red puede instalar rutas privadas y DNS para dominios concretos). Sin terminal interactiva y sin `--yes` se niega; con `--yes` no pregunta. Después crea claves **nuevas para esa red** (`device-init`), genera el `agent.toml` con la raíz **fijada desde el código** (nunca desde el servidor) y registra el dispositivo: con token por `POST /api/v1/enroll`, sin token por `POST /api/v1/networks/{id}/join-requests` (red `con_solicitud`). Termina imprimiendo la huella del dispositivo, el estado (`pendiente_de_aprobacion` o `aprobada`) y el siguiente paso. Si falla antes de que el servidor acepte, **borra** el directorio de la red; si el servidor ya aceptó y falla algo local, lo conserva y lo explica. `--name` es el nombre del dispositivo (sin él, el del sistema). `--insecure-http` es solo de laboratorio: sustituye `https://` por `http://` en la URL del código.
  La opción `--status-gid <u32>` escribe `status_gid = N` en el `agent.toml` de esa red para permitir que ese grupo lea su socket de estado; sin ella se conserva el acceso exclusivo de root.
- **`list`**: nombre, id corto, servidor, interfaz, puerto y estado: `detenida`, `iniciada, sin configuración` (pendiente de aprobación o sin sincronizar), `conectada`, `conflicto: …` (la configuración solapa con otra interfaz o ruta, ver límites) o `desconocida` (sin permiso para leer el socket de estado). `--json` es estable para la app: array de objetos con los campos de `network.json` más `state` (`detenida`, `iniciada_sin_configuracion`, `conectada`, `conflicto`, `desconocida`) y `detail`.
- **`up` / `down`**: arrancan o paran la instancia de esa red con su configuración; son idempotentes. La red se identifica por id completo, prefijo o sufijo único (≥ 4 caracteres) o nombre único; si es ambiguo se listan las candidatas.
- **`leave`**: confirma, para la instancia, ejecuta `atenea-agent cleanup` solo de esa red y **borra su directorio, claves incluidas** (irreversible). No toca ninguna otra red. Si la limpieza falla, no borra nada.

`join`, `leave` y la asignación de interfaz/puerto se serializan con un bloqueo exclusivo del directorio raíz (`File::lock`); también `up` y `down` lo respetan. La asignación reserva los recursos declarados de la instancia heredada aunque esté parada, y comprueba los puertos contra el sistema.

## Disposición en disco

Raíz por defecto: `/var/lib/atenea/networks` (Linux) y `/Library/Application Support/Atenea/networks` (macOS).

```
<raíz>/<uuid>/                 # UUID en minúsculas, modo 0711 (ver nota)
├── network.json               # 0644: metadatos NO secretos
└── device/                    # 0700
    ├── noise.key  identity.key  bundle.bin   # claves nuevas para esta red
    ├── agent.toml             # 0600: raíz del código, sync contra la URL del código
    ├── config.bin  state/     # los crea el agente
    └── agent.log
```

`network.json`: `network_id`, `name`, `server`, `url`, `root_anchor_fingerprint`, `device_id`, `device_fingerprint`, `interface`, `listen_port`, `created` (segundos Unix) y `join_status`. **El token del código no se guarda nunca en ningún fichero.**

Nota (decisión fuera del contrato): el directorio de la red es 0711 en vez de 0700 para que `list` funcione sin root leyendo `network.json`. Otros usuarios locales pueden así leer, sabiendo el UUID, el host del servidor, el id y el nombre de la red, las huellas (de la raíz y del dispositivo), la interfaz y el puerto; nunca claves ni token (`device/` es 0700 y `agent.toml` 0600). Sin acceso al socket de estado `list` muestra `desconocida`.

| Recurso | Regla |
| --- | --- |
| Interfaz | `atenea<N>` con la N libre más baja (según los `network.json` y, en Linux, `/sys/class/net`) |
| Puerto UDP | el primero libre ≥ 51820 que no usen otras redes y que se pueda abrir (bind de prueba) |
| Sockets | `/run/atenea/<id8>/control.sock` y `status.sock` (Linux) o `/var/run/atenea/<id8>/…` (macOS); `ATENEA_RUN_DIR` lo sustituye (laboratorio). Los crea `up` (0755 root; el agente exige directorio de root y no escribible por grupo/otros) |

`<id8>` son los **últimos** 8 dígitos hex del UUID: los UUIDv7 del servidor comparten el prefijo temporal, de modo que los 8 primeros colisionan. `join` rechaza una colisión.

## Servicios por red y compatibilidad

La instancia heredada continúa con `atenea-agent.service` y `/etc/atenea/agent.toml` (Linux), o `one.atenea.agent` y `/Library/Application Support/Atenea/agent.toml` (macOS). No se convierte ni se reutiliza como servicio de una red. Puede coexistir con varias redes; se reservan su interfaz y puerto, y los sockets por red quedan separados.

### Linux (systemd)

El `.deb` instala `atenea-network@.service` con el mismo endurecimiento que la unidad heredada. `network up` escribe un drop-in root para la configuración `<root>/<UUID>/device/agent.toml`, limita la escritura al dispositivo de esa red y configura `RuntimeDirectory=atenea/<id8>`; después ejecuta `systemctl enable --now atenea-network@<UUID>.service`. Así vuelve tras arrancar el sistema. El lanzador `/usr/lib/atenea/network-agent` valida el UUID y prepara el directorio de ejecución propio como root:atenea 0750. El padre compartido se conserva para que parar la instancia heredada no retire los sockets de otras redes.

`network down` ejecuta `systemctl disable --now` (no vuelve tras reiniciar); `leave` además elimina el drop-in y las claves locales. Una actualización con `dpkg -i` reinicia únicamente las instancias que estaban activas (incluida la heredada); una instalación nueva no activa nada. Remove/purge paran y deshabilitan todas las instancias, limpian sus interfaces y **conservan claves, configuraciones y directorios de red**. No es lo mismo que `network leave`.

### macOS (launchd)

El CLI genera `/Library/LaunchDaemons/one.atenea.network.<id8>.plist` como root:wheel 0644, con `KeepAlive/PathState` sobre su configuración. Ejecuta `launchctl enable` y `bootstrap` para esa etiqueta; `down` hace `disable` y `bootout` para que no vuelva tras reiniciar. El lanzador `atenea-launchd.sh [UUID [ROOT]]` conserva exactamente el modo heredado sin argumentos y valida UUID, ruta absoluta, cadena de propietarios root y ausencia de enlaces o escritura de grupo/otros. No se admite un `--root` bajo `/tmp` ni una ruta de usuario para un servicio; en modo proceso sí sirve para pruebas.

Las redes llevan `resolver_owner = "<id8>"` en su TOML; los resolutores tienen la marca `# Atenea One: resolver administrado (<id8>)`. Solo la instancia heredada usa la marca antigua sin id. Cada parada/cleanup retira exclusivamente sus propios ficheros: nunca sobrescribe ni borra los de otra instancia ni los ajenos. Un dominio ya reclamado provoca `dns_domain_conflict` en `config.last_error` **antes de aplicar direcciones, rutas o DNS**, también al actualizar solo DNS.

El desinstalador retira servicios/plists y limpia cada red sin borrar sus claves ni directorios; `postinstall` conserva el comportamiento heredado y reinicia solo las redes cargadas, no las deshabilitadas, esperando a que cada servicio quede descargado antes de volver a cargarlo (`bootout` es asíncrono). Esto se comprobó en la CI de macOS; **no ejecutar `tests/macos/run.sh`, `postinstall`, desinstaladores ni `launchctl` en el equipo del piloto**. La CI no ejercita en macOS `network_overlap` ni `dns_domain_conflict` (solo pruebas unitarias y, el solape, el laboratorio de Linux).

## Migrar la instalación heredada (ADR-013 §10)

Una red = una instancia. La instalación heredada (un único `agent.toml`) se importa como una red más; sin redes unidas el equipo no está conectado a nada. Todos los valores de los ejemplos son sintéticos.

```sh
atenea network legacy [--json]                      # sin root: {"present": true|false}
sudo atenea network migrate-legacy [--name EQUIPO] [--yes]
```

- **`legacy`** solo comprueba con `Path::exists` si existe el `agent.toml` heredado (`/etc/atenea/agent.toml` o `/Library/Application Support/Atenea/agent.toml`) y que su red no esté ya importada. Texto: «Hay una instalación heredada pendiente de importar» / «No hay instalación heredada».
- **`migrate-legacy`** (root; Windows: «no soportado») valida todo antes de tocar nada (UUID bien formado, sin colisión de id corto con otras redes, claves presentes, nombre válido y no repetido; sin `--name` se usa el servidor), **muestra red, servidor y huella de la raíz y avisa de que se parará la instancia heredada y el túnel se cortará unos segundos**. Sin terminal y sin `--yes` se niega. Después: para y deshabilita la heredada (systemd `disable --now` / launchd `disable`+`bootout`) y ejecuta `cleanup`; **mueve** (no copia) claves, `bundle.bin`, `config.bin` y `state/` a `<raíz>/<uuid>/device/`; reescribe `agent.toml` con rutas nuevas, sockets en `<run>/<id8>/`, `resolver_owner = <id8>`, mismo puerto UDP, interfaz y `status_gid`; escribe `network.json` (`join_status = aprobada`); deja el respaldo `<raíz>/<uuid>/legacy-agent.toml.bak` (0600); borra el `agent.toml` heredado y arranca la red como cualquier otra (`network up`).
- **Si algo falla después de parar la heredada**, se devuelven los ficheros, se borra el directorio de red a medias y se reinicia la heredada; el error lo cuenta. Si la restauración misma falla, se conserva el directorio nuevo (con el respaldo) y el error lo dice. Repetirlo con la red ya importada da un error claro sin cambios.
- **La heredada no vuelve sola:** sin `agent.toml` heredado, `atenea-agent.service` no arranca (`ConditionPathExists=/etc/atenea/agent.toml`) y `one.atenea.agent` tampoco (`KeepAlive/PathState` sobre su `agent.toml`; además queda deshabilitada). El `.deb` solo reinicia en una actualización las unidades que estaban activas (`prerm` registra las activas; `postinst` arranca únicamente esas), y el `.pkg` solo recarga `one.atenea.agent` si `agent.toml` existe (`postinstall`); ninguno la resucita.
- **Un host que nunca migra** sigue funcionando igual.

### Reversión manual

Con la red ya importada (`<id>` = su UUID): `sudo atenea network down <id>`; `sudo atenea network leave` **no** (borraría las claves). En su lugar: mover `<raíz>/<id>/device/{noise.key,identity.key,bundle.bin,config.bin,state}` a las rutas de `legacy-agent.toml.bak`, copiar ese fichero a la ruta heredada con el modo original (0640 root:atenea en Linux), quitar el drop-in/plist de la red (`/etc/systemd/system/atenea-network@<id>.service.d` o `/Library/LaunchDaemons/one.atenea.network.<id8>.plist`), borrar `<raíz>/<id>` y arrancar la heredada (`systemctl enable --now atenea-agent` / `launchctl enable system/one.atenea.agent` + `bootstrap`). Hazlo con el equipo sin tráfico crítico.

### Herramientas que cambian de ruta

El socket de estado heredado `/var/run/atenea/status.sock` (macOS) o `/run/atenea/status.sock` (Linux) **deja de existir**; el nuevo es `<run>/<id8>/status.sock` (el `<id8>` son los 8 últimos hex del UUID). Hay que actualizar: los scripts de soak del propietario, `atenea status --socket …` y `ATENEA_STATUS_SOCKET` de la aplicación (que lee el estado de las redes por `network list --json`). Los sockets de control pasan igual a `<run>/<id8>/control.sock`; `journalctl -u atenea-agent` pasa a `journalctl -u atenea-network@<UUID>`; en macOS el registro pasa de `/var/log/atenea-agent.log` a `<raíz>/<uuid>/device/agent.log`.

## Aplicación de escritorio

La sección **Redes** de la aplicación (ADR-013 fase 4, [ADR-011 §2](../architecture/ADR-011-aplicacion-de-escritorio.md)) usa solo estos comandos: `list --json` y `inspect --json` sin privilegios, y `join`, `up`, `down` y `leave` con la autorización del sistema operativo (macOS: diálogo de administrador; Linux: `pkexec`) en cada operación. Unirse ejecuta `join … --yes --status-gid <grupo primario del usuario>` y a continuación `up`, en una sola autorización. El token de un código con token es visible un instante en la línea de órdenes de esa operación. Windows no está soportado. **La elevación real no se ha probado**; consulta los límites de ADR-013 §9.

## Privilegios

La CLI **no se autoeleva** (opción A de ADR-013 §3.4): se ejecuta como root o con `sudo`; no invoca `sudo` ni `pkexec`. Sin permisos devuelve «permiso denegado: ejecuta con sudo». El token del código es visible en la línea de órdenes (historial, `ps`): para códigos con token se recomienda un historial desactivado.

## Límites y seguridad

- **Socket de estado.** El de una red unida sin `--status-gid` es solo de root, de modo que `list` sin root muestra `desconocida` cuando el socket está activo pero no puede leerlo.
- **Modo proceso:** solo en contenedores sin systemd o con `--backend process`; no vuelve tras reiniciar. Linux/systemd y macOS/launchd sí tienen servicio persistente por red.
- **Solapamiento.** El CIDR no viaja en el código: se detecta al llegar la primera configuración verificada, no en `join`. `NetConf::overlap_with_host`, antes de `configure_link`, rechaza con `network_overlap` y detalle si otra interfaz tiene una ruta **igual o más específica** que un prefijo de `network_prefixes` (mismo CIDR o `10.77.0.128/25` ajeno dentro de `10.77.0.0/24`). Una ruta más ancha (`10.0.0.0/8` de VPN/Docker/LAN) no es conflicto: gana la más específica. Ignora siempre las rutas host (/32 o /128) de las propias `tunnel_addrs`, aunque macOS las liste en `lo0`; otra Atenea con el mismo CIDR se detecta por su ruta de red. El dispositivo TUN creado al arrancar queda vacío si se rechaza. No se comprueban solapes entre rutas de pares ni subredes anunciadas. La colisión de dominios DNS se comprueba por separado en macOS (apartado anterior). Hay pruebas de parseo con fixtures sintéticos de formato real `netstat`/`ip -j route` y pruebas de extremo a extremo Linux; **no con dos redes reales en un Mac**.
- **`leave` no revoca.** Borra el estado local y las claves, pero el dispositivo sigue registrado en el servidor hasta que un administrador lo **revoque** por su huella.
- **Consentimiento.** Una red hostil puede ofrecer rutas privadas y DNS para dominios concretos (ADR-007, nunca la ruta por defecto): compara nombre, servidor y **huella de la raíz por otro canal** antes de confirmar. El código no autentica (solo detecta erratas).
- En macOS la interfaz se nombra `atenea<N>` en `agent.toml` pero el sistema usa `utunN`; la comprobación contra el sistema es solo de metadatos. Sin probar en macOS.

## Laboratorio `two-networks`

```sh
tests/network-lab/run-two-networks.sh   # requiere Docker y la imagen atenea-lab ya construida
```

Control plane real, PostgreSQL 17, Dex y un cliente unido con `atenea network join` a dos redes (A con código de token, B con código de solicitud aprobado comparando la huella) más un peer por red, y una tercera red de CIDR solapado. Comprueba: claves distintas, interfaz/puerto/sockets propios, ping en ambas, aislamiento (ni el cliente ni los pares cruzan A↔B, con captura), `leave` de A sin pérdida en B ni cambios en sus ficheros y recursos, revocación de B sin afectar a A, código manipulado rechazado sin tocar disco y red solapada en `conflicto`. Detalles y límites en `tests/network-lab/README.md`. Usa contenedores privilegiados; no ejecutarlo en una máquina con cargas de producción.

El ensayo de servicios Linux se ejecuta con `ATENEA_PACKAGING_IMAGE=atenea-infra:local ATENEA_PACKAGING_NO_DNS=1 tests/packaging/run.sh`: systemd PID 1, instalación/actualización/retirada del `.deb`, legacy más dos redes, TCP real, habilitación y `daemon-reexec`, conservando claves tras remove/purge. Se reutiliza una imagen existente sin red ni descargas. **No se ejercita DNS dividido con systemd-resolved ni shellcheck en este modo**; el ensayo completo por defecto sigue requiriendo la imagen de `tests/packaging/Dockerfile`, reservada a CI o preaprovisionamiento autorizado.

El mismo ensayo termina migrando la heredada (`tests/packaging/migrate-legacy.sh`, con systemd y dpkg reales): comprueba la negativa sin `--yes`, la validación previa sin parar nada, que la red queda conectada por `atenea-network@<UUID>` (tráfico firmado, mismo puerto e interfaz), que la unidad heredada queda inactiva, deshabilitada y sin `agent.toml`, que no vuelve con `systemctl start`, `daemon-reexec` ni una actualización del `.deb`, la idempotencia y que remove/purge conservan las claves migradas. No ejerce el camino de restauración con systemd (solo las pruebas unitarias, en modo proceso) ni macOS.

## Fuzzing del código

`JoinCode::decode` tiene el objetivo `join_code_parse` con semillas del vector fijo y entradas truncadas o alteradas (`fuzz/README.md`).

