# Fase 7 — subredes, DNS, privilegios y paquetes (E2E)

- **Fecha:** 2026-10-25
- **Entorno:** contenedores Linux privilegiados y desechables (Docker Desktop, kernel linuxkit, nftables, iproute2, systemd 257 en Debian 13). Sin Proxmox: la LAN interna es una topología de espacios de nombres.
- **Reproducir:** `tests/network-lab/subnet/run.sh` (subredes, control local y restauración) y `tests/packaging/run.sh` (`.deb` instalado con systemd como PID 1).
- Diseño: [ADR-007](../architecture/ADR-007-subredes-dns-privilegios.md) y [`signed-config-v2`](../protocol/signed-config-v2.md), ambos **Propuestos** (pendientes de aceptación).

## Laboratorio de subredes (`TODO OK`)

Topología: cliente `a`, segundo cliente `a2`, router de subred `r` (192.168.50.0/24 y `fd50::/64` detrás), VM interna `h` (con ruta de retorno), VM `h2` (sin ruta de retorno hacia el túnel) y `h3` (LAN local de `a2`, con el **mismo** prefijo que la subred remota).

| Comprobación | Resultado |
| --- | --- |
| Rutas IPv4 e IPv6 de la subred instaladas en el cliente por `atenea0`; estado `installed` | Correcto |
| Ruta por defecto del cliente antes y después | **Idéntica** |
| DNS dividido (simulado): `dns atenea0 10.77.0.2`, `domain atenea0 ~lab.internal`, nunca `~.` | Correcto; el agente informa `applied` |
| Forwarding: el agente informa `forwarding_ok=false`, no cambia sysctls, y pasa a `true` cuando el administrador lo activa | Correcto tras corregir el agente (se calculaba solo al aplicar la configuración) |
| Acceso real a la VM por IPv4 e IPv6 (ping y TCP 1 MB íntegro) | Correcto |
| ACL: UDP y puertos no permitidos descartados; destino fuera de la ruta sin ruta; origen falsificado descartado | Correcto |
| Sin ruta de retorno y sin SNAT, `h2` no responde; con `snat=true` responde y ve el origen enmascarado (192.168.50.1); al volver a `snat=false` la tabla nftables desaparece | Correcto |
| Subred solapada: `a2` no instala la ruta remota (`conflict`), conserva su LAN y alcanza a su vecino local | Correcto |
| Control local: un usuario sin grupo no para ni lee; el grupo del socket de estado solo lee; modos 0600 y 0660 | Correcto |
| Una configuración sin la ruta la retira; una posterior la vuelve a instalar | Correcto |
| Captura del puente subyacente: el contenido enviado a la VM no aparece en claro | 0 apariciones |
| `kill -9` del cliente: sin interfaz ni rutas residuales; `kill -9` del router: la tabla nftables queda (esperado), `atenea-agent cleanup` la elimina y es idempotente; reinicio y parada limpia | Correcto |

## Paquete y servicio (`tests/packaging`)

`.deb` construido con `dpkg-deb`; instalado en Debian 13 con systemd como PID 1: instalación limpia, grupo y permisos, `systemd-analyze verify`, servicio activo con configuración mínima, socket de estado legible solo para el grupo `atenea` y solo lectura, `kill -9` con reinicio por systemd sin residuos, `dpkg -r` (conserva configuración) y `dpkg -P` (conserva claves, estado y grupo). Exposición del servicio según `systemd-analyze security`: **4.0 (OK)**.
`systemd-resolved` real (servicio arrancado en el contenedor): `resolvectl dns/domain` por enlace y desaparición al borrar el enlace, sobre un enlace `dummy`.

## Qué demuestra

- Una subred privada se sirve a un cliente solo cuando está declarada, aprobada y firmada; el cliente la instala sin tocar su LAN ni su ruta por defecto, y la ACL sigue mandando.
- SNAT es una elección explícita por ruta con efecto medible; sin SNAT hace falta ruta de retorno.
- Las rutas, el DNS por enlace y la tabla nftables no sobreviven a una caída del agente, o se limpian con `cleanup`.
- Un usuario local sin permiso no controla el agente.

