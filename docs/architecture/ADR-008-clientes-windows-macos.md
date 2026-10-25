# ADR-008: Clientes macOS y Windows (fase 8)

- **Estado:** **Aceptado** por el responsable el 2026-10-25, incluida la excepción Wintun (solo el controlador/DLL, ninguna parte del protocolo WireGuard). Clientes macOS/Windows implementados; la aceptación privilegiada pasa en runners reales de GitHub (`macos-latest`, `windows-latest`; commit `b31de56`): adaptador, v1/v2, rutas, DNS dividido, `ping` IPv4/IPv6 con `atenea-testpeer`, aislamiento de IPC, terminación forzada y limpieza. **Pendiente** (no impide aceptar el diseño): paquetes firmados (Apple Developer ID, certificado de firma de Windows) y matriz de interoperabilidad en máquinas reales.
- **Relacionados:** ADR-003 (sesión), ADR-007 (subredes, DNS, privilegios).

## Contexto

El protocolo, la identidad, el motor de caminos, STUN, relays y la verificación de configuración son portables (`crates/crypto`, `crates/identity`, `crates/nat-traversal`, `apps/agent/src/{engine,packet,coord,relay}.rs`). Lo que ata el agente a Linux es la integración con el sistema: TUN, rutas, DNS, NAT, IPC local, almacenamiento privado y ciclo de vida. Un binario que compila en macOS o Windows no es un cliente instalado: un cliente instalado crea su adaptador, aplica rutas y DNS, se ejecuta como servicio, se limpia al salir y se controla con permisos correctos.

## Decisiones

### 1. Cinco fronteras de plataforma, política compartida

Se introducen cinco interfaces pequeñas (módulo `platform` del agente; la implementación Linux actual es el comportamiento de referencia y no cambia): **túnel L3** (`TunDevice`), **red del host** (rutas, direcciones, DNS, forwarding y direcciones locales), **IPC local** (escucha y cliente con identidad del par), **ficheros privados** (lectura de secretos y reemplazo duradero) y **ciclo de vida** (parada solicitada). La **política** (conflictos de rutas, estados, diferencias, protección del plano de control, formato del IPC, verificación de configuración) vive en código común: solo cambia cómo se ejecuta cada operación en el host.

El refactor de preparación está en `apps/agent/src/platform`: despacho estático de `HostNetwork`, `TunDevice`, `LocalIpc`, `PrivateFiles` y `Lifecycle`; `netconf` mantiene la política y admite un backend en memoria. Linux conserva las órdenes y permisos existentes, incluidos los dos protocolos de reemplazo duradero de estado/configuración. macOS implementa utun, rutas y DNS dividido; Windows implementa Wintun, NetTCPIP, NRPT, canalizaciones con nombre y ciclo SCM. Ambos tienen paquete sin firmar y aceptación privilegiada definida en CI (resultado pendiente del push). No se añadieron dependencias para macOS.

### 2. Alcance de macOS y Windows en v1: solo clientes

macOS y Windows **consumen** rutas de subred y DNS dividido; **no sirven subredes**. `advertise_routes` se rechaza al arrancar con un error claro y `advertised` en una configuración firmada para esos sistemas se informa como no soportado (no se instala NAT ni se cambia el forwarding). Servir subredes sigue siendo Linux (ADR-007). Esto evita portar SNAT (pf, WinNAT/RRAS) sin un caso de uso.

### 3. macOS: CLI y demonio de launchd con `utun`

Adaptador `utun` creado con el socket de control del kernel (`com.apple.net.utun_control`), cabecera de familia de 4 bytes normalizada a IP puro. Rutas con `route`/`ifconfig`; **DNS dividido con `/etc/resolver/<dominio>`** (resolutor por dominio nativo de macOS, sin tocar el DNS global); limpieza de esos ficheros al salir, al arrancar y en `cleanup`. Ejecución como demonio root de launchd (plist del paquete); IPC por socket Unix (mismo diseño que Linux, con `LOCAL_PEEREPID`/`getpeereid`). No se usa NetworkExtension en v1.

### 4. Windows: Wintun, servicio de Windows y canalizaciones con nombre

Adaptador **Wintun** (DLL oficial cargada en tiempo de ejecución junto al ejecutable; no se vincula ni se modifica). Es la **única excepción de controlador aceptada por el responsable el 2026-10-25**; no implica aceptar el protocolo WireGuard ni otros componentes. Direcciones, MTU, rutas y direcciones locales usan los cmdlets nativos **NetTCPIP** en vez de duplicar IP Helper mediante FFI: mantienen el alcance por índice de interfaz, ofrecen JSON independiente del idioma y permiten probar órdenes con un ejecutable inyectado. PowerShell se invoca por ruta absoluta, sin perfiles ni interacción. DNS dividido con **Add-DnsClientNrptRule** por dominio, sin DNS global ni espacio de nombres raíz; se elige esta API soportada frente a escribir claves de registro sin contrato. Cada regla lleva una marca exacta de propiedad por interfaz, usada para limpiar únicamente las propias al arrancar, al parar y en `cleanup`.

