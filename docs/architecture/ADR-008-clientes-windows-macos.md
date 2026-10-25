# ADR-008: Clientes macOS y Windows (fase 8)

- **Estado:** Propuesto (2026-10-25). Las decisiones marcadas «Decisión del responsable» bloquean partes concretas y no se asumen.
- **Relacionados:** ADR-003 (sesión), ADR-007 (subredes, DNS, privilegios).

## Contexto

El protocolo, la identidad, el motor de caminos, STUN, relays y la verificación de configuración son portables (`crates/crypto`, `crates/identity`, `crates/nat-traversal`, `apps/agent/src/{engine,packet,coord,relay}.rs`). Lo que ata el agente a Linux es la integración con el sistema: TUN, rutas, DNS, NAT, IPC local, almacenamiento privado y ciclo de vida. Un binario que compila en macOS o Windows no es un cliente instalado: un cliente instalado crea su adaptador, aplica rutas y DNS, se ejecuta como servicio, se limpia al salir y se controla con permisos correctos.

## Decisiones

### 1. Cinco fronteras de plataforma, política compartida

Se introducen cinco interfaces pequeñas (módulo `platform` del agente; la implementación Linux actual es el comportamiento de referencia y no cambia): **túnel L3** (`TunDevice`), **red del host** (rutas, direcciones, DNS, forwarding y direcciones locales), **IPC local** (escucha y cliente con identidad del par), **ficheros privados** (lectura de secretos y reemplazo duradero) y **ciclo de vida** (parada solicitada). La **política** (conflictos de rutas, estados, diferencias, protección del plano de control, formato del IPC, verificación de configuración) vive en código común: solo cambia cómo se ejecuta cada operación en el host.

El refactor de preparación está en `apps/agent/src/platform`: despacho estático de `HostNetwork`, `TunDevice`, `LocalIpc`, `PrivateFiles` y `Lifecycle`; `netconf` mantiene la política y admite un backend en memoria. Linux conserva las órdenes y permisos existentes, incluidos los dos protocolos de reemplazo duradero de estado/configuración. macOS implementa utun, rutas y DNS dividido, con paquete sin firmar y aceptación privilegiada definida en CI (resultado pendiente del push); Windows sigue rechazando `run` explícitamente. No se añadieron dependencias para macOS.

### 2. Alcance de macOS y Windows en v1: solo clientes

macOS y Windows **consumen** rutas de subred y DNS dividido; **no sirven subredes**. `advertise_routes` se rechaza al arrancar con un error claro y `advertised` en una configuración firmada para esos sistemas se informa como no soportado (no se instala NAT ni se cambia el forwarding). Servir subredes sigue siendo Linux (ADR-007). Esto evita portar SNAT (pf, WinNAT/RRAS) sin un caso de uso.

### 3. macOS: CLI y demonio de launchd con `utun`

Adaptador `utun` creado con el socket de control del kernel (`com.apple.net.utun_control`), cabecera de familia de 4 bytes normalizada a IP puro. Rutas con `route`/`ifconfig`; **DNS dividido con `/etc/resolver/<dominio>`** (resolutor por dominio nativo de macOS, sin tocar el DNS global); limpieza de esos ficheros al salir, al arrancar y en `cleanup`. Ejecución como demonio root de launchd (plist del paquete); IPC por socket Unix (mismo diseño que Linux, con `LOCAL_PEEREPID`/`getpeereid`). No se usa NetworkExtension en v1.

### 4. Windows: Wintun, servicio de Windows y canalizaciones con nombre

Adaptador **Wintun** (DLL cargada en tiempo de ejecución; no se vincula ni se modifica). Direcciones, MTU y rutas por la API IP Helper (`windows-sys`, en un crate propio que permite `unsafe`, como `crates/tunnel`); DNS dividido con reglas **NRPT** por dominio (sin DNS global ni ruta DNS por defecto); IPC por **canalización con nombre** con DACL distinta para control (SYSTEM y Administradores) y estado (solo lectura, grupo local); ciclo de vida con el Administrador de control de servicios. Claves y estado con DACL restrictiva desde la creación (no se emula `0600`).

### 5. Verificación: runners reales de CI y un par de usuario

- Trabajos de CI en `macos` y `windows` (máquinas reales con privilegios) que instalan, crean el adaptador, aplican rutas y DNS desde una configuración firmada, comprueban IPC y permisos, y retiran todo.
- Un **par de usuario** de pruebas (el `Engine` del repositorio en proceso, sin TUN) responde a ICMP dentro del túnel: permite un `ping` real contra el adaptador del agente en cada sistema.
- **Pendiente de máquinas reales del responsable:** la matriz Linux↔Windows↔macOS con tráfico IPv4/IPv6 por P2P y relays, suspensión/reanudación, Wi-Fi, y la instalación firmada. Los runners de CI no se conectan entre sí.

### Decisiones del responsable (no se asumen)

1. **Wintun** pertenece a WireGuard LLC (es un controlador TUN, no el protocolo WireGuard; licencia de binarios precompilados con condiciones de redistribución). ¿Se acepta como excepción a «sin componentes de WireGuard»? Alternativa: TAP-Windows6 (L2, GPL con excepciones), más pesado y sin ventajas.
2. **Firma y notarización:** un Apple Developer ID (macOS, notarización) y un certificado de firma de código (Windows). Sin ellos solo se entregan paquetes **sin firmar**, aptos para pruebas, y se documenta.
3. **Máquinas reales** (Windows y macOS) para la matriz de interoperabilidad y la instalación firmada.

## Consecuencias

- Una plataforma nueva es un backend de cinco interfaces, no una bifurcación del agente.
- Los clientes no pueden ser routers de subred: quien lo necesite usa un Linux.
- Hasta que el responsable aporte máquinas reales y certificados, la fase 8 no puede declararse completa; el informe separará lo verificado en runners de lo pendiente.
