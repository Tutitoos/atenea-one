<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-025: Versiones instaladas de cada dispositivo (agente, CLI y aplicación)

- **Estado:** **Propuesto.** El responsable fijó el **alcance** (agente, CLI y aplicación de cada dispositivo) y la **visibilidad** (todos los miembros de la red) el 2026-11-16; el resto de decisiones (P1 a P3) son recomendaciones pendientes de su aceptación. **Sin código todavía.** Nada probado fuera de la lectura del código de `main`.
- **Relacionados:** [ADR-024](ADR-024-servicios-expuestos-por-dispositivo.md) (el canal de informe y de pares que se reutiliza, y su regla «servidor antes que agentes»), [ADR-012](ADR-012-nombres-de-dispositivo.md) (metadatos de presentación), [ADR-016](ADR-016-politicas-de-acceso-minimas.md) (quién ve a quién), [ADR-019](ADR-019-aplicacion-de-escritorio-wails.md) y [ADR-021](ADR-021-asistente-de-primera-ejecucion.md) (la aplicación no tiene privilegios y su superficie es una lista cerrada), [ADR-022](ADR-022-instalar-el-cliente-con-la-aplicacion.md) y [ADR-023](ADR-023-instalador-unico-de-windows.md) (los instaladores únicos).

## Contexto

La pantalla «Detalles» de un dispositivo en la aplicación de escritorio muestra el camino, la latencia, el tráfico, la dirección remota y la clave pública, pero **no qué versión de Atenea tiene instalada ese equipo**. Para saber si un par está actualizado hay que entrar en él. Hechos comprobados en `main` (2026-11-16):

- **Ningún dato de versión viaja entre dispositivos.** El agente informa cada 15 s al control plane (`sync::Report`: caminos, NAT, DNS, rutas, nombre del sistema y, si se activa, servicios) y los demás agentes leen `GET /api/v1/device/peers` (`{device_id, name, role, services}`). Ninguno de los dos lleva versiones. El control plane solo publica **la suya** (`GET /api/v1/server/info`, `control_plane.version` y `commit`); coordinator y relay no tienen canal.
- **El informe es estricto.** `Report` del control plane es `deny_unknown_fields`: un campo que el servidor no conozca hace que rechace **todo** el informe con `400 invalid_report`. El agente no degrada ni reintenta sin el campo. Es la causa de la regla de ADR-024: el servidor se actualiza antes que los agentes.
- **Cada binario sabe su versión.** `atenea-agent` y `atenea` imprimen `<nombre> <versión> (<commit>)` con `--version` (`apps/agent/src/build_info.rs`). **La aplicación de escritorio no:** su binario de Go no lleva versión ni commit (`ldflags` solo `-s -w`), y la versión vive en los metadatos de cada paquete.
- **Los tres componentes se instalan juntos y en la misma versión** en el caso normal: el `.deb` de la aplicación depende de `atenea (= misma versión)` y los instaladores únicos de macOS y de Windows llevan los tres (ADR-022, ADR-023). **Pueden diferir** en una instalación parcial: el DMG de macOS de la aplicación sola, o una actualización a medias.
- **La aplicación no puede enviar datos al agente.** El socket de estado solo admite `status` y el de control no acepta cargas ni cliente sin privilegios (ADR-024, decisión 8, descartó hacer escribible el de estado). La aplicación solo lo lee.
- **`/device/peers` solo devuelve los pares con política** hacia o desde el solicitante (ADR-016, la misma `compile` que la configuración firmada). El inventario de la organización (administradores y token de servicio de solo lectura) ve todos los dispositivos de la red.

## Decisión

**Cada agente detecta la versión instalada de sus tres componentes, la informa en el canal de metadatos de presentación y los demás dispositivos que ya ven a ese par la muestran en «Detalles».** Es un dato de presentación: no autoriza, no enruta ni cambia ninguna política.

### Decididas por el responsable

| # | Decisión | Elegida | Alternativas |
|---|---|---|---|
| D1 | **Alcance** | **Agente, CLI y aplicación** de cada dispositivo | Solo este equipo (sin protocolo); solo agente y CLI (un paquete, una versión) |
| D2 | **Quién ve las versiones de un dispositivo** | **Todos los miembros de la red**, sin opt-in ni restricción a administradores | Solo administradores; solo pares con política |

**Cómo se interpreta D2 en este ADR (hay que confirmarlo).** «Todos los miembros» se aplica a quien **ya ve ese dispositivo**: `/device/peers` sigue devolviendo solo los pares con política (ADR-016) y el inventario sigue siendo el de la organización. **No** se añade un endpoint que liste versiones de toda la red a un dispositivo que no tiene política con el otro: eso ampliaría lo que un miembro puede saber de equipos a los que ni siquiera puede llegar. Si el responsable quiere esa lista de red completa, es una decisión aparte (un endpoint nuevo y otra revisión de amenazas).

