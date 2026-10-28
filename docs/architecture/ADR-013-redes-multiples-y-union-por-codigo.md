# ADR-013: Varias redes por dispositivo y unión por código (estilo ZeroTier)

- **Estado:** **Aceptado** por el responsable el 2026-10-28 con las decisiones de §6 (código todo en uno, privilegios opción A, una instancia por red, claves por red). Implementadas: fase 1 (servidor) y fase 2 (CLI, §7).
- **Relacionados:** ADR-005 (raíz y emisores), ADR-007 (rutas y DNS), ADR-008 (clientes), ADR-010 (L2), ADR-011 (aplicación de escritorio, solo lectura), ADR-012 (nombres).

## 1. Objetivo

Desde la aplicación de escritorio, pegar un **código de red**, pulsar «Unirse a una red» y quedar conectado a esa red del servidor; poder estar en **varias redes a la vez** y conectar o desconectar cada una, como en ZeroTier. Un mismo servidor (el VPS) aloja varias redes aisladas entre sí.

## 2. Qué existe hoy (comprobado en el código)

| Pieza | Estado |
| --- | --- |
| Servidor con varias redes | **Sí.** El control plane ya modela organizaciones → redes (`networks.id`, CIDR propios, `root_anchor` propio). El coordinator y el relay separan por `network_id` (nonces, cupos, `authorized_pair`, tokens de relay). |
| Unirse con un código | **Parcial.** El panel emite un `join` `{url, token, network_id, root_anchor}` por red (`POST /networks/{net}/enrollment-tokens`), pero el token es de **un solo uso** y el cliente lo consume con `atenea enroll` + `agent-config`; no hay un «código» único que pegar. |
| Unirse solo con el identificador de red | **No.** No existe ninguna petición de unión sin token. |
| Varias redes en un dispositivo | **No.** El agente es de **una red por instancia** (ADR-010 §2.4): un `agent.toml`, una interfaz, un directorio de estado, un par de sockets, un servicio `launchd` (`one.atenea.agent`) y una limpieza (`cleanup`) que asume una sola instancia de sistema. |
| Interfaz que cambia el sistema | **No.** La aplicación es de solo lectura por decisión (ADR-011): no puede crear interfaces, rutas ni claves. |

Conclusión: el servidor ya está casi listo; **lo que falta es el cliente** (varias instancias, un gestor con privilegios y la pantalla de unión) y una **forma de unión mejor que el token de un uso**.

## 3. Propuesta

### 3.1 Código de red

Un texto único y copiable que contiene todo lo que el cliente necesita: dirección del servidor, identificador de red y **huella de la raíz de confianza** (`root_anchor`), más una suma de comprobación. Ejemplo sintético: `atenea1-AB12…-7F`. Con la huella dentro no hay confianza a ciegas en la primera conexión (TOFU): el cliente fija la raíz que le dio quien le pasó el código por otro canal. Variante opcional con token de un uso para redes cerradas. Si falta el servidor o la huella, el cliente rechaza el código; no se completa nada de memoria.

### 3.2 Política de unión de cada red (campo nuevo en el servidor)

| Política | Efecto | Riesgo |
| --- | --- | --- |
| `cerrada` (por defecto) | Solo con un token emitido por un administrador (hoy). | Ninguno nuevo. |
| `con_solicitud` | Cualquiera con el código envía una solicitud: el dispositivo queda `pending` y un administrador la aprueba comparando la **huella del dispositivo** (como ahora). | Spam de solicitudes: tope de pendientes por red, límites por IP (ya existe `enroll_per_minute`) y caducidad de pendientes. |
| `abierta` | Aprobación automática. **No se ofrece en v1.** | Cualquiera con el código entra. |

El identificador de red **no es un secreto ni una credencial**: lo que autoriza es la aprobación del administrador.

### 3.3 Varias redes en el dispositivo

Una **instancia de agente por red** (reutiliza el agente actual, sin tocar el protocolo):

