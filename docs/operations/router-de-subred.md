# Servir una subred interna (fase 7)

**Instalar primero en una VM Linux de Proxmox, nunca en el host Proxmox.** El router es un dispositivo de confianza: una intrusión permite observar tráfico de su subred. No se ha validado esta fase sobre un Proxmox real. Preparar consola de recuperación de la VM antes de cambiar conectividad.

## Topología de ejemplo

- Red del túnel: `10.77.0.0/24`, router `10.77.0.1`.
- LAN de laboratorio: `192.168.50.0/24`, dirección LAN de la VM router `192.168.50.2`.
- Servidor DNS interno: `192.168.50.53`, dominio `lab.internal`.

Son direcciones privadas sintéticas. La VM debe alcanzar la LAN y el control plane, y no debe usar una LAN que se solape con el túnel. Verificar primero el [enrolamiento e instalación](instalacion-linux.md).

## 1. Forwarding: decisión del administrador

El agente **lee** el estado del forwarding y lo informa como `forwarding_ok`; no escribe sysctls globales. Activarlo en la VM router, nunca copiando este ajuste al host Proxmox:

```sh
sudo cp /usr/share/doc/atenea/99-atenea-router.conf.example /etc/sysctl.d/99-atenea-router.conf
sudo sysctl -p /etc/sysctl.d/99-atenea-router.conf
sysctl net.ipv4.ip_forward
```

El ejemplo activa IPv4; descomentar IPv6 solo si se sirve IPv6 y revisar las consecuencias para RA y el firewall de esa VM. Al retirar la función de router, revisar y retirar ese fichero y restaurar los valores anteriores de forma explícita: el agente no los conserva ni restaura. Permitir el forwarding necesario entre la interfaz `atenea0` y la LAN en el firewall existente. No vaciar reglas ajenas ni añadir un `accept` global; ACL del túnel y firewall del host son controles distintos.

## 2. Declarar, aprobar y permitir

Añadir en **primer nivel** de `/etc/atenea/agent.toml`, antes de `[sync]`:

```toml
advertise_routes = ["192.168.50.0/24"]
```

Reiniciar el servicio. Con sincronización configurada, el dispositivo declara su petición; no instala ni sirve esa ruta hasta recibir una configuración firmada aprobada. Como máximo ocho rutas por dispositivo. Solo se permiten redes privadas RFC 1918, CGNAT y ULA, con longitud mínima /8 IPv4 y /32 IPv6. No se permiten rutas públicas ni por defecto: **no es un exit node**.

En el panel, un administrador debe:

1. Comprobar identidad y huella del dispositivo, LAN declarada y ausencia de solapes con el túnel u otras rutas aprobadas.
2. Revisar la petición `pending` y elegir **explícitamente** SNAT sí o no; no hay valor por defecto.
3. Aprobar la ruta del dispositivo correcto.
4. Crear las políticas mínimas desde los clientes o grupos de origen hacia el prefijo y los protocolos/puertos necesarios. Una ruta aprobada **no** equivale a acceso: sin políticas se deniega todo. Las políticas seleccionan los pares que reciben la ruta.
5. Revisar el inventario: `routes` debe pasar a `installed`; si indica `conflict`, leer el motivo y corregir el solape, no borrar a ciegas rutas de la LAN. El estado es declarado por el agente y hay que comprobar su antigüedad.

La retirada de una declaración elimina la petición; rechazar una ruta aprobada la revoca. Esperar la próxima sincronización y comprobar que desaparece en los clientes; un cliente desconectado conserva su última configuración hasta que caduque el lease. No hay tránsito de tráfico entre pares a través del router.

La comprobación de conflictos ocurre al aplicar la configuración o cambiar los endpoints resueltos, no continuamente. Tras cambiar de LAN, revisar de nuevo las rutas: un solape nuevo puede persistir hasta que se reaplique la configuración. Si es necesario, parar y arrancar el agente para volver a comprobarlas antes de usar la subred.

## 3. Elegir SNAT o rutas de retorno (ADR-007 §3)

| Elección al aprobar | Qué ve la LAN | Configuración necesaria |
| --- | --- | --- |
| `snat = true` | IP LAN del router; pierde la identidad del cliente en registros | No requiere rutas de retorno nuevas en los hosts LAN |
| `snat = false` | IP real del cliente en el túnel | Ruta de retorno hacia la red del túnel a través de la VM router |

Sin SNAT, configurar en el router principal de la LAN o en cada destino una ruta como esta (ejemplo Linux; persistir según su distribución):

```sh
sudo ip route add 10.77.0.0/24 via 192.168.50.2
```

No ejecutarla indiscriminadamente en clientes del túnel ni en el host Proxmox. Para IPv6 configurar una ruta de retorno equivalente a los prefijos ULA reales del túnel. Probar ida y vuelta: que el cliente alcance la LAN no demuestra que las respuestas sepan regresar.

Con SNAT el agente mantiene su tabla nftables exclusiva `atenea_atenea0` (forma `atenea_<interfaz>`). No modificarla a mano: se limpia al cerrar, al arrancar y con `cleanup` tras una caída. No toca las tablas del firewall del administrador. Para inspeccionar: `sudo nft list table inet atenea_atenea0`. Las rutas de cliente viven en la TUN y desaparecen al destruirla; nunca se sustituye la ruta por defecto.

## 4. DNS dividido

En DNS de la red, configurar servidores `192.168.50.53` y dominios `lab.internal`. Como máximo tres servidores y ocho dominios; dominios ASCII en minúsculas, sin punto final ni comodines. Los servidores deben quedar en los CIDR del túnel o dentro de rutas aprobadas; añadir políticas UDP/TCP 53 para los clientes necesarios. Si se revoca la ruta que permite alcanzar el servidor, deja de incluirse en las configuraciones emitidas.

El agente aplica DNS por enlace con systemd-resolved, como `resolvectl dns atenea0 192.168.50.53` y `resolvectl domain atenea0 '~lab.internal'`. No usa `~.` ni captura DNS general, no edita `resolv.conf` y no crea nombres de dispositivos. Confirmar que el resolved del host es el mecanismo que consultan las aplicaciones; arrancarlo por sí solo no garantiza esa integración. Comprobar:

```sh
resolvectl dns atenea0
resolvectl domain atenea0
resolvectl query servicio.lab.internal
atenea status --socket /run/atenea/status.sock
```

`dns: unsupported` significa que falta resolvectl o resolved no está activo; decidir con el administrador cómo habilitarlo, sin reemplazar automáticamente el DNS de la máquina. Al desaparecer `atenea0`, resolved elimina su configuración de enlace.

## Diagnóstico y retirada

Consultar `journalctl -u atenea-agent`, `forwarding_ok`, estado de rutas y DNS, antigüedad del informe, rutas del cliente y firewall de ambos lados. Las rutas que capturarían endpoints de pares, coordinator, relay o control plane se rechazan como conflicto para no cortar el propio plano de control. La LAN local que solapa con la ruta servida tampoco se pisa.

Revocar rutas en el panel, retirar `advertise_routes`, actualizar políticas y DNS que dependían de ellas y comprobar todos los clientes. Parar el servicio elimina TUN, rutas y tabla propia; el sysctl y las rutas de retorno administradas manualmente **permanecen** hasta retirarlas conscientemente. Para desinstalación y custodia de claves véase la guía de instalación.
