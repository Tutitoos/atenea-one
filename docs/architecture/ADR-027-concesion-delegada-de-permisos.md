<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-027: Concesión delegada de permisos (administrar permisos desde un dispositivo)

- **Estado:** **Aceptado** por el responsable el 2026-11-18, con las seis decisiones D1 a D6 tal como se propusieron. Fases 2 a 6 en curso. **Modifica** una regla de [ADR-026](ADR-026-administracion-delegada-por-dispositivo.md) (§3.4 «Sin escalada», la parte de que un dispositivo no concede permisos) y lo hace de forma acotada.
- **Relacionados:** [ADR-026](ADR-026-administracion-delegada-por-dispositivo.md) (concesiones, alcance y rutas `/device/admin/`), [ADR-020](ADR-020-token-de-servicio-de-solo-lectura.md) (el token de servicio sigue siendo de solo lectura), [ADR-021](ADR-021-asistente-de-primera-ejecucion.md) y [ADR-024](ADR-024-servicios-expuestos-por-dispositivo.md) (lista cerrada de operaciones elevadas de la aplicación).

## Contexto

Hechos comprobados en el código de la 0.1.33:

- **Conceder, cambiar o retirar permisos solo lo hace un `owner` o `admin` con sesión en el panel** (`PUT`/`DELETE /api/v1/networks/{net}/devices/{dev}/grant`). Ninguna ruta de dispositivo escribe concesiones (ADR-026 §3.4 y §«Amenazas»).
- **La aplicación de escritorio no tiene sesión de administrador de la red.** Firma con la identidad del dispositivo. Ser administrador del sistema operativo (el diálogo de elevación) no da autoridad sobre la red.
- **El agente solo lee la concesión del propio equipo** (`GET /device/admin/grant`). Las concesiones de los demás las lista `GET …/grants`, que exige sesión de miembro o token de servicio.
- **Petición del responsable:** que cada dispositivo tenga una pestaña **Permisos** que muestre sus permisos y, si quien mira tiene acceso de administración, permita configurarlos desde ahí, en la aplicación de escritorio.

Hacerlo exige que un dispositivo pueda escribir concesiones. Es justo lo que ADR-026 excluyó para evitar la escalada y la delegación en cadena; por eso hace falta decidirlo aquí antes de escribir código.

## Decisión propuesta

Un `owner` o `admin` puede conceder a un dispositivo aprobado un permiso nuevo, **`write:grants`** («administrar permisos»). Quien lo tiene puede conceder, cambiar y retirar permisos a **otros dispositivos de su alcance**, con tres topes que impiden subir de nivel. Se ejerce con la identidad de siempre (peticiones firmadas); no se crean credenciales ni sesiones nuevas.

### Decisiones a aceptar

- **D1. Un permiso nuevo, `write:grants`.** Solo lo concede un administrador desde el panel. Un delegado nunca lo concede a nadie: **la delegación llega a un solo nivel** (administrador → delegado con `write:grants` → delegados sin él).
- **D2. No se concede más de lo que se tiene.** Los permisos que concede un delegado son un subconjunto de los suyos y **nunca incluyen `write:grants`**.
- **D3. El alcance del que recibe cabe en el alcance de quien concede.** Con alcance `enrolled` o `devices`, solo puede conceder el alcance `devices` con una lista contenida en el suyo (hasta 64). Con alcance `network`, puede conceder `enrolled`, `devices` o `network`. El destinatario del permiso tiene que estar dentro del alcance de quien concede.
- **D4. Un delegado solo modifica o retira las concesiones que él dio.** Las de un administrador no las toca, igual que ocurre con las políticas (ADR-026 §3.2). **Nunca cambia su propia concesión.** Cada concesión guarda, además de `granted_by`, `granted_by_device` (nulo si la dio un administrador).
- **D5. Cascada.** Si cambia o se retira la concesión de un delegado con `write:grants`, o se revoca el dispositivo, se retiran en la misma transacción **todas las concesiones que él dio**. No quedan permisos cuyo origen ya no existe.
- **D6. Lectura.** Un dispositivo con `write:grants` lee las concesiones de los dispositivos de su alcance (para mostrarlas y editarlas). Sin ese permiso, cada dispositivo solo ve la suya. El token de servicio sigue como hoy (lista todas, solo lectura).

### Rutas nuevas (firma `AtDevice`, bajo `/api/v1/device/admin/`)

