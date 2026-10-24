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
- El DNS del **agente** se probó con un `resolvectl` simulado; `systemd-resolved` real se probó con un enlace `dummy` y `resolvectl` directo, **no** con el agente configurando el enlace del túnel. La integración completa agente + resolved real queda pendiente.
- La aprobación de rutas con SNAT desde el **panel** no tiene prueba E2E con navegador: `apps/web/e2e/routes.spec.ts` está **omitido** (`test.skip`) porque `tests/e2e-panel/lab.sh` aún no prepara LAN, forwarding ni la declaración de rutas por un agente. El panel y el control plane están cubiertos por pruebas de componentes y de integración con PostgreSQL.
- El laboratorio emite las configuraciones con `atenea issue` (emisor manual); el ciclo declarar → aprobar → emitir se probó en el control plane (PostgreSQL real), no con agentes reales enlazados al control plane.
- Solo IPv4/IPv6 de red privada; no hay prueba de subredes solapadas entre **dos routers** (el control plane lo rechaza con 409 y el agente lo rechaza al verificar).
- No se probó amd64 ni otras distribuciones; el `.deb` se construyó y probó en arm64.
- Sin revisión criptográfica externa del formato v2.

## Regresiones

`tests/network-lab/run.sh`, `run-e2e.sh` (control plane con Dex), `nat/run-e2e.sh`, `nat/run-e2e-relay.sh` y el E2E del panel con Chromium pasan con el agente nuevo. Fuzz `config_v2_parse`: 60 s sin fallos.

## Incidencias de la fase

- El código de `run.rs` solo se compila en Linux: clippy en macOS no lo ve. Un `continue` perdido en un `select!` y un `if` colapsable solo aparecieron en Docker. Desde ahora Linux es parte de la comprobación.
- Dos fallos reales del agente aparecieron en el laboratorio: `forwarding_ok` obsoleto; y una comprobación del arnés mal planteada para la desaparición del enlace en resolved.
