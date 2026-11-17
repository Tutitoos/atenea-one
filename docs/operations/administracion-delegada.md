<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Administración delegada por dispositivo: guía de operación

- **Estado:** implementada en `main` (fases 2 a 6 de [ADR-026](../architecture/ADR-026-administracion-delegada-por-dispositivo.md)) y **sin publicar ni desplegar en ningún servidor**. Todo lo que se afirma aquí está probado con datos sintéticos (pruebas de integración, Vitest, Go y el ensayo [`rollback-0013.sh`](../../tests/stack/rollback-0013.sh)); **no se ha ejecutado en un despliegue real** ni con el diálogo de administrador del sistema real.
- **Para quién:** quien administra una red y quien opera el servidor.

## Qué permite

Un `owner` o `admin` de la organización concede a **un dispositivo aprobado** permisos sobre **su** red, con un alcance. El dispositivo los ejerce con su identidad de siempre (peticiones firmadas), desde la CLI o desde la aplicación de escritorio. Sin concesión nada cambia: un dispositivo solo actúa sobre sí mismo.

| Permiso | Permite |
| --- | --- |
| `read:devices` | Ver el inventario de la red (toda, con huella y quién dio de alta a cada uno). |
| `read:policies` | Ver las políticas de la red. |
| `write:join-codes` | Generar códigos de unión de un solo uso (máximo 1 h, 8 vivos). |
| `write:approve-devices` | Aprobar, **por huella completa**, un pendiente de su alcance. |
| `write:policies` | Crear y borrar políticas **entre dispositivos de su alcance** (nunca `any` ni prefijos). |
| `write:revoke-devices` | Revocar un dispositivo de su alcance. |

Alcance de las escrituras: `enrolled` (por defecto: lo que él dio de alta y él mismo), `devices` (lista de hasta 64) o `network`. **`network` concede la red entera**, también los equipos más sensibles; el panel lo advierte. Las lecturas cubren siempre toda la red.

Un delegado **no** concede ni amplía permisos, no cambia DNS, rutas de subred, roles, nombres ni la política de unión, y no borra políticas de un administrador. Las políticas conservan la regla inversa de [ADR-016](../architecture/ADR-016-politicas-de-acceso-minimas.md).

## Conceder, cambiar y retirar (solo en el panel)

1. Abre el panel, la red y el detalle de un dispositivo **aprobado**.
2. En «Administración delegada», **Conceder permisos**: marca los permisos y el alcance y guarda. **Cambiar permisos** sustituye la concesión; **Retirar permisos** la borra.
3. Concederla no exige que el dispositivo haga nada: el agente la lee en un máximo de 30 s.

Por API: `PUT`/`DELETE /api/v1/networks/{net}/devices/{dev}/grant` y `GET …/grants` ([contrato §23](../api/control-plane-v1.md)). Ningún dispositivo puede escribir concesiones. Un token de servicio solo puede **listar** las concesiones.

## Usarla

- **CLI** (root en Linux y macOS, consola elevada en Windows): `atenea network permissions|devices|join-code|approve|revoke|policy …`; ver [redes múltiples](redes-multiples-cli.md#administración-delegada-adr-026).
- **Aplicación de escritorio**: «Administración» en el detalle del propio equipo si hay concesión. Las lecturas no piden autorización; cada escritura pide la de administrador del sistema. **En Windows la aplicación no hace escrituras**: usa la CLI.

Alta típica de un equipo nuevo por un delegado: `join-code` → el equipo nuevo ejecuta `atenea network join` y muestra su huella → el delegado compara la huella completa y ejecuta `approve <huella>` → `policy add --from … --to …`.

## Orden de despliegue y compatibilidad

1. **Servidor primero.** La migración `0013` es aditiva (tabla `device_grants` y tres columnas de autoría; `created_by` admite nulo). Las rutas nuevas solo existen en el control plane nuevo.
2. **Después los agentes y la CLI.** Un agente nuevo contra un servidor anterior recibe `404` en `/device/admin/grant`: no muestra «Administración» y no falla. Una CLI nueva contra un servidor anterior responde «el servidor no admite administración delegada».
3. **Coste:** cada agente nuevo hace una petición firmada cada 30 s (hasta tres si tiene concesión), dentro del límite de 30 por minuto y dispositivo del control plane. Las rutas delegadas tienen además un límite propio de 60 por minuto.
4. No hay interruptor: sin concesión el efecto es esa lectura y nada más.

## Retroceso

La `0013` no se deshace. El control plane **anterior arranca con normalidad sobre el esquema ampliado** (ensayo `rollback-0013.sh`, 69 comprobaciones, con binarios de desarrollo y PostgreSQL sintético, no con Ansible, Compose ni imágenes reales):

- N lee y escribe organizaciones, redes, dispositivos y políticas con normalidad y emite configuración firmada; las políticas y los dispositivos que creó un delegado siguen legibles y N puede borrarlas.
- **N no ejerce las concesiones** (`/device/admin/*` responde `404`) y **no las borra al revocar un dispositivo**: queda una fila huérfana en `device_grants`. Al volver a N+1 esa fila no se ejerce (`403 device_not_approved`, porque comprueba el estado del dispositivo) pero sigue listada en `GET …/grants`. Es inocua y se puede borrar con `DELETE …/devices/{dev}/grant` (el servidor responde `404` si no la tiene) o ignorar.
- `--migrate` de N sobre una base con `0013` falla sin cambiar nada.
- Para retroceder: parar, arrancar el binario anterior **sin** `--migrate`. No borrar `device_grants` ni las columnas.

## Auditoría

Cada acción delegada queda con `actor_kind = device` y el dispositivo autor: `token.create`, `device.approve`, `device.revoke`, `policy.create`, `policy.delete`. Conceder y retirar: `device.grant` y `device.grant_revoke` (con `reason`: `admin` o `device_revoked`) como `user`. En el panel, «Auditoría» y los campos «Dado de alta por» y «Creada por».

## Cuando algo falla

| Respuesta | Significado y qué hacer |
| --- | --- |
| `403 grant_missing` | El dispositivo no tiene ese permiso. Concédelo en el panel. |
| `403 out_of_scope` | El destino (o un extremo de la política, o su autor) queda fuera del alcance. Amplía el alcance o actúa desde un administrador. |
| `403 device_not_approved` | El dispositivo está pendiente o revocado. |
| `401` | Firma inválida: comprueba el reloj del equipo (±60 s) y que no esté revocado. |
| `409 fingerprint_mismatch` | La huella no es la del dispositivo: no aprobar hasta aclararlo con quien se une. |
| `409 join_code_limit` | Hay 8 códigos vivos de ese delegado: esperar a que caduquen o se usen. |
| `404` en `/device/admin/*` | Servidor anterior a la `0013`. |
| La aplicación no muestra «Administración» | Sin concesión, servidor o agente anteriores, o la lectura aún no llegó (hasta 30 s). |

## Riesgo residual

Un delegado comprometido puede, dentro de su alcance, crear políticas entre dispositivos de ese alcance, aprobar pendientes de sus códigos y revocar. Con alcance `network` puede hacerlo en toda la red. Lo limitan el alcance, los topes y la auditoría; no se elimina. Conceder `network` o `write:revoke-devices` a un equipo es una decisión de confianza equivalente a dárselo a quien lo controla.

## Sin comprobar

Un despliegue real (servidor, agentes y CLI publicados), el diálogo de administrador del sistema real, la aplicación con una ventana abierta contra un servidor con concesiones, el panel en un navegador y la rama de Windows de la CLI en un equipo Windows (solo se compila en la CI).