IPC por **canalización con nombre** local con DACL distinta para control (SYSTEM y Administradores) y estado (solo consultas; `status_gid` 545 permite BUILTIN\Users, 544 limita a Administradores, omitido mantiene SYSTEM/Administradores). No se admiten grupos locales personalizados ni se emulan UID/GID Unix: la autorización de control usa impersonación y el token real del cliente, con reversión antes de volver a Tokio. La DACL de estado no concede creación de instancias del pipe. Claves y estado tienen propietario Administrators y DACL protegida SYSTEM/Administrators desde la creación; su manejo exige elevación y la lectura valida el mismo handle que consume (no se emula `0600`). La FFI de seguridad/SCM está confinada a `atenea-windows`, no se permite `unsafe` en el agente.

La política de estado es explícita: por defecto permanece privado; configurar `status_gid = 545` **debe** permitir `status` a un usuario estándar miembro de BUILTIN\Users. No se exige la impersonación privilegiada del canal de control a ese lector. «Solo lectura» describe las órdenes permitidas, no un handle sin escritura: la apertura y la ACE Users usan `FILE_GENERIC_READ | FILE_WRITE_DATA` (`0x12008b`), que permite leer respuesta y metadatos del pipe, enviar la consulta y sincronizar la E/S. No obtiene `FILE_CREATE_PIPE_INSTANCE`, `WRITE_DAC`, `WRITE_OWNER`, escritura de atributos ni órdenes mutantes. La CLI abre con `CreateFileW` directo, sin `GENERIC_WRITE`, que incluiría creación de instancias. La aceptación imprime owner/DACL/ACE del objeto real y prueba el acceso de un usuario estándar; las pruebas nativas del crate también usan un token con Administrators deshabilitado.

El canal de estado abierto a Users fija además una etiqueta obligatoria **Medium / No-Write-Up**: un lector estándar de integridad media puede enviar la consulta aunque el servidor sea LocalSystem, sin admitir escritura desde Low/Untrusted. Esta etiqueta no se aplica al control ni al estado privado. La regresión nativa usa un token de integridad media con Administrators deshabilitado, no un administrador elevado que simplemente simula otra identidad.

La prohibición de nuevas instancias se prueba en el extremo **servidor**: [CreateNamedPipe exige `FILE_CREATE_PIPE_INSTANCE`](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights). La regresión obtiene el descriptor real completo y usa `AccessCheck` con el token impersonado para negar `0x4` y los derechos de modificación; además intenta crear otra instancia con los mismos parámetros que funcionan para un administrador. No se deduce ese permiso de una apertura `CreateFile` del extremo cliente con el bit homónimo `FILE_APPEND_DATA`, ni se suman ACE ignorando el token.

El ZIP **sin firmar** de Atenea no contiene Wintun: el instalador descarga [Wintun 0.14.1 oficial](https://www.wintun.net/builds/wintun-0.14.1.zip), valida SHA256 `07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51` publicado en [wintun.net](https://www.wintun.net/) y contrastado por descarga independiente el 2026-10-25, comprueba la firma de la DLL y conserva su licencia. No se versiona la DLL. `AteneaAgent` se registra como LocalSystem con arranque **manual**, sin arrancar durante la instalación; usa `%ProgramData%\Atenea` para datos y `%ProgramFiles%\Atenea` para binarios. La parada SCM limpia normalmente; `cleanup` recupera residuos propios tras terminación forzada. Véase [instalación Windows](../operations/instalacion-windows.md).

### 5. Verificación: runners reales de CI y un par de usuario

- Trabajos de CI en `macos` y `windows` (máquinas reales con privilegios) que instalan, crean el adaptador, aplican rutas y DNS desde una configuración firmada, comprueban IPC y permisos, y retiran todo.
- Un **par de usuario** de pruebas (el `Engine` del repositorio en proceso, sin TUN) responde a ICMP dentro del túnel: permite un `ping` real contra el adaptador del agente en cada sistema.
- **Pendiente de máquinas reales del responsable:** la matriz Linux↔Windows↔macOS con tráfico IPv4/IPv6 por P2P y relays, suspensión/reanudación, Wi-Fi, y la instalación firmada. Los runners de CI no se conectan entre sí.

### Decisiones del responsable y pendientes

1. **Aceptado el 2026-10-25:** Wintun, propiedad de WireGuard LLC, únicamente como controlador TUN mediante su API. Se revisó la licencia de binarios precompilados del ZIP oficial: no modificar ni retirar avisos; redistribución limitada junto al software que usa la API. Se conserva la licencia en instalación y no se aloja la DLL en el repositorio.
2. **Firma y notarización pendientes:** un Apple Developer ID (macOS, notarización) y un certificado de firma de código (Windows). Sin ellos solo se entregan paquetes **sin firmar**, aptos para pruebas. La firma del controlador oficial Wintun no firma Atenea.
3. **Máquinas reales pendientes** (Windows y macOS) para la matriz de interoperabilidad y la instalación firmada. Definir un trabajo en CI no equivale a observar su resultado.

## Consecuencias

- Una plataforma nueva es un backend de cinco interfaces, no una bifurcación del agente.
- Los clientes no pueden ser routers de subred: quien lo necesite usa un Linux.
- Hasta que el responsable aporte máquinas reales y certificados, la fase 8 no puede declararse completa; el informe separará lo verificado en runners de lo pendiente.