- `GET grants`: concesiones de los dispositivos del alcance (requiere `write:grants`).
- `PUT devices/{dev}/grant` y `DELETE devices/{dev}/grant`: mismo cuerpo y validación que la ruta de administrador, más las reglas D2 a D5. Códigos estables: `403 grant_missing` (sin `write:grants`), `403 out_of_scope`, `403 grant_exceeds_own` (permiso o alcance mayor que el propio, o `write:grants`), `403 grant_self` (su propia concesión) y `403 grant_not_yours` (concesión de un administrador u otro delegado).
- Migración `0014` aditiva: `device_grants.granted_by_device` (nulo = administrador) y el permiso nuevo en la lista cerrada. Un control plane anterior ignora el permiso y no tiene las rutas.
- Auditoría: `device.grant` y `device.grant_revoke` con `actor_kind = device`, el delegado autor y, en la cascada, `reason = grantor_changed`.
- Límites: la tasa propia de ADR-026 (60 por minuto) y como mucho 64 concesiones dadas vivas por delegado.

### CLI

`atenea network grants` y `atenea network grant <dispositivo> --permission … --scope …` / `ungrant <dispositivo>`, con `--network` y `--json`. Mismos privilegios que el resto de `atenea network`.

### Aplicación de escritorio

- **Pestaña «Permisos» en el detalle de cada dispositivo.** En el propio equipo muestra su concesión. En los demás, la del dispositivo si quien mira tiene `write:grants` y el destino está en su alcance; si no, indica que no consta.
- **Escritura:** dos operaciones elevadas nuevas (`grant-set`, `grant-remove`), cada una una sola llamada al CLI fijo con argumentos de formato cerrado y el diálogo de administrador del sistema (D5 de ADR-026). La superficie del servicio vinculado pasa de 15 a 17 métodos. En Windows la aplicación sigue sin hacer escrituras (usa la CLI).
- El agente gana una lectura (`grants`) cuando el equipo tiene `write:grants`; el socket de estado sigue admitiendo solo `status`.

### Panel web

La lista de permisos del panel incluye `write:grants`. En el detalle de una concesión se ve quién la dio (administrador o delegado).

## Amenazas y límites

| Riesgo | Tratamiento |
| --- | --- |
| Un delegado con `write:grants` comprometido reparte permisos | Solo dentro de su alcance, solo permisos que ya tiene y sin `write:grants`; tope de 64 concesiones dadas, tasa propia y auditoría. Quien le concede `write:grants` con alcance `network` concede la red |
| Escalada: concederse más o ampliar el alcance | `403 grant_self` y `403 grant_exceeds_own`; el destinatario nunca puede superar a quien concede |
| Delegación en cadena | `write:grants` solo lo concede un administrador (D1): la profundidad es uno |
| Pisar o retirar permisos de un administrador | `403 grant_not_yours` (D4) |
| Permisos huérfanos tras retirar al que los dio | Cascada en la misma transacción (D5) |
| Un servidor anterior con una CLI o app nuevas | Las rutas responden `404`; la CLI y la app lo muestran como «el servidor no admite conceder permisos». Se despliega el servidor antes |
| Más superficie elevada en la aplicación | Dos operaciones nuevas con argumentos de formato cerrado y una prueba de la cadena exacta |
| Exposición de quién tiene qué | Solo con `write:grants` y solo para dispositivos de su alcance |

## Plan por fases

Cada fase es un cambio revisable con CI. No se publica una versión por fase.

1. **ADR** (este documento).
2. **Control plane:** migración `0014`, rutas, reglas D2 a D5, cascada, OpenAPI, doc §25 y pruebas contra PostgreSQL 17.
3. **CLI:** `grants`, `grant`, `ungrant`.
4. **Panel:** el permiso nuevo y la autoría.
5. **Agente y aplicación:** lectura `grants`, pestaña «Permisos» y las dos operaciones elevadas (aprovecha la estructura de pestañas del detalle).
6. **Guía de operación y retroceso:** ensayo de la `0014` y pruebas en el piloto.

## Pruebas de aceptación

- Un delegado sin `write:grants` recibe `403 grant_missing`.
- Con `write:grants` y alcance `devices {A,B}`: concede `read:devices` a A con alcance `devices {B}`; no puede conceder a un dispositivo fuera de `{A,B}`, ni `write:policies` si no la tiene, ni `write:grants`, ni un alcance mayor, ni cambiar la suya, ni tocar la concesión de un administrador.
- Retirar o cambiar la concesión del delegado, o revocarlo, borra lo que él dio en la misma transacción y lo audita.
- Un administrador sigue pudiendo cambiar o retirar cualquier concesión desde el panel.
- Un control plane anterior sobre la `0014` arranca con normalidad y no ejerce `write:grants`.

## Qué no se hace en esta versión

Delegar `write:grants` (cadena), que un delegado conceda alcance `network` sin tenerlo, conceder desde el token de servicio, iniciar sesión de administrador de la red en la aplicación, y cambiar el rol, el nombre administrativo, la política de unión, las rutas de subred, el DNS o el certificado de emisor (siguen siendo del administrador).