## Qué no demuestra

- **No se ha instalado en un Proxmox ni en una VM Proxmox real**: no hay acceso desde este entorno. El criterio del plan («acceso real a VM interna en Proxmox») queda **pendiente de tu validación**.
- Con `systemd-resolved` real el agente se probó en el paquete instalado; en el laboratorio de subredes y el E2E del panel el DNS usa un `resolvectl` simulado (sin systemd en esos contenedores).
- El laboratorio manual de subredes emite con `atenea issue`; el ciclo con control plane real se prueba en el E2E del panel (un router, un cliente).
- Solo IPv4/IPv6 de red privada; no hay prueba de subredes solapadas entre **dos routers** (el control plane lo rechaza con 409 y el agente lo rechaza al verificar).
- No se probó amd64 ni otras distribuciones; el `.deb` se construyó y probó en arm64.
- Sin revisión criptográfica externa del formato v2.

## Regresiones

`tests/network-lab/run.sh`, `run-e2e.sh` (control plane con Dex), `nat/run-e2e.sh`, `nat/run-e2e-relay.sh` y el E2E del panel con Chromium pasan con el agente nuevo. Fuzz `config_v2_parse`: 60 s sin fallos.

## Incidencias de la fase

- El código de `run.rs` solo se compila en Linux: clippy en macOS no lo ve. Un `continue` perdido en un `select!` y un `if` colapsable solo aparecieron en Docker. Desde ahora Linux es parte de la comprobación.
- Dos fallos reales del agente aparecieron en el laboratorio: `forwarding_ok` obsoleto; y una comprobación del arnés mal planteada para la desaparición del enlace en resolved.
- **El E2E del panel destapó un bug real que el laboratorio manual no veía:** el coordinator entrega al cliente la IP de LAN del router (`192.168.50.1`) como candidato; el agente la trataba como endpoint protegido del plano de control y marcaba su propia ruta como `conflict`. Corrección: los candidatos y endpoints de **pares** dentro de la red del túnel o de una subred servida se descartan (solo son alcanzables por el propio túnel), el router no anuncia direcciones dentro de sus `advertised`, y los endpoints del plano de control (sync, coordinator, STUN, relay) siguen protegiendo.
- El `axe` del E2E marcó un contraste que en realidad era el fundido de aparición del diálogo (160 ms); además la pantalla tenía un defecto visual real (radios mal maquetados), ya corregido. El `axe` espera ahora a que el diálogo esté estable, sin desactivar ninguna regla.

## Pendientes de la fase, cerrados el 2026-10-25

- **E2E del panel con rutas** (`apps/web/e2e/routes.spec.ts`, ya sin `skip`): agente router con LAN interna sin ruta de retorno declara `192.168.50.0/24`; el panel la muestra `pending`; el botón queda deshabilitado hasta elegir SNAT; se aprueba con SNAT; el cliente la instala y alcanza la VM (ping y TCP 1 MB íntegro, origen enmascarado `192.168.50.1`); el DNS dividido configurado en el panel llega al agente (`dns atenea0 192.168.50.1`, `domain atenea0 ~lab.internal`, sin `~.`); al revocar la ruta el cliente la retira y deja de alcanzar la VM (15 s). Los tres specs (hidratación, panel y rutas) pasan en 2 min.
- **Agente real con `systemd-resolved` real** (`tests/packaging`): el servicio instalado desde el `.deb` configura `atenea0` con `10.77.0.1`, `~lab.internal` y `Default Route: no`; `/etc/resolv.conf` no cambia (mismo SHA-256); el enlace y la configuración desaparecen al parar y tras `kill -9`; reinicio por systemd reaplica el DNS; una configuración posterior sin DNS deja `dns: none`; `forwarding_ok` pasa de `false` a `true` al activar `ip_forward`.
- **Ciclo declarar → aprobar → emitir con agentes reales** enlazados al control plane real: cubierto por el E2E anterior.
