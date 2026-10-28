# Varias redes con la CLI (ADR-013 fase 2)

- **Estado:** implementada y probada en el laboratorio Docker (`tests/network-lab/run-two-networks.sh`, ver más abajo). No se ha probado en un Mac ni en una máquina real. Todos los valores de los ejemplos son sintéticos.
- **Referencias:** [ADR-013](../architecture/ADR-013-redes-multiples-y-union-por-codigo.md), [código de red v1](../protocol/join-code-v1.md), instalación en [Linux](instalacion-linux.md) y [macOS](instalacion-macos.md).
- **Alcance:** solo CLI Unix (Linux y macOS). En **Windows** `atenea network …` devuelve «no está soportado todavía en Windows» (se sigue usando el flujo de [instalación Windows](instalacion-windows.md) con una sola red). Sin interfaz gráfica.

## Comandos

Todos aceptan `--root <dir>` (también `ATENEA_NETWORKS_DIR`). `join`, `up`, `down` y `leave` cambian el sistema y se ejecutan como root.

```sh
sudo atenea network join "ATENEA1-…" [--name EQUIPO] [--yes] [--insecure-http]
atenea network list [--json]
sudo atenea network up    <id|prefijo|sufijo|nombre>
sudo atenea network down  <id|prefijo|sufijo|nombre>
sudo atenea network leave <id|prefijo|sufijo|nombre> [--yes]
```

- **`join <código>`**: decodifica el código (cualquier error termina sin tocar el disco), **muestra y pide confirmación** (nombre de la red, servidor, id, **huella de la raíz**, si el código lleva token o es de solicitud, y el aviso de que una red puede instalar rutas privadas y DNS para dominios concretos). Sin terminal interactiva y sin `--yes` se niega; con `--yes` no pregunta. Después crea claves **nuevas para esa red** (`device-init`), genera el `agent.toml` con la raíz **fijada desde el código** (nunca desde el servidor) y registra el dispositivo: con token por `POST /api/v1/enroll`, sin token por `POST /api/v1/networks/{id}/join-requests` (red `con_solicitud`). Termina imprimiendo la huella del dispositivo, el estado (`pendiente_de_aprobacion` o `aprobada`) y el siguiente paso. Si falla antes de que el servidor acepte, **borra** el directorio de la red; si el servidor ya aceptó y falla algo local, lo conserva y lo explica. `--name` es el nombre del dispositivo (sin él, el del sistema). `--insecure-http` es solo de laboratorio: sustituye `https://` por `http://` en la URL del código.
- **`list`**: nombre, id corto, servidor, interfaz, puerto y estado: `detenida`, `iniciada, sin configuración` (pendiente de aprobación o sin sincronizar), `conectada`, `conflicto: …` (la configuración solapa con otra interfaz o ruta, ver límites) o `desconocida` (sin permiso para leer el socket de estado). `--json` es estable para la app: array de objetos con los campos de `network.json` más `state` (`detenida`, `iniciada_sin_configuracion`, `conectada`, `conflicto`, `desconocida`) y `detail`.
- **`up` / `down`**: arrancan o paran la instancia de esa red con su configuración; son idempotentes. La red se identifica por id completo, prefijo o sufijo único (≥ 4 caracteres) o nombre único; si es ambiguo se listan las candidatas.
- **`leave`**: confirma, para la instancia, ejecuta `atenea-agent cleanup` solo de esa red y **borra su directorio, claves incluidas** (irreversible). No toca ninguna otra red. Si la limpieza falla, no borra nada.

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

Nota (decisión fuera del contrato): el directorio de la red es 0711 en vez de 0700 para que `list` funcione sin root leyendo `network.json`. Otros usuarios locales pueden así leer, sabiendo el UUID, el host del servidor, el id y el nombre de la red, las huellas (de la raíz y del dispositivo), la interfaz y el puerto; nunca claves ni token (`device/` es 0700 y `agent.toml` 0600). Sin acceso al socket de estado (modo 0600 de root) `list` muestra `desconocida`.

| Recurso | Regla |
| --- | --- |
| Interfaz | `atenea<N>` con la N libre más baja (según los `network.json` y, en Linux, `/sys/class/net`) |
| Puerto UDP | el primero libre ≥ 51820 que no usen otras redes y que se pueda abrir (bind de prueba) |
| Sockets | `/run/atenea/<id8>/control.sock` y `status.sock` (Linux) o `/var/run/atenea/<id8>/…` (macOS); `ATENEA_RUN_DIR` lo sustituye (laboratorio). Los crea `up` (0755 root; el agente exige directorio de root y no escribible por grupo/otros) |

