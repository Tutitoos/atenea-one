# Agente Linux: subredes, DNS y control local

El agente aplica únicamente rutas y DNS de una configuración firmada vigente. Declarar una ruta no la autoriza: un administrador debe aprobarla y elegir explícitamente su valor de SNAT. No hay nodo de salida ni tránsito entre pares. Véanse [ADR-007](../../docs/architecture/ADR-007-subredes-dns-privilegios.md) y [configuración v2](../../docs/protocol/signed-config-v2.md).

## Configuración local

Además de las claves habituales del TOML:

```toml
interface = "atenea0"
ipc_socket = "/run/atenea/control.sock"
status_socket = "/run/atenea/status.sock"
# Sustituir por el GID numérico del grupo local autorizado a consultar estado.
status_gid = 1000
advertise_routes = ["192.168.50.0/24"]
noise_key_file = "credential:noise"
identity_key_file = "credential:identity"
```

`advertise_routes` es opcional (vacío por defecto), admite hasta ocho prefijos privados y requiere `[sync]` para declararlos al control plane. Se declara al arrancar, después de aplicar una configuración o de `reload`, y cada cinco minutos; los fallos no se registran con cuerpos ni claves y se cuentan en `sync.route_announce_errors`. Recargar una lista vacía retira una declaración anterior. `reload` vuelve a leer esta lista local y fuerza la lectura de la configuración firmada; el resto de cambios locales requiere reiniciar el servicio.

`credential:nombre` busca el fichero dentro de `$CREDENTIALS_DIRECTORY` de systemd (`LoadCredential` o `LoadCredentialEncrypted`). El nombre no puede contener `/` ni `..`; falta de directorio, clave ilegible o permisos de grupo/otros producen un error. También se admiten los ficheros privados 0600 habituales. La interfaz admite letras ASCII, números y `_`, hasta 15 caracteres: se usa como identificador de la tabla nftables.

## Aplicación por enlace

Linux necesita `ip`, `nft` y, para DNS dividido, `resolvectl` con systemd-resolved activo. El servicio necesita `CAP_NET_ADMIN`; no necesita `CAP_NET_RAW`. No modifica `/etc/resolv.conf` ni sysctls globales. El administrador debe habilitar explícitamente el forwarding de las familias servidas; `forwarding_ok` lo lee de nuevo en cada consulta de estado y en cada informe, sin esperar una nueva configuración ni intentar repararlo. Es nulo cuando la configuración aplicada no anuncia subredes.

Cada ruta de un par se instala con `ip route add PREFIJO dev INTERFAZ`. Se informa `conflict` si solapa una ruta no predeterminada de otra interfaz, cubre un endpoint del plano de control o no se puede comprobar/instalar con seguridad. La ruta predeterminada nunca se modifica. Reaplicar conserva las rutas sin cambios y elimina únicamente las que el agente instaló y han desaparecido. Al cerrar la TUN, el kernel retira sus rutas.

El servicio arranca aunque no pueda resolver los endpoints del control plane, coordinator o STUN. Mientras falte esa información, las rutas de subred quedan `pending` con motivo `endpoints_unresolved`; la TUN y las direcciones de la red del túnel siguen operativas. Reintenta cada diez segundos fuera del bucle de E/S, con una única resolución en vuelo (sin acumular hilos). Si tarda más de diez segundos, retira las rutas de subred hasta completar la resolución. Cambiar las IP resueltas vuelve a comprobar los conflictos.

El DNS configura solo servidores por enlace y dominios `~lab.internal`, nunca `~.`. Se desactiva el uso del enlace como ruta DNS predeterminada. Un sistema sin resolved informa `unsupported`; una configuración sin servidores o dominios informa `none`. Los cambios de resolved desaparecen con el enlace.

SNAT afecta exclusivamente al tráfico que entra por la TUN y sale hacia cada subred marcada `snat = true` en la configuración firmada. Las reglas se reemplazan en una transacción nftables en la tabla exclusiva `inet atenea_INTERFAZ`. Sin SNAT, la LAN necesita una ruta de retorno a la red del túnel. El agente elimina su tabla al arrancar y cerrar; tras una caída:

```sh
atenea-agent cleanup --config /etc/atenea/agent.toml
```

La tabla ausente es un éxito; un error de permisos o de nft es un error, no se oculta. Es adecuado para `ExecStopPost`.

## Sockets y estado

El socket de control tiene modo 0600 y comprueba las credenciales del proceso remoto (root o propietario del socket). Atiende `status`, `stop`, `reload` y `cleanup`. El de estado es opcional, modo 0660 y grupo `status_gid`; únicamente acepta `status`. El directorio padre también debe limitar el acceso al grupo autorizado. Un comando enviado al socket equivocado devuelve un error.

```sh
atenea status --socket /run/atenea/status.sock
atenea reload --socket /run/atenea/control.sock
atenea down --socket /run/atenea/control.sock
```

El estado IPC y los informes autenticados al control plane incluyen `routes` (`prefix`, `state` y motivo opcional), `dns` (`applied`, `unsupported`, `none`) y `forwarding_ok` (nulo si no sirve subredes). El estado es declarado por el agente, no una prueba de conectividad de la LAN.

## Pruebas sin privilegios

`netconf::Binaries` inyecta las rutas de `ip`, `nft`, `resolvectl` y del árbol de sysctls. Las pruebas unitarias usan ejecutables simulados y ficheros temporales; no cambian la red del anfitrión. La aceptación del laboratorio Linux, nftables y resolved reales corresponde al propietario y no queda sustituida por estas pruebas.
