# Panel web, bloque A: rediseño de presentación y huecos sin servidor

- **Fecha:** 2026-10-28. **Aprobado por el responsable:** el alcance (bloque A completo ahora; bloque B con servidor para la v0.1.6). Las decisiones marcadas «[mía]» son del implementador y el responsable puede cambiarlas.
- **Ámbito:** solo `apps/web`. Sin cambios de servidor, de contrato ni de dependencias.

## Intención
El panel ya cubre casi todo el contrato (43 operaciones; solo falta borrar organización). El problema es cómo se presenta: sin jerarquía, con opciones dispersas, UUID por todas partes y formularios a ancho completo. El responsable pidió «arreglar el panel y su diseño y revisar que tenga todo lo que necesitamos».

## Objetivos
1. Navegación clara: migas de pan (Organización › Red › Sección), selector de red y secciones agrupadas.
2. Un sistema de diseño pequeño y consistente: botones con variantes, cabecera de página, tarjetas, campos con ayuda y error asociado, insignias de estado, estados vacíos con siguiente paso, anchura máxima de formularios.
3. Cubrir los huecos que no exigen servidor: cola de solicitudes pendientes con contador y «Rechazar», «Volver al nombre del sistema» y `hostname`/origen del nombre, miembros visibles para todos los roles, borrar organización con confirmación, página «Resumen» por red, aviso al fijar el nombre como administrativo.
4. Nombres en lugar de UUID donde el dato ya está en pantalla (políticas, topología, caminos); los UUID restantes, cortos y con copiar.
5. Código legible (se acabaron las líneas de miles de caracteres) y pruebas actualizadas.

## No objetivos
Nada que requiera servidor nuevo (nombres o correos de miembros, añadir por correo, renombrar red u organización, caducidad del certificado de emisor y versiones del servidor en el panel): es el bloque B, con la v0.1.6. Tampoco se cambia la autenticación OIDC, el cliente de API ni las dependencias.

## Antiobjetivos (lo que lo haría un fracaso aunque compile)
Perder accesibilidad existente (foco visible, enlace «saltar al contenido», etiquetas, `aria`, diálogos Radix, movimiento reducido, SNAT con radiogroup); romper la CSP, la hidratación de `#app` o la generación de tipos; ocultar una operación que hoy existe; que una acción destructiva parezca una normal.

## Restricciones que se conservan
`scripts/csp.mjs`, hidratación de `#app`, Zod sin JIT, `packages/api-client` (generado: no se edita), pruebas existentes (se actualizan, no se borran sin sustituir), copiar sin secretos persistentes.

## Decisiones de diseño
- **Navegación [mía]:** barra lateral con selector de organización y de red; secciones de red agrupadas: *Resumen*, *Dispositivos* (insignia con solicitudes pendientes), *Incorporar*, *Red* (DNS, certificado, subredes, políticas y zona de peligro) y *Observabilidad* (topología, métricas, relays, auditoría). Migas de pan arriba. Cada sección tiene su propia URL (`?org=&net=&view=`) como hoy.
- **Página de organización [mía]:** cabecera con nombre y rol; pestañas *Redes*, *Miembros* y *Ajustes*. Cada red es un **enlace** (no un botón en un título) con CIDR y estado del certificado. «Crear red» y «Crear organización» abren diálogos. Los miembros los ve cualquier rol (solo el owner modifica); «Ajustes» contiene borrar organización (owner), con confirmación escribiendo el nombre.
- **Resumen de red [mía]:** tarjetas con dispositivos (aprobados, pendientes, revocados, conectados), solicitudes pendientes con acción directa, política de unión, políticas, certificado instalado o no y relays declarados; y los últimos 5 eventos de auditoría. Todo sale de consultas que ya existen.
- **Dispositivos [mía]:** cola de pendientes arriba (huella, aprobar y rechazar), tabla compacta con búsqueda, filtros por estado y tipo y orden, y panel de detalle por fila para huella, rutas, DNS y caminos. «Rechazar» usa la revocación existente.
- **Tokens de diseño [mía]:** se alinean colores, radios y tamaños con la aplicación de escritorio (`bg/side/card/sel/ok/warn/bad/off`); se conserva Geist (ya es dependencia).
- **Actualización [mía]:** un único indicador «Actualizado hace N s» en la cabecera, no uno por bloque, y la región `aria-live` solo anuncia cambios y errores, no el reloj.
- **Formularios:** anchura máxima de 560 px, ayuda debajo, error asociado al campo con `aria-describedby`, botón primario a la izquierda; acciones destructivas con variante `danger` y confirmación.
- **Selector CSS:** el `<aside>` explicativo de Políticas deja de heredar los estilos de la barra lateral.

## Qué queda al implementador
Nombres de componentes, estructura de ficheros, textos finos en español, y la composición exacta de cada tarjeta, siempre dentro de lo anterior.

## Contrato de implementación de la base (etapa 1)

Los componentes compartidos están en `apps/web/app/ui.tsx`. Sus propiedades nuevas son:

- `Button`: atributos nativos de botón, `variant` (`primary`, `secondary`, `danger`, `link`) y `loading`.
- `PageHeader`: `title`, `description?`, `actions?`.
- `Card`: `title?`, `children`, `className?`.
- `Field`: `label`, `help?`, `error?`, `children(props)`; el control debe propagar los atributos recibidos (`id`, `aria-describedby`, `aria-invalid`).
- `StatusBadge`: `tone?` (`ok`, `warn`, `bad`, `neutral`) y `children` con texto de estado.
- `EmptyState`: `message`, `action`.
- `CopyButton`: `value`, `label?`; `ShortId`: `value`.
- `ConfirmDialog`: `trigger`, `title`, `description`, `confirmation?`, `label?`, `confirm` asíncrono.
- `UpdatedIndicator`: `updatedAt` (marca temporal en milisegundos); se monta una vez en la cabecera de red.
- `QueryAnnouncements`: sin propiedades; se monta una vez en el shell y anuncia cambios de datos y errores, nunca el reloj.
- `Breadcrumbs` (`breadcrumbs.tsx`): `organization` y `network?` con `id`/`name`, y `section?`.

El registro único está en `apps/web/app/sections.tsx`: cada entrada tiene `id` estable para `view`, `label`, `group` y `Component`. El shell genera navegación y montaje desde ese registro. Se conservan los identificadores históricos de las vistas; `Resumen` es el predeterminado. `SectionProps` entrega `net`, consultas `network`, `devices`, `routes`, `role`, `canEdit`, `elapsed` y `refresh`. El enlace de solicitudes pendientes usa `view=Dispositivos&status=pending`; el adaptador pasa ese estado como filtro inicial a Dispositivos sin recortar el inventario.

La navegación del shell usa enlaces, incluido «Todas las redes» para volver a la organización. La entrada con `id=Observabilidad` se presenta en la barra lateral como «Vista conjunta», sin cambiar su URL ni el registro. El selector de red usa el nombre de la consulta de detalle cuando la lista de redes todavía no incluye la red seleccionada. Los avisos usan `--warn` y `--warn-ink` con valores para ambos temas; los estados locales vacíos no reservan espacio vertical.

El pulido visual conserva la API de componentes: las tarjetas de Resumen no se estiran en su fila; las huellas y sus controles de copia permanecen alineados; las direcciones ausentes no añaden líneas vacías. Políticas presenta «Todos los puertos», «Puerto N» o «Puertos N–M» y mantiene el borrado con confirmación en un botón de contorno. El tamaño de SVG de topología está acotado a ese componente para no deformar los iconos de la leyenda de métricas; los puntos de las series tienen relleno semántico explícito para ser visibles en ambos temas. A 900 px o menos, la barra lateral pasa al flujo superior y las tablas conservan desplazamiento propio.

## Estado de implementación (2026-10-28)
Implementado el bloque A completo en `apps/web`, sin tocar servidor, contrato ni dependencias. Pasan `typecheck`, `lint`, **96 pruebas** y `build` (más cabeceras y comprobación de documentos). Se vio renderizado en Chrome sin cabecera, con datos **sintéticos** servidos por un simulador local, en tema claro y oscuro y a 800 px de ancho.

**Hecho:** sistema de diseño compartido, navegación con migas y selector de red, página de organización (pestañas, miembros visibles para todos los roles, borrar organización), Resumen de red, Dispositivos (cola de pendientes con aprobar y rechazar, tabla con búsqueda, filtros y orden, detalle por fila, «Volver al nombre del sistema», PATCH solo de lo cambiado), Incorporar, DNS y certificado con zona de peligro, Políticas con dispositivos por nombre, Subredes, Observabilidad con nombres y Auditoría con frases.

**No verificado:** otros navegadores y dispositivos reales; contraste de todos los estados interactivos; comportamiento con un servidor real (el panel desplegado es el de la v0.1.5 hasta publicar la v0.1.6); el tamaño del paquete sigue en torno a 790 kB (234 kB comprimido) y no se ha optimizado.

**Prueba contra el laboratorio real (2026-10-28):** `bash tests/e2e-panel/run.sh` (control plane real, PostgreSQL 17, Dex con OIDC y dos agentes Linux con NAT, manejados con Chromium) pasa **3 de 3**: hidratación con CSP, recorrido de dos agentes (P2P directo, relay UDP, relay TLS, revocación, accesibilidad en oscuro y móvil) y rutas (SNAT explícito, DNS dividido y revocar ruta). Las pruebas se adaptaron a la interfaz nueva sin debilitar sus aserciones. La ejecución reveló **cuatro defectos reales** que las pruebas con datos simulados no habían visto, ya corregidos con prueba de regresión que falla sin el arreglo: (1) dos botones «Crear red» a la vez en una organización sin redes; (2) una organización recién creada trataba al propietario como miembro hasta 30 s por no invalidar la sesión; (3) contraste de `--muted` en el tema claro de 4,19:1 (por debajo de AA), ahora `#626267`; (4) la confirmación «Ruta aprobada» se perdía al refrescar porque el diálogo se desmontaba.

**Bloque B (con servidor, v0.1.6):** nombres y correos de miembros y añadir por correo, renombrar red y organización, caducidad del certificado de emisor y versiones del servidor en el panel.