- Directorio propio por red: `…/networks/<id>/` con claves, estado, configuración firmada y `agent.toml`.
- **Claves por red** (par de claves nuevo en cada unión): el servidor no puede correlacionar un mismo equipo entre redes, y comprometer una red no compromete las demás.
- Interfaz propia (`utunN` en macOS; `atenea<N>` en Linux; un adaptador por red en Windows), sockets de control y de estado propios y un servicio por red (`one.atenea.agent.<id8>`).
- **Puerto UDP por instancia** (51820, 51821…): cada red necesita su puerto alcanzable para el camino directo. Alternativa a medio plazo: un solo proceso que multiplexe varias redes sobre un socket (el prólogo Noise ya separa redes), con refactor mayor del bucle principal; **no** en la primera entrega.
- **Cambios obligatorios en lo existente** (hoy asumen una sola instancia): la limpieza de resolutores DNS de macOS marca los ficheros como «de Atenea» sin distinguir red; el directorio `/var/run/atenea` y los nombres de socket son únicos; la etiqueta de `launchd` es única. Todo ello pasa a depender del identificador de red.
- **Conflictos entre redes:** CIDR solapados y dominios DNS repetidos. El cliente debe **rechazar la unión** (con el motivo) si el CIDR de la red nueva solapa con otra red unida o con una red local, antes de tocar nada.

### 3.4 Privilegios (decisión de seguridad principal)

Unirse, salir, conectar y desconectar **cambian el sistema** (claves root, interfaz, rutas, resolutores). Hoy la aplicación no puede y por diseño es de solo lectura.

| Opción | Cómo | Pros | Contras |
| --- | --- | --- | --- |
| **A. Petición de administrador por operación** | La app lanza el CLI con elevación del sistema (diálogo de contraseña en macOS, UAC en Windows, `pkexec` en Linux) para cada unión o salida. | Sin servicio con privilegios nuevo ni API local nueva; superficie mínima. | Pide contraseña cada vez; el interruptor de conectar/desconectar sería incómodo. |
| **B. Gestor residente** | Un servicio con privilegios con un socket local (solo el grupo de administradores, comprobando la credencial Unix del cliente) y una API **mínima** (`unir`, `salir`, `listar`, `conectar`, `desconectar`) que ejecuta el CLI; **no** ejecuta órdenes arbitrarias. | Experiencia ZeroTier/Tailscale: un clic, sin contraseñas. | Componente nuevo con privilegios: modelo de amenazas propio; cualquier proceso del grupo autorizado puede unir el equipo a una red; requiere pruebas y revisión de seguridad antes de usarse. |

**Recomendación:** entregar primero **A** (y el CLI, que es la base de ambas) y pasar a **B** solo si la fricción lo justifica, con su propio ADR. Una red hostil puede hacer que el equipo acepte rutas privadas y DNS para dominios concretos (ADR-007; nunca la ruta por defecto): por eso la pantalla de unión muestra el nombre de la red, el servidor y la huella de la raíz, y pide confirmación explícita.

### 3.5 Interfaz

- **Redes:** sección nueva con la lista de redes unidas (nombre, servidor, estado, interruptor de conexión) y «Unirse a una red…».
- **Dispositivos:** un selector de red; dentro de cada red siguen «Local», «Servidor» y «Pares».
- **Bandeja:** una línea por red con su punto de estado.
- Nombre de la red: el que viaja en el código; alias local editable.
- La aplicación pasa de solo lectura a poder **pedir** operaciones: se revisa ADR-011 (la lectura de estado sigue sin privilegios y sin órdenes de control).

## 4. Plan por fases

| Fase | Contenido | Tamaño | Prueba de salida |
| --- | --- | --- | --- |
| **0** | Aceptar este ADR (decisiones de §6). | S | ADR aceptado. |
| **1** | **Servidor:** código de red (generación en el panel con huella y QR), política `cerrada`/`con_solicitud`, endpoint de solicitud sin token con límites y tope de pendientes, caducidad de pendientes, auditoría, OpenAPI. Una migración. | M | Pruebas contra PostgreSQL: cada política, límites, caducidad, aislamiento entre redes y que un código manipulado se rechaza. |
| **2** | **CLI:** `atenea network join <código>`, `leave`, `list`, `up`, `down`; análisis del código con fuzz; claves por red; rechazo de CIDR solapados. Sin interfaz gráfica. | M | Laboratorio con dos redes en un mismo cliente: no se alcanzan entre sí, salir de una no toca la otra, revocar en una no afecta a la otra. |
| **3** | **Multiinstancia en el agente y empaquetado:** directorios, interfaces, sockets, etiquetas y limpieza por red; `.pkg`, `.deb` y Windows. | M–L | Pruebas de limpieza (un `cleanup` no borra recursos de otra red), reinicio con dos redes y desinstalación. |
| **4** | **Aplicación:** sección Redes, selector, diálogo de unión con confirmación y opción **A** de privilegios; bandeja por red. | M | Pruebas de interfaz y de accesibilidad; unión y salida reales en un Mac. |
| **5** | **Piloto:** crear una segunda red en el VPS, unir el Mac a las dos, medir el efecto en el soak (más sesiones, más puertos) y documentar. | S | Soak de 72 h con dos redes. |
| **6** (opcional) | Gestor residente (opción **B**) con ADR y revisión de seguridad propios. | L | Modelo de amenazas y pruebas negativas. |
| **7** (aplazado) | Multiplexado de varias redes en un proceso y un único puerto. | L | Comparación medida frente a una instancia por red. |

