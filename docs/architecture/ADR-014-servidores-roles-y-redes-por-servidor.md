# ADR-014: Servidores con varias redes, rol de servidor y pulido de la aplicación

- **Estado:** **Propuesto** (2026-10-28). Nada de esto está implementado. Las decisiones de §5 las toma el responsable antes de empezar la fase 2.
- **Relacionados:** ADR-011 (aplicación de escritorio), ADR-012 (nombres de dispositivo, metadatos sin firmar), ADR-013 (varias redes por dispositivo).

## 1. Qué se ha pedido

Con la aplicación funcionando contra el piloto, el responsable pidió (cuatro capturas):

1. **Centrar** el estado «● Conectado»: el punto queda más bajo que el texto.
2. **Icono de copiar** en «Direcciones del túnel» (las direcciones IPv4 e IPv6 no se pueden copiar con un gesto como la clave pública).
3. **El servidor no debe aparecer en «Dispositivos»** y su detección debe ser **automática**. La marca «Servidor» local (commit `bc68bc6`) es provisional: se retira cuando exista la detección.
4. **Mejorar la información del servidor**: hoy el detalle de «panel…» muestra cuatro contadores y «Relays (0)».
5. **Un servidor con varias redes**: el servidor deja de ser «la red». Un mismo servidor aloja varias redes y la aplicación debe reflejarlo.

## 2. Qué existe hoy (comprobado en el código y en el piloto)

| Aspecto | Estado |
| --- | --- |
| Grupo «Servidor» de la aplicación | Se deriva de `status.sync.url` de **la red seleccionada**: un servidor por red, sin relación entre redes del mismo servidor. |
| El VPS como dispositivo | El VPS ejecuta su propio agente y está inscrito en la red como un dispositivo más (nombre `vps-…`, IP de túnel). Nada en el servidor ni en el estado del agente lo distingue de otro dispositivo. |
| Reconocerlo por dirección | **No es fiable:** desde un Mac, el `endpoint` del VPS es una IP de Tailscale distinta de la IP a la que resuelve el panel. Reconocerlo por nombre sería adivinar. |
| Información que ya tiene el agente | Último sondeo y errores del control plane, contadores de anuncios y avisos del coordinator, relays con estado y RTT medido, NAT/STUN observado, validez del lease. |
| Información que **no** existe | Versión de cada servicio del servidor, reloj del servidor frente al local, caducidad del certificado de emisor de la red. El control plane solo expone `/healthz`. |
| Servidor con varias redes | **Sí** en el servidor: organizaciones → redes; coordinator y relay separan por `network_id` (ADR-013 §2). **No** en el cliente: la aplicación no agrupa redes por servidor. |
| El VPS en varias redes | Un agente es de una red (ADR-013): para estar en varias necesita una instancia por red. El VPS sigue con la instancia heredada (una sola red). |
| Marca «Servidor» actual | Local a un equipo, en el almacenamiento del webview, activada a mano. No viaja. |

## 3. Propuesta

### 3.1 Pulido de presentación (fase 1)

Centrado vertical del estado (punto y texto en la misma línea base) y un botón de copiar con **icono** (accesible: `aria-label` y confirmación breve) en cada dirección del túnel y en los valores copiables de los detalles. Sin cambios de servidor ni de versión.

### 3.2 Rol de servidor, asignado por el administrador y detectado después sin intervención (fases 2–3)

- **Dato:** columna `devices.role text NOT NULL DEFAULT 'device' CHECK (role IN ('device','server'))` (migración nueva).
- **Quién lo fija:** un **administrador**, no el propio dispositivo. Dos vías: (a) al crear el token de inscripción (`role: "server"`; el dispositivo que lo consuma nace con ese rol) y (b) un cambio explícito sobre un dispositivo existente (endpoint de administración y panel). El aprovisionamiento del VPS (Ansible) crea su token con `role: server`: a partir de ahí **ningún usuario tiene que marcar nada** y la aplicación lo detecta sola.
- **Cómo llega a la aplicación:** `GET /api/v1/device/peers` (ya firmado y limitado a los pares visibles) devuelve también `role`. La aplicación agrupa en «Servidores» a los pares con `role = server` y los quita de «Dispositivos».
- **Naturaleza:** metadato de **presentación**, sin firmar, igual que los nombres (ADR-012). **Nunca** se usa para identidad, autorización ni rutas. Que lo fije el administrador evita que un dispositivo se anuncie como servidor.
- **Compatibilidad:** un servidor sin el campo (versión anterior) devuelve pares sin `role`: la aplicación los muestra todos como dispositivos, sin inventar nada.
- **Alternativas descartadas:** detectar por IP o por nombre (comprobado que falla, §2); que el agente declare su rol (cualquier dispositivo podría etiquetarse como servidor); mantener la marca local (no viaja y exige un gesto por equipo).

### 3.3 Información del servidor (fase 4)

Dos partes, de menor a mayor coste:

