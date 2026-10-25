# Cliente macOS (fase 8)

macOS es **solo cliente**: consume rutas aprobadas y DNS dividido; rechaza `advertise_routes` y no instala pf, SNAT ni forwarding. La política y el protocolo son los mismos que Linux. Requiere macOS, herramientas nativas (`ifconfig`, `route`, `netstat`) y root para abrir utun/configurar red. No usa NetworkExtension ni extensiones de kernel externas.

## Construcción sin instalar

```sh
cargo build --release -p atenea-agent -p atenea-cli --bins
packaging/macos/build-pkg.sh
pkgutil --payload-files target/macos-package/atenea-client-unsigned.pkg
```

El script usa `pkgbuild --root` y `productbuild`, un directorio temporal y la salida `target/macos-package` (ignorada por git). **No instala ni ejecuta el servicio**, no usa sudo y no modifica la red. El paquete contiene CLI, agente, plist, lanzador y desinstalador, no claves ni configuraciones. Es específico de la arquitectura del binario compilado (arm64 o x86_64), no universal.

El `.pkg` no está firmado ni notarizado. La distribución fuera de pruebas requiere Apple Developer ID Application para firmar ejecutables y Developer ID Installer para el paquete, hardened runtime según el proceso de firma elegido, envío con `xcrun notarytool submit --wait`, `xcrun stapler staple` y validación con Gatekeeper. La cuenta, certificados, perfiles y política de distribución son **decisión pendiente del responsable**; no se simulan ni se incluyen credenciales en CI. No se debe eludir Gatekeeper como estrategia de distribución.

## Instalación y configuración (administrador, máquina de pruebas)

Estos comandos **sí cambian el host**. No ejecutarlos para comprobar únicamente la construcción:

```sh
sudo installer -pkg target/macos-package/atenea-client-unsigned.pkg -target /
```

Sin `/Library/Application Support/Atenea/agent.toml`, el postinstall no registra ni arranca el demonio. `RunAtLoad` es falso; `KeepAlive/PathState` solo mantiene el servicio cuando existe ese fichero. El lanzador vuelve a comprobarlo, crea `/var/run/atenea` (0700 root:wheel) y limpia resolutores propios antes de ejecutar el agente. El plist ejecuta como root:wheel y conserva stdout/stderr en `/var/log/atenea-agent.log` (administrar rotación de logs con las herramientas del sistema). No sustituye una configuración firmada por valores de ejemplo.

Crear las claves con `atenea device-init` en un directorio privado y obtener una configuración firmada vigente del administrador de red. Un TOML local orientativo (sustituir identificadores/ancla por los emitidos):

```toml
interface = "utun"
listen = "0.0.0.0:51820"
ipc_socket = "/var/run/atenea/control.sock"
noise_key_file = "/Library/Application Support/Atenea/device/noise.key"
identity_key_file = "/Library/Application Support/Atenea/device/identity.key"
config_file = "/Library/Application Support/Atenea/config.bin"
state_dir = "/Library/Application Support/Atenea/state"
network_id = "77777777777777777777777777777777"
device_id = "22222222222222222222222222222222"
trust_anchor = "1111111111111111111111111111111111111111111111111111111111111111"
```

`utun` pide al kernel un índice libre; `utunN` solicita ese índice concreto y falla si está ocupado. El nombre real se utiliza para rutas, MTU y exclusión de candidatos. No usar `atenea0` en macOS. Las rutas de ficheros son ejemplos del paquete, no valores predeterminados nuevos del programa. Claves y TOML deben ser 0600 root; el directorio de configuración, 0700 root:wheel.

Tras colocar configuración y claves:

```sh
sudo launchctl bootstrap system /Library/LaunchDaemons/one.atenea.agent.plist
sudo atenea status --socket /var/run/atenea/control.sock
```

Para consultar estado sin root, configurar `status_socket` y el GID numérico `status_gid` de un grupo local autorizado; cambiar explícitamente el grupo/modo del directorio `/var/run/atenea` para permitirle atravesarlo (0750), también en el lanzador del servicio. El socket de control sigue en 0600 y valida la identidad Unix del par; el de estado es 0660 y no acepta órdenes de control. No conceder lectura de claves al grupo. El backend reutiliza `peer_cred`, chown y persistencia privada Unix (0600 y fsync); no añade código unsafe al agente.

## Red y limpieza