Cada fase deja `check.sh` y CI en verde y documentación con los límites reales; nada se despliega sin autorización explícita.

## 5. Riesgos y límites

- **Autoridad de la raíz por red.** Cada red necesita un certificado de emisor firmado por una raíz **fuera de línea** (ADR-005; hoy 30 días de validez). Con varias redes crece el trabajo de renovación manual. Reutilizar una misma raíz para varias redes parece posible (el control plane recibe la `root_anchor` al crear la red) pero **no está comprobado**; la renovación automática con raíz fuera de línea es un problema abierto.
- **L3 frente a L2.** Las redes virtuales de ZeroTier son de nivel 2 (Ethernet, difusión). Las de Atenea son de nivel 3. El modo L2 está diseñado (ADR-010) y **aplazado hasta el informe de la auditoría**; este plan no lo incluye.
- **Carga:** más redes por equipo implican más sesiones, más puertos y más sondeos; sin medir.
- **Un código filtrado** permite enviar solicitudes (no entrar): aprobar compara siempre la huella del dispositivo.
- **Privilegios:** la opción B añade superficie local; la opción A no la añade pero es menos cómoda.
- **Sin comprobar hoy:** coordinator y relay con dos redes reales en el mismo servidor (el código las separa, no se ha ejecutado), y el solapamiento de dos interfaces de Atenea en macOS.

## 6. Decisiones del responsable (2026-10-28)

1. **Formato del código:** **todo en uno**, con la huella de la raíz de confianza dentro.
2. **Política de unión:** `cerrada` por defecto y `con_solicitud` disponible en v1. *Esta decisión no la dio el responsable de forma explícita: se deduce de que la fase 1 incluye `con_solicitud`; se mantiene la recomendación de `cerrada` por defecto y `abierta` no se ofrece.*
3. **Privilegios:** **opción A** (elevación por operación). La opción B queda para un ADR posterior.
4. **Una instancia de agente por red**; el multiplexado queda aplazado.
5. **Claves por red.**
6. **L2:** fuera de alcance; espera a la auditoría y a la fase 10 (ADR-010).

**Desviación de la fase 1 respecto a §4:** el QR del panel se aplaza porque exige una dependencia nueva de generación de QR (inventario y revisión de suministro); el panel muestra el código, la huella y un botón de copiar.

## 7. Estado de la fase 2 (CLI)

**Implementada** (CLI Linux/macOS; Windows devuelve «no soportado»). Hecho: `atenea network join|list|up|down|leave` con `--root`, confirmación explícita antes de actuar, claves nuevas y `agent.toml` con la raíz fijada desde el código, una instancia de agente por red (interfaz `atenea<N>`, puerto UDP ≥ 51820 y sockets propios), guarda de solapamiento en el agente (`network_overlap`), objetivo de fuzz `join_code_parse` y el laboratorio `tests/network-lab/two-networks.sh`, que demostró en Docker claves distintas, ambas redes a la vez, aislamiento, `leave` de A sin cortar B, revocación de B sin afectar a A, código manipulado y red solapada. **Regla de la guarda:** como el CIDR no viaja en el código, se evalúa al llegar la primera configuración verificada y solo hay conflicto si otra interfaz tiene una ruta igual o más específica que un prefijo de `network_prefixes`; una ruta más ancha (VPN, Docker, LAN) no lo es, y las rutas de host de las propias `tunnel_addrs` se ignoran siempre (en macOS `netstat` las lista en `lo0`). Probada con fixtures de salida real de `netstat` e `ip -j route`; **sin probar en un Mac con dos redes reales**. **No hecho** (fases 3 y 4): servicios `systemd`/`launchd` por red, limpieza multiinstancia de DNS en macOS, empaquetado, Windows y la aplicación; `leave` no revoca en el servidor. Detalle: la guía enlazada abajo.

Detalle operativo: [redes-multiples-cli.md](../operations/redes-multiples-cli.md). Instalación base: [Linux](../operations/instalacion-linux.md) y [macOS](../operations/instalacion-macos.md). Protocolo: [join-code-v1](../protocol/join-code-v1.md).
