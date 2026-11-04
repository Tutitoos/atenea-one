<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-022: Que la aplicación de escritorio traiga el cliente (instalador integrado)

- **Estado:** **Aceptado** por el responsable el 2026-11-04 (opción **O2**) e **implementado en `main`** (PR #35) para macOS (`.pkg` único) y Linux (`Depends: atenea (= versión)` en el `.deb` de escritorio); **publicado en la v0.1.15**, con el `.pkg` único instalado en un Mac real (redes conectadas) y los dos `.deb` instalados con `apt` en un Debian 13 y un Ubuntu 24.04 (la aplicación pasa la prueba de humo en este último); la aplicación del `.pkg` abre su ventana de primera ejecución con un perfil vacío. **Sin comprobar:** el aspecto real de esa ventana. Complementa a [ADR-021](ADR-021-asistente-de-primera-ejecucion.md) (su decisión 2 se mantiene) y cumple la decisión 7 de [ADR-019](ADR-019-aplicacion-de-escritorio-wails.md), que dejó el instalador integrado para «un ADR propio».
- **Relacionados:** ADR-019 (decisiones 7 y 9: sin *updater*), ADR-021 (asistente y privilegios), ADR-011 §2 (autoridad = sistema operativo, ruta fija del CLI), ADR-009 (releases firmados), ADR-017 (Windows).

## Contexto

El asistente de primera ejecución (0.1.14) detecta que falta el cliente y enseña una orden para copiar (`sudo installer -pkg atenea-cli-*-macos-*.pkg -target /`). El responsable lo ha visto funcionando y pide que sea mejor: que la aplicación **«lo tenga dentro»** o que **acceda a la URL `latest` de GitHub y lo instale automáticamente**.

Hechos del código y de la publicación (v0.1.14):

- El cliente son el CLI `atenea` y el agente `atenea-agent`. En macOS los trae el `.pkg` (4,7 MB, `/Library/Atenea/bin`, root:wheel, más el `postinstall` que registra y reinicia los servicios); en Linux, el `.deb` `atenea`; en Windows, sus ejecutables. La aplicación (DMG de 4,0 MB) **no incluye nada de eso**.
- Toda operación con privilegios de la aplicación ejecuta **un binario fijo propiedad de root** (`/Library/Atenea/bin/atenea`, `/usr/bin/atenea`) con argumentos validados y el diálogo del sistema en cada operación (ADR-011 §2). **Ninguna operación ejecuta con privilegios algo que un usuario sin privilegios pueda reemplazar.**
- Un DMG se arrastra a `/Applications` y la copia queda **propiedad del usuario**: su contenido, incluido cualquier fichero dentro de `Atenea One.app`, lo puede modificar un proceso con los permisos de ese usuario. Un `.pkg` instalado como root deja el `.app` en propiedad de root (a comprobar al construirlo).
- El webview **no tiene salida de red** (CSP). La aplicación Go hoy **no hace ninguna petición HTTP** (abrir el panel lo hace el navegador del sistema).
- Los releases se firman con Ed25519 y su clave pública se obtiene **por otro canal** (ADR-009); los paquetes de macOS no están firmados ni notarizados.

## Opciones

| | Qué hace | Privilegios y confianza | Coste |
| --- | --- | --- | --- |
| **O1** | Sin cambios: orden para copiar. | Ninguno nuevo. | El usuario debe descargar el paquete y abrir una terminal. |
| **O2** | **Un solo instalador** en macOS (`.pkg` de distribución con la aplicación y el cliente) y, en Linux, instalar los dos `.deb` de la misma versión con una orden. El asistente sigue detectando la falta del cliente. | **Ninguna operación nueva en la aplicación.** Instala el sistema operativo, con su diálogo; el `.app` queda propiedad de root. | `productbuild` y su prueba; el job de release de escritorio pasa a depender del paquete del cliente. |
| **O3** | La aplicación **lleva el paquete dentro** y un botón «Instalar el cliente» lo abre en el instalador del sistema (macOS: `open` del `.pkg` en Installer.app; Linux: `pkexec apt install` de un `.deb` en una ruta de root). | En macOS, el instalador del sistema pide la autorización y enseña el paquete; la aplicación no ejecuta nada con privilegios. Una ruta de root (Linux) es segura; un `.pkg` dentro de un `.app` propiedad del usuario **no** lo es si lo ejecuta root (ver riesgos). | Un método más en el servicio (13), el paquete dentro del DMG y la dependencia entre jobs. |
| **O4** | La aplicación **descarga** el paquete de GitHub (`latest`) y lo instala **automáticamente**. | La aplicación pasa a ser un **canal de ejecución de código por red**. | Cliente HTTP en Go, verificación de firma, ruta de instalación segura, gestión de fallos de red. |

## Riesgos de O4 (lo que se pide hoy), con evidencia

1. **Es un *updater*.** ADR-019 decisión 9 lo excluye hasta tener su ADR y su firma: «una actualización automática es un canal de ejecución de código». Descargar y ejecutar como administrador un paquete de internet es exactamente eso.
2. **`latest` es un objetivo móvil.** Una aplicación 0.1.14 instalaría el cliente de la versión que haya mañana: sin garantía de compatibilidad con la aplicación y sin que nadie la haya probado junta. Lo correcto sería **fijar la versión de la propia aplicación** (`…/download/v0.1.14/…`), no `latest`.
3. **La confianza hay que traerla dentro.** Para ejecutar con privilegios lo descargado, la aplicación tendría que **llevar embebida la clave pública Ed25519** del responsable y verificar manifiesto, firma y SHA-256 (hoy esa clave se obtiene por otro canal a propósito). El `.pkg` no está firmado ni notarizado, así que el sistema no añade ninguna garantía.
4. **Se crea el primer cliente de red de la aplicación** (TLS, redirecciones, tamaños, tiempos, proxies) y depende de la disponibilidad de GitHub: sin red o con GitHub caído, el asistente falla justo en el primer paso. GitHub además ve la IP de cada instalación.
5. **Comprobar y ejecutar son dos momentos.** Si el paquete queda en un directorio del usuario entre la verificación y `installer`, un proceso del mismo usuario lo puede cambiar. La salida correcta es copiarlo a un directorio de root y **verificar la copia** dentro de la orden elevada.
6. **El diálogo no cubre el contenido.** El usuario autoriza «instalar el cliente», no «ejecutar estos bytes»; hoy autoriza operaciones sobre un binario que ya es de root.

## Riesgo de O3 con el paquete dentro del `.app` (macOS)

Si el `.pkg` va en `Atenea One.app/Contents/Resources` y **la propia aplicación lo ejecuta con privilegios**, root ejecutaría un fichero que un proceso del usuario puede sustituir: una escalada de privilegios local. Solo se evita (a) con el hash del paquete fijado en la aplicación y la verificación de **una copia de root**, o (b) **sin ejecutarlo con privilegios desde la aplicación**: abrirlo en el instalador del sistema, que pide su propia autorización y enseña lo que instala. Se propone (b).

## Recomendación

**O2 para macOS y Linux, sin tocar la superficie de la aplicación**, y descartar O4 por ahora:

1. **macOS:** un `.pkg` de distribución («Atenea One») que instala `Atenea One.app` en `/Applications` y el cliente, con una sola autorización y una sola versión. El `.dmg` actual se conserva para quien solo quiera la aplicación. Un `.app` instalado por un `.pkg` queda **propiedad de root y no del usuario**, lo que elimina el riesgo de la copia modificable por el usuario (el propietario exacto se comprueba al construirlo).
2. **Linux:** el asistente muestra la orden de **las dos instalaciones juntas** (`sudo apt install ./atenea-cli-*.deb ./atenea-desktop-*.deb`) y el `.deb` de escritorio declara `Depends: atenea (= versión)` para que `apt` impida una combinación inconsistente.
3. **El asistente no cambia de naturaleza:** sigue sin instalar, descargar ni abrir enlaces (ADR-021 decisión 2 se mantiene). Con O2 el cliente va en el mismo instalador, así que el caso «falta el cliente» pasa a ser excepcional.
4. **O3 solo si hace falta un botón** «Instalar el cliente» dentro de la aplicación tras O2: abrir el `.pkg` en el instalador del sistema (nunca ejecutarlo con privilegios desde la aplicación); costaría un método más en el servicio y se anotaría aquí.
5. **Si se quiere O4 de todos modos**, el mínimo aceptable es: versión **fijada a la de la aplicación**, clave pública embebida y verificación de manifiesto y SHA-256 **antes** de escribir, copia a un directorio de root y verificación de la copia en la orden elevada, límites de tamaño y de tiempo, un solo origen (`github.com/Tutitoos/atenea-one`) y un nuevo método en el servicio con su prueba de superficie. Es trabajo propio de otro ADR de actualizaciones y de firmar el `.pkg`.

## Consecuencias de O2

- **Release:** `release.yml` gana un job que construye el `.pkg` de distribución (necesita el `.pkg` del cliente y el `.app` ya empaquetado, así que depende de los jobs `native` y `desktop`), entra en el manifiesto y en la atestación y lleva su propia prueba (`pkgutil --expand`, contenido, propietario del `.app` en `/Applications`, `postinstall` del cliente intacto). La CI de escritorio no cambia.
- **El `postinstall` del cliente es el que ya existe** y sigue cubierto por `tests/macos/test_run_dir.py` y el humo privilegiado.
- **Comprobado al construirlo y publicarlo (v0.1.15):** el `.pkg` de distribución mantiene el `postinstall` y los permisos del cliente (`check-suite-pkg.sh`), la aplicación queda de root y arranca, y su ventana de primera ejecución se abre. Ver [v0.1.15](../operations/v0.1.15.md) §5.

## Decisión del responsable (2026-11-04)

**O2**, tal como se redactó en «Recomendación» (puntos 1 a 3). **No se implementa O3** (el botón dentro de la aplicación) ni **O4** (descarga automática): quedan descartadas por ahora, con las condiciones del punto 5 si alguna vez se retoma O4. La decisión 2 de ADR-021 **se mantiene**: la aplicación sigue sin instalar, descargar ni abrir enlaces. Implementación: primero el instalador de macOS, después el `.deb` de escritorio con su dependencia.