Las direcciones se aplican con `ifconfig`, MTU firmada y alias IPv4/IPv6; IPv6 usa `inet6 -dad` (flag nativo documentado en `ifconfig(8)`) antes de añadir el alias con `prefixlen`. utun es punto a punto: se añade explícitamente la ruta IPv4 del prefijo del túnel, sin inventar gateway. Las rutas de subred usan `route -n add/delete -inet|-inet6 -net PREFIJO -interface utunN`. `netstat -rn -f inet|inet6` alimenta la política común de conflictos; las direcciones candidatas se extraen de `ifconfig` sin libc/getifaddrs, excluyendo utun, loopback, link-local y direcciones tentativas/deprecated/duplicadas. Los fixtures de pruebas son sintéticos, con direcciones de documentación, sin inventario del propietario.

El DNS se escribe exclusivamente en `/etc/resolver/<dominio>` con `nameserver IP`. No modifica DNS global, `/etc/resolv.conf` ni la ruta predeterminada. Rechaza dominio vacío, raíz `.` y nombres que no sean etiquetas DNS; nunca sobrescribe un resolutor ajeno. La primera línea `# Atenea One: resolver administrado` identifica los únicos ficheros que puede retirar. Hay una instancia de sistema de Atenea: la limpieza abarca sus resolutores propios, aunque cambie el índice utun tras una caída. Los cambios de rutas/direcciones se retiran al cerrar el descriptor; los resolutores se limpian al parar, arrancar y ejecutar:

```sh
sudo atenea-agent cleanup --config '/Library/Application Support/Atenea/agent.toml'
```

`cleanup` puede ejecutarse después de SIGKILL. No destruye interfaces ajenas. Para parar sin que KeepAlive lo reinicie, usar `launchctl bootout system/one.atenea.agent`; una orden `atenea down` aislada permite el reinicio de launchd mientras siga existiendo la configuración.

## Desinstalación

```sh
sudo /usr/local/bin/atenea-uninstall.sh
```

Descarga launchd, limpia resolutores propios, elimina binarios/plist/sockets y olvida el recibo `one.atenea.client`. **Conserva claves, configuración, estado antirrollback y logs** deliberadamente. El administrador puede eliminarlos manualmente solo cuando retire la identidad del dispositivo; no se borran secretos ni el estado por una actualización o una desinstalación accidental.

## Evidencia y límites

El job `macos` ejecuta fmt, clippy (`-D warnings`), tests de túnel/agente/CLI y check del workspace sin privilegios; construye/lista el paquete sin instalarlo. `tests/macos/run.sh` exige root y `GITHUB_ACTIONS=true` y solo se destina a una VM desechable: emite claves/configuraciones v1/v2 al momento, abre la utun real, comprueba IPv4/IPv6, MTU, rutas de subred y ruta por defecto intacta, resolutor dividido, permisos 0600/0660, rechazo de otro usuario, ecos reales con `atenea-testpeer`, parada y SIGKILL seguido de cleanup y limpieza de arranque.

El supervisor acota cada orden externa (10 s por defecto), cada lectura IPC (3 s), el proceso del par (100 s), cada ejecución del agente (240 s) y la aceptación completa (alarma de 540 s y watchdog externo de 600 s). `ping` usa salida numérica; IPv4 añade `-t 3` además de `-W`, que en Darwin no es un deadline global. No se ejecuta código Python entre fork y exec para cambiar de usuario. Cada fase se escribe inmediatamente a stderr y `trap EXIT` vuelca ambos logs, `ifconfig`, `netstat -rn` y el contenido de `/etc/resolver`, mata los grupos de procesos y ejecuta cleanup con plazo; no depende del `finally` de la prueba. El paso de CI tiene límite de 12 minutos y el job de 20.

Las regresiones del supervisor se ejecutan sin privilegios con `python3 -m unittest discover -s tests/macos -p 'test_*.py'`; usan procesos simulados y temporales, sin red. La ejecución CI 40146048454 confirmó compilación, clippy, pruebas y construcción/listado del paquete; la aceptación privilegiada quedó bloqueada tras la última emisión registrada y se canceló. Aquel script no registraba fases ni acotaba todas las órdenes y la limpieza, por lo que ese último mensaje no identifica por sí solo dónde se bloqueó; el siguiente runner debe aportar el diagnóstico y confirmar conectividad, no solo ausencia de bloqueo.

Escribir el job **no prueba su resultado**: la aceptación privilegiada queda pendiente del push y del resultado del runner. No se ejecuta en el Mac del propietario. También quedan pendientes instalación real del paquete, ciclo de vida real de launchd, firma/notarización y matriz de interoperabilidad Linux↔macOS↔Windows en máquinas del responsable (incluidos suspensión/reanudación, cambios de Wi-Fi y relays).