1. **Sin tocar el servidor:** mostrar con los datos que el agente ya tiene: URL, latencia y último sondeo correcto, errores recientes, relays con estado, transporte y RTT, NAT/STUN observado, validez del lease y el desfase de reloj estimado a partir de las respuestas (si el agente lo conserva; si no, se omite).
2. **Endpoint nuevo `GET /api/v1/server/info`**, firmado por el dispositivo como el resto: versión y commit de control plane, coordinator y relay, hora del servidor y **caducidad del certificado de emisor de la red del dispositivo** (hoy caduca el 2026-11-26 y nadie lo ve). Solo datos del servidor y de la red del propio dispositivo: nada de otras organizaciones. Se documenta en OpenAPI.

### 3.4 Un servidor, varias redes (fases 5–6)

- **Clave del servidor en el cliente:** el host de `url` de cada red unida (`network.json`), normalizado. No hay entidad nueva en el servidor: el servidor ya aloja varias redes.
- **Navegación:** Servidores → Redes → Dispositivos. Una sección «Servidores» lista cada host con las redes unidas a él y su estado agregado; el selector de red de la cabecera agrupa por servidor. El detalle de un servidor muestra su información (§3.3), sus redes con Conectar/Desconectar y los dispositivos con rol de servidor de esas redes.
- **Información compartida:** relays y coordinator son del servidor, no de la red: se leen de cualquier red conectada a ese servidor (la que responda).
- **El VPS en varias redes:** exige migrar su instancia heredada al modelo de redes (`atenea network migrate-legacy`, ya implementado) y una instancia por red con **un puerto UDP distinto y publicado** por cada una (`atenea_extra_udp_ports`). Cada red tiene sus propias claves en el VPS (ADR-013).

## 4. Plan por fases

| Fase | Contenido | Tamaño | Toca | Prueba de salida |
| --- | --- | --- | --- | --- |
| **0** | Aceptar este ADR y resolver §5. | S | docs | ADR aceptado. |
| **1** | Pulido: centrado del estado e iconos de copiar (§3.1). | S | solo aplicación | Pruebas de pantalla y accesibilidad; captura en macOS. |
| **2** | Rol de servidor en el servidor: migración, token con `role`, cambio por administrador, `role` en `/device/peers`, panel, OpenAPI (§3.2). | M | control plane, panel, CLI (`enroll`) | Pruebas contra PostgreSQL real: nace con rol, solo un administrador lo cambia, aislamiento entre redes, servidor viejo sin campo. |
| **3** | Aplicación: grupo «Servidores» automático desde `role`; se retira la marca local. | S | aplicación | Pruebas con y sin `role`; captura. Depende de la fase 2. |
| **4** | Información del servidor, parte 1 (datos del agente) y parte 2 (`/server/info`) (§3.3). | M | aplicación; control plane en la parte 2 | Parte 2: pruebas contra PostgreSQL y firma ajena rechazada. |
| **5** | Aplicación: agrupar redes por servidor (§3.4). | M | solo aplicación | Pruebas con dos redes del mismo servidor y de servidores distintos. Independiente de 2–4. |
| **6** | Piloto: migrar el VPS (y el CT) al modelo de redes, una segunda red real en el mismo servidor **con pares reales** y soak con dos redes. | M | piloto | Medido, con la huella del binario. Cierra pendientes de ADR-013. |

**Orden recomendado:** 1 → 5 → 2 → 3 → 4 → 6. Las fases 1 y 5 solo cambian la aplicación: se pueden instalar sin tocar el agente ni el servidor del piloto. Las fases 2, 4 (parte 2) y 6 cambian el servidor y exigen una versión nueva, copia verificada y despliegue con autorización; **no deben hacerse durante un soak** porque reinician el coordinator y cambian las versiones de los agentes.

## 5. Decisiones que necesita el responsable

1. **Quién fija el rol:** el administrador (token de inscripción y cambio explícito), como se propone, o alguna otra vía.
2. **Un solo rol (`server`) o más** (por ejemplo `relay`, `subnet-router`): se propone solo `server` ahora y ampliar la restricción `CHECK` cuando haya un uso real.
3. **Visibilidad de las versiones del servidor** para cualquier dispositivo aprobado de la red (§3.3): se propone sí, por ser datos de despliegue sin información de otras organizaciones.
4. **Agrupación de servidores por host exacto** (propuesto) o por dominio base.
5. **Cuándo migrar el VPS** (fase 6): antes o después del soak y de la auditoría.

## 6. Riesgos y límites

- **El rol es un rótulo, no una garantía.** Lo fija un administrador y no se firma: no sustituye a la política de acceso.
- **Servidor y aplicación de versiones distintas** durante la transición: la aplicación nueva debe funcionar con un servidor sin `role` ni `/server/info` (se muestra lo que haya).
- **Más redes en el VPS = más puertos UDP publicados** y más sesiones: sin medir.
- **Cambiar el servidor en medio del soak lo invalida:** ver el orden de §4.
- **Sin comprobar:** todo lo propuesto. Windows queda fuera (las redes por instancia no están soportadas allí).
