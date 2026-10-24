# ADR-007: Subredes, DNS, privilegios del agente y custodia de claves locales (fase 7)

- **Estado:** Propuesto (2026-10-25; implementado en laboratorio, pendiente de aceptación del responsable)
- **Relacionados:** ADR-002 (lease), ADR-005 (jerarquía de claves), `docs/protocol/signed-config-v2.md`, modelo de amenazas T23–T28.

## Contexto

Hasta la fase 6 un dispositivo solo se comunica con las direcciones de túnel de sus pares. La fase 7 permite que un dispositivo Linux («router de subred») sirva una red interna detrás de él (p. ej. las VMs de un Proxmox) a otros dispositivos, configure DNS dividido y se ejecute como servicio con privilegios acotados. Un error aquí puede romper la LAN del usuario, capturar su tráfico general (equivalente a un *exit node*) o abrir el agente a usuarios locales.

## Decisiones

### 1. Rutas de subred: anunciadas por el dispositivo, aprobadas por un administrador, firmadas en la configuración

1. El router **declara** qué prefijos puede servir (`advertise_routes` en su TOML local). Es una petición: no tiene efecto hasta que el control plane la apruebe (`pending` → `approved`). Solo una configuración firmada puede hacer que un agente instale una ruta o acepte tráfico para una subred.
2. Solo se aceptan prefijos **privados**: RFC 1918, `100.64.0.0/10` y ULA `fc00::/7`; longitud mínima /8 (IPv4) y /32 (IPv6). Nunca la ruta por defecto, ni nada que la cubra, ni loopback, enlace local, multicast o documentación. **No hay exit node**: servir Internet exigiría una decisión y un contrato nuevos.
3. Un prefijo aprobado **no puede solaparse** con los de la red del túnel ni con ningún otro prefijo aprobado de la misma red (en el control plane al aprobar, y en el agente al verificar). Elimina bucles entre routers y ambigüedad de destino.
4. Quién recibe una ruta lo decide la política: un cliente solo conoce las rutas de los pares que su configuración ya incluye (por políticas). La ACL sigue siendo de denegación por defecto: la ruta sin regla no permite tráfico.

### 2. Plano de datos

- Origen de un paquete recibido de un par: su `addrs` o una de sus `routes`. Destino: una dirección propia o un prefijo que el propio dispositivo tiene aprobado (`advertised`); **nunca** se reenvía entre pares (no hay tránsito): un paquete para la ruta de otro par recibido de un par se descarta.
- ACL en el origen y en el destino con la configuración local de cada uno (sin cambios de modelo). Los selectores de prefijo pueden apuntar a rutas aprobadas.
- El agente **no cambia sysctls globales**: comprueba `ip_forward`/`forwarding` y lo informa (`forwarding_ok`). Activarlo es una decisión del administrador del host (se entrega un ejemplo en `/usr/share/doc/atenea/`). Así no hay nada que restaurar.

### 3. SNAT: decisión explícita por ruta, sin valor por defecto

Cada ruta aprobada lleva `snat` (booleano) elegido por el administrador al aprobarla; la API no admite omitirlo.

| `snat` | Efecto | Coste |
| --- | --- | --- |
| `false` | Los hosts de la LAN ven la IP de túnel del cliente real. Necesitan una **ruta de retorno** a la red del túnel por el router. | Mantener rutas en la LAN; se conserva identidad en registros. |
| `true` | El router enmascara (nftables, tabla propia `atenea_<interfaz>`) lo que sale hacia la LAN. No hace falta tocar la LAN. | La LAN no distingue clientes; los registros de los hosts pierden el origen real. |

La tabla nftables es propiedad exclusiva del agente: se crea al aplicar una configuración, se elimina al cerrar y **se borra al arrancar y con `atenea-agent cleanup`** (por si el proceso murió sin limpiar).

### 4. Instalación de rutas en el cliente sin pisar la red local

Una ruta aprobada no se instala (y se informa `conflict` con el motivo) si: se solapa con una ruta no-por-defecto ya presente por otra interfaz (p. ej. la LAN local con el mismo prefijo), o cubre la dirección de un par, del coordinator, de un relay o del control plane (evita capturar el propio plano de control). La ruta por defecto jamás se modifica. Las rutas viven en la interfaz TUN: al cerrar el descriptor el kernel las elimina (sin residuo tras cierre o caída).

### 5. DNS dividido

La red define servidores (direcciones alcanzables por el túnel) y **dominios de enrutamiento**. El agente los aplica **solo** con `systemd-resolved` por enlace (`resolvectl dns/domain atenea0 … ~dominio`), sin `~.`: nunca captura el DNS general. Al desaparecer el enlace, resolved descarta la configuración (restauración automática). Sin `resolvectl` o sin resolved activo se informa `dns: unsupported` y **no** se edita `/etc/resolv.conf`. No hay resolución de nombres de dispositivo propia (fuera de alcance).

### 6. Privilegios y control local

- El agente corre como root con las capacidades mínimas que da systemd (`CAP_NET_ADMIN`; `CAP_NET_RAW` no) y endurecimiento (`NoNewPrivileges`, `ProtectSystem=strict`, etc.); se evita un usuario sin privilegios porque `resolvectl` y nftables exigirían reglas polkit adicionales.
- **Dos sockets Unix:** `ipc_socket` (control: parar, recargar, limpiar) con modo 0600 propietario root, y `status_socket` opcional (solo lectura: `status`) con modo 0660 y el grupo numérico `status_gid` (grupo `atenea` del paquete). El kernel impone quién puede conectar; el agente además comprueba `SO_PEERCRED` (uid 0 o el propietario) en el socket de control. Un usuario fuera del grupo no puede ni leer el estado.

### 7. Custodia de las claves locales

Las claves privadas siguen en ficheros 0600 de root. Se admite la forma `credential:<nombre>` en `noise_key_file`/`identity_key_file`: se lee de `$CREDENTIALS_DIRECTORY` (systemd `LoadCredential`/`LoadCredentialEncrypted`), de modo que el administrador puede cifrarlas con `systemd-creds` (TPM2 cuando existe) sin cambiar el agente. Cargar claves sin permisos estrictos sigue siendo un error. Ningún registro contiene claves, tokens ni la configuración.

## Consecuencias

- Una subred servida por un router depende de la integridad del router y de quien apruebe rutas: el panel debe mostrar con claridad las rutas pendientes y el valor de `snat` antes de aprobar.
- Sin SNAT hace falta mantener rutas de retorno; con SNAT se pierde la atribución por origen en la LAN.
- Prefijos privados únicamente: no se pueden servir rangos públicos (limitación aceptada).
- Los cambios de ruta en un cliente tardan lo que tarde en llegar la nueva configuración (≤ intervalo de sincronización); no hay rutas persistidas fuera de la TUN.
- Pendiente: probar en un host Proxmox/VM real (no hay acceso desde este entorno); el laboratorio de la fase 7 emula la LAN con espacios de nombres.