### Propuestas (pendientes de aceptación)

| # | Propuesta | Recomendación | Alternativa descartada |
|---|---|---|---|
| P1 | **Cómo se detecta** | El agente ejecuta `--version` de **rutas fijas y de confianza** (decisión 2) | (a) un canal de la aplicación al agente: exige que un proceso sin privilegios escriba en un agente root y rompe la garantía de ADR-024; (b) leer `BUILD-INFO.json` de cada instalador: da la versión del **paquete**, no la de cada componente |
| P2 | **¿Siempre activo?** | **Sí, sin interruptor** (consecuencia de D2: no es un dato que el usuario deba decidir publicar) | Opt-in como los servicios (ADR-024 D1): dejaría la mayoría de los pares sin dato |
| P3 | **Frescura** | La versión **no caduca a los 120 s** como los servicios: se devuelve la última informada con `versions_at` | Omitirla pasados 120 s: un equipo apagado dejaría de decir qué tenía instalado |

1. **Qué se informa.** Por componente instalado: `component` (`agent`, `cli` o `desktop`), `version` (`MAYOR.MENOR.PARCHE` con sufijo opcional de prerelease) y `commit` (de 7 a 40 hexadecimales, o `unknown`). Un componente que no está instalado **no aparece** en la lista. **Nunca** se informan rutas, usuario, sistema operativo, arquitectura ni nombre del paquete.
2. **Cómo se detecta (P1).** `atenea-agent` es el componente en ejecución: su versión sale de su compilación. Para `cli` y `desktop` ejecuta el binario instalado **en una ruta absoluta fija por sistema**, sin shell ni argumentos distintos de `--version`:

   | Sistema | CLI | Aplicación |
   |---|---|---|
   | Linux | `/usr/bin/atenea` | `/usr/bin/atenea-desktop` |
   | macOS | `/Library/Atenea/bin/atenea` | `/Applications/Atenea One.app/Contents/MacOS/atenea-desktop` |
   | Windows | `%ProgramFiles%\Atenea\atenea.exe` (carpeta conocida del sistema) | `%ProgramFiles%\Atenea\atenea-desktop.exe` |

   Antes de ejecutar se exige que el fichero sea regular, no un enlace, de root o de Administradores y sin escritura para el grupo ni otros (la comprobación de confianza que ya usan las rutas fijas del agente y ADR-017 §7). Entorno vacío, plazo de 2 s, salida de 256 bytes como máximo y lectura estricta de `^<nombre> <versión> \(<commit>\)$`. **Cualquier fallo omite ese componente**; no es un error del agente ni cambia su estado. Se detecta al arrancar y cada 10 minutos (un paquete nuevo reinicia el agente, y instalar o quitar la aplicación se ve a los 10 minutos como máximo).
3. **La aplicación aprende su versión.** El binario de Go gana `--version` (imprime `atenea-desktop <versión> (<commit>)` y sale antes de crear ventana, bandeja o servicio) y la versión y el commit se incrustan al compilar (`-X` en `packaging/desktop/build-binary.sh`). **Riesgo a comprobar en la fase 2:** en Windows el binario es del subsistema gráfico (`-H=windowsgui`); hay que verificar en el runner que `--version` escribe en una tubería heredada. Si no fuera posible, Windows no informa la aplicación hasta decidir otra vía, y esta limitación se documenta (no se cambia el subsistema del binario por esto).
4. **Cómo viaja.** `versions` se añade a `sync::Report` como campo **opcional** (`skip_serializing_if`) y a `Report` del control plane. Validación estricta: como máximo **3** entradas, una por componente, sin repetidos; `version` de hasta 32 caracteres de `[0-9A-Za-z.+-]`; `commit` de 7 a 40 hexadecimales o `unknown`; la lista vacía es válida («no hay nada instalado que informar»). Se guarda como `devices.versions jsonb NOT NULL DEFAULT '[]'` y `devices.versions_at timestamptz` (migración `0012`), **sustituida** en cada informe, sin historial. Mismos límites de tamaño, nonce y tasa que `/device/status`.
5. **Quién la ve (D2).** `GET /api/v1/device/peers` devuelve `versions` y `versions_at` por par, con la **misma visibilidad por política** que el resto del par. El inventario de la organización (`Device`) los lleva también. Pasados 120 s **no se omite** (P3): la interfaz muestra el valor y, si `versions_at` tiene más de un día, cuánto hace que se informó.
6. **Qué ve el usuario.** En «Detalles» de un par, tres filas: «Versión del agente», «Versión de la CLI» y «Versión de la aplicación», con `versión (commit corto)`. Tres estados que no se confunden: **«No instalada»** (el par informó y el componente no está en la lista), **«Sin informar»** (`versions_at` vacío: agente anterior a este cambio o aún sin informe) y el valor. En «Detalles» del propio equipo se leen del estado local del agente (`status.versions`), sin pasar por el servidor. El panel web las muestra en el inventario (fase posterior).
7. **Lo que no cambia.** El contrato firmado (`SignedConfig`), el protocolo del túnel, las políticas, la aprobación por huella, la superficie de 14 métodos de la aplicación (**no** se añade ninguno), el filtrado del runtime de Wails y la lista de operaciones elevadas: no hay operación nueva ni canal nuevo de la aplicación al agente.