`<id8>` son los **últimos** 8 dígitos hex del UUID: los UUIDv7 del servidor comparten el prefijo temporal, de modo que los 8 primeros colisionan. `join` rechaza una colisión.

## Privilegios

La CLI **no se autoeleva** (opción A de ADR-013 §3.4): se ejecuta como root o con `sudo`; no invoca `sudo` ni `pkexec`. Sin permisos devuelve «permiso denegado: ejecuta con sudo». El token del código es visible en la línea de órdenes (historial, `ps`): para códigos con token se recomienda un historial desactivado.

## Límites y seguridad

- **Sin servicio del sistema** (`systemd`/`launchd`): `up` lanza el agente en segundo plano; tras un reinicio hay que volver a hacer `up`. El servicio por red es la fase 3.
- **Solapamiento.** El CIDR de la red no viaja en el código y el servidor solo lo entrega dentro de la configuración firmada tras aprobar el dispositivo; por eso se detecta **al llegar la primera configuración verificada**, no en `join`. El agente (`NetConf::overlap_with_host`, antes de `configure_link`) la rechaza —no aplica direcciones ni rutas y deja `config.last_error = "network_overlap"` con `last_error_detail`— solo si existe una ruta de **otra interfaz igual o más específica** que un prefijo de `network_prefixes` (otra interfaz reclama parte de la red del túnel; p. ej. otra red Atenea con el mismo CIDR, o `10.77.0.128/25` ajeno para `10.77.0.0/24`). Una ruta **más ancha** (`10.0.0.0/8` de una VPN, Docker o la LAN) **no** es conflicto: gana la más específica y el tráfico ya va por el túnel. Las rutas de host (/32, /128) de las propias `tunnel_addrs` se ignoran siempre, sea cual sea la interfaz que las liste: en macOS `netstat` anota `fd77::2` en `lo0`, no en `utunN`, y contarla rechazaría la configuración propia en cada reaplicación; otra Atenea con el mismo CIDR se sigue detectando por su ruta de red. Se mantiene el dispositivo TUN vacío que el agente crea al arrancar. No se comprueban solapes entre rutas de pares ni entre subredes anunciadas, ni dominios DNS repetidos. La regla está probada con fixtures de salida real de `netstat` (macOS) e `ip -j route` (Linux) y, de extremo a extremo, solo en Linux (laboratorio); **sigue sin probarse en un Mac con dos redes reales**.
- **`leave` no revoca.** Borra el estado local y las claves, pero el dispositivo sigue registrado en el servidor hasta que un administrador lo **revoque** por su huella.
- **Consentimiento.** Una red hostil puede ofrecer rutas privadas y DNS para dominios concretos (ADR-007, nunca la ruta por defecto): compara nombre, servidor y **huella de la raíz por otro canal** antes de confirmar. El código no autentica (solo detecta erratas).
- Asignación de interfaz y puerto: dos `join` simultáneos podrían elegir lo mismo; se asume un administrador a la vez.
- En macOS la interfaz se nombra `atenea<N>` en `agent.toml` pero el sistema usa `utunN`; la comprobación contra el sistema es solo de metadatos. Sin probar en macOS.

## Laboratorio `two-networks`

```sh
tests/network-lab/run-two-networks.sh   # requiere Docker y la imagen atenea-lab ya construida
```

Control plane real, PostgreSQL 17, Dex y un cliente unido con `atenea network join` a dos redes (A con código de token, B con código de solicitud aprobado comparando la huella) más un peer por red, y una tercera red de CIDR solapado. Comprueba: claves distintas, interfaz/puerto/sockets propios, ping en ambas, aislamiento (ni el cliente ni los pares cruzan A↔B, con captura), `leave` de A sin pérdida en B ni cambios en sus ficheros y recursos, revocación de B sin afectar a A, código manipulado rechazado sin tocar disco y red solapada en `conflicto`. Detalles y límites en `tests/network-lab/README.md`. Usa contenedores privilegiados; no ejecutarlo en una máquina con cargas de producción.

## Fuzzing del código

`JoinCode::decode` tiene el objetivo `join_code_parse` con semillas del vector fijo y entradas truncadas o alteradas (`fuzz/README.md`).