## Amenazas y límites

| Riesgo | Tratamiento |
| --- | --- |
| La versión exacta revela el nivel de parcheo de cada equipo (mapa de vulnerabilidades conocidas) | **Aceptado por el responsable (D2).** Se limita a quien ya ve al par por política, sin historial, y se dice en la documentación y en las notas de la versión. Quien no quiera publicarla no puede desactivarlo en esta versión (P2) |
| Un dispositivo comprometido informa una versión falsa | Es un dato de presentación sin efecto en autorización ni enrutado; no sirve para decidir nada de seguridad. La interfaz lo pinta como texto con juego de caracteres cerrado |
| El agente, que corre como root, ejecuta binarios | Solo rutas absolutas fijas, con comprobación de confianza, sin shell, sin argumentos libres, entorno vacío, plazo y tope de salida; es el mismo patrón que `ip`, `ifconfig` y PowerShell, y nada de lo que imprima se interpreta más allá de la expresión estricta |
| Un agente nuevo contra un control plane antiguo | `deny_unknown_fields` haría fallar **todo** el informe (caminos, DNS, rutas, nombre). Como P2 lo envía siempre, **el servidor se actualiza antes que cualquier agente** y la nota de la versión lo dice en la primera línea del procedimiento. No se añade lógica de degradación |
| Se cree que «instalada» es «en ejecución» | La interfaz y la documentación lo dicen: para `cli` y `desktop` es lo instalado en disco; el agente es el que corre. Tras actualizar el paquete el agente se reinicia, así que la diferencia dura segundos |
| Abuso o desbordamiento | Máximo 3 entradas, longitudes acotadas y los mismos límites de tasa y tamaño del informe |
| Cuota compartida | No se añade ninguna petición: viaja en el informe de 15 s y en la respuesta de pares ya existente |

## Plan por fases

Cada fase es independiente y se puede publicar sola; **la 3 se despliega antes que la 4**.

1. **ADR** (este documento) y su aceptación.
2. **Aplicación: `--version`.** Versión y commit incrustados, la opción y su prueba en los tres sistemas, incluida la comprobación de Windows (decisión 3).
3. **Control plane.** Migración `0012`, `versions` en `Report`, en `/device/peers` y en el inventario, OpenAPI y su prueba de contrato, pruebas de migración (versiones `1..12`), de visibilidad por política y de que un agente antiguo sigue siendo válido. **Se despliega antes de actualizar ningún agente.**
4. **Agente: detección y publicación.** `installed_versions()` por sistema con pruebas de muestra y de rechazo de rutas no fiables; `status.versions`; envío en el informe; lectura en `fetch_peer_metadata` (incluido el duplicado del objetivo de fuzzing `agent_engine`).
5. **Aplicación de escritorio.** Las tres filas en «Detalles» del par y del propio equipo, con los tres estados.
6. **Panel web.** Versiones en el inventario.

## Pruebas de aceptación

- Con el agente nuevo, un `--version` válido en una ruta fija de confianza produce el componente; una ruta sustituida por un enlace, un fichero escribible por otros, una salida mal formada, una salida de más de 256 bytes y un binario que no termina en 2 s **omiten** el componente sin error del agente.
- La lista vacía y las de 1 a 3 componentes se aceptan; 4 entradas, un componente repetido o desconocido, una versión con caracteres fuera de la lista, o un commit de otra longitud responden `400 invalid_report` sin efectos en la base de datos.
- Un informe **sin** `versions` (agente antiguo) sigue siendo válido y **no** borra lo guardado; un par sin política con el solicitante **no** ve sus versiones; con política, sí, con su `versions_at`.
- Un control plane de la versión anterior rechaza el informe con `versions`: la prueba documenta el orden de despliegue (servidor primero).
- Prueba de contrato OpenAPI y de migración al día; la superficie de la aplicación sigue siendo de 14 métodos.
- En «Detalles»: «No instalada», «Sin informar» y el valor se muestran por separado.

## Qué no se hace en esta versión

Interruptor para no informar; versiones de coordinator y relay por dispositivo; historial de versiones o alertas de «equipo desactualizado»; lista de versiones de toda la red para dispositivos sin política entre sí; comprobar la autenticidad de lo que informa un dispositivo; una vía de la aplicación al agente; información de arquitectura o sistema operativo; actualizar equipos desde la interfaz.
