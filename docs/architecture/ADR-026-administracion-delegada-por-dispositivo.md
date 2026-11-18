<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-026: Administración delegada por dispositivo (permisos por red, con alcance)

- **Estado:** **Aceptado** por el responsable el 2026-11-17, con las cinco decisiones D1 a D5 tal como se recomendaron. **Fases 2 a 7 implementadas, publicadas en la [0.1.33](../operations/v0.1.33.md) y desplegadas en el piloto.** Migración `0013`, gestión de concesiones por un administrador y rutas `/api/v1/device/admin/` con permisos, alcance, reglas y límites (control plane, pruebas contra PostgreSQL 17), las órdenes de `atenea network` (probadas contra un servidor HTTP simulado y, en el piloto, con la CLI real), el panel web (conceder, cambiar y retirar permisos con su alcance; «Dado de alta por» y «Creada por»; **no visto en un navegador**), las tres lecturas del agente (`status.admin`, cada 30 s) y la sección «Administración» de la aplicación de escritorio con `AdminApply` (cinco escrituras, un único método nuevo: 15 en total). **Fase 7:** [guía de operación](../operations/administracion-delegada.md) y ensayo del retroceso de la `0013` con `tests/stack/rollback-0013.sh` (69 comprobaciones con los binarios reales del control plane y de la CLI, peticiones firmadas AtDevice y PostgreSQL 17 sintético, **no** con Ansible, Compose ni imágenes: el control plane `v0.1.32` arranca con normalidad sobre la `0013`, no ejerce las concesiones y, al revocar un dispositivo, deja huérfana su fila de `device_grants`, que N+1 no ejerce). **Prueba en el piloto (2026-11-18):** el ciclo completo de la aceptación con la CLI real y un equipo desechable (código del delegado, unión pendiente, aprobación por huella completa, política entre los dos, tráfico solo al puerto concedido y limpieza); los rechazos (huella distinta, pendiente ajeno, política `any`) solo están probados en el ensayo. **Limitaciones:** en Windows la aplicación no hace las escrituras (el canal de resultado de UAC no devuelve el código de unión y no se ha probado ahí; se usa la CLI); la aplicación no se ha visto en una ventana con una concesión ni se ha probado el diálogo del sistema desde ella; la rama de Windows de la CLI solo se compila en la CI.
- **Relacionados:** [ADR-016](ADR-016-politicas-de-acceso-minimas.md) (políticas con deny por defecto y la regla inversa), [ADR-013](ADR-013-redes-multiples-y-union-por-codigo.md) (código de unión y aprobación), [ADR-020](ADR-020-token-de-servicio-de-solo-lectura.md) (el token de servicio sigue siendo de solo lectura), [ADR-019](ADR-019-aplicacion-de-escritorio-wails.md), [ADR-021](ADR-021-asistente-de-primera-ejecucion.md) y [ADR-024](ADR-024-servicios-expuestos-por-dispositivo.md) (la aplicación no tiene privilegios y su lista de operaciones elevadas es cerrada), [ADR-017](ADR-017-redes-por-instancia-en-windows.md) (elevación con UAC en Windows).

## Contexto

Hechos comprobados en el código:

- **Solo un usuario del panel administra una red.** Crear códigos de unión (`POST /api/v1/networks/{net}/enrollment-tokens`), aprobar o revocar un dispositivo y crear o borrar políticas exigen sesión OIDC con rol `owner` o `admin` de la organización. El token de servicio (ADR-020) solo lee.
- **Un dispositivo solo actúa sobre sí mismo.** Con su firma (`Authorization: AtDevice v1 …`, clave de identidad Ed25519) lee su configuración y sus pares (`/device/config`, `/device/peers`), informa su estado, declara rutas y se retira (`/device/leave`). No ve el inventario ni las políticas de la red.
- **Aprobar no da acceso.** Las políticas son deny por defecto y cada una nombra un origen y un destino (`any`, un dispositivo o un prefijo), un protocolo y un rango de puertos (ADR-016). Un dispositivo recién aprobado no tiene pares hasta que un administrador crea políticas hacia él y desde él.
- **Consecuencia práctica:** montar un equipo nuevo (por ejemplo un nodo de pruebas junto a un equipo de desarrollo) exige que un administrador genere el código, compare la huella, apruebe y cree varias políticas a mano, cada vez. Quien está operando ese equipo no puede hacerlo aunque el administrador confíe en él para ese ámbito.
- **La alternativa «todo dispositivo aprobado ve a todos»** se descartó para redes donde conviven entornos con distinta sensibilidad (por ejemplo producción y desarrollo): daría acceso a producción a cualquier alta.

## Decisión

Un `owner` o `admin` puede conceder a **un dispositivo aprobado** permisos de administración sobre **su** red, limitados por un **alcance**. El dispositivo los ejerce con su identidad de siempre (peticiones firmadas), desde la CLI o desde la aplicación de escritorio. No se crean credenciales nuevas.

### Decididas por el responsable

- **D1. El dispositivo delegado aprueba lo que da de alta**, indicando siempre la huella completa del dispositivo pendiente. No hay aprobación automática.
- **D2. Alcance por defecto: «los dispositivos que él dio de alta, más él mismo».** Las otras dos formas (lista explícita y toda la red) las elige el administrador al conceder.
- **D3. Sin etiquetas ni grupos** en esta decisión. Las políticas siguen nombrando dispositivos.
- **D4. Solo dispositivos.** El token de servicio no gana permisos de escritura.
- **D5. En la aplicación, cada escritura pide la autorización de administrador del sistema**; las lecturas no.

### 1. Permisos

| Permiso | Permite |
| --- | --- |
| `read:devices` | Ver el inventario de la red (nombre, estado, direcciones, huella, quién lo dio de alta). |
| `read:policies` | Ver las políticas de la red. |
| `write:join-codes` | Generar un código de unión de un solo uso, con caducidad corta. |
| `write:approve-devices` | Aprobar un dispositivo pendiente **dentro de su alcance**, indicando su huella. |
| `write:policies` | Crear y borrar políticas **dentro de su alcance**. |
| `write:revoke-devices` | Revocar un dispositivo **dentro de su alcance**. |

Una concesión es un conjunto de permisos más un alcance. Un dispositivo tiene como mucho una concesión por red; concederla de nuevo la sustituye.

### 2. Alcance

- `enrolled` (por defecto): los dispositivos cuyo código de unión generó este dispositivo, más él mismo.
- `devices`: una lista explícita de dispositivos de la red (hasta 64), que puede incluir al propio.
- `network`: todos los dispositivos de la red.

El alcance limita las escrituras. Las lecturas (`read:*`) cubren siempre toda la red: quien administra una parte necesita ver con qué convive, y el inventario no contiene secretos.

### 3. Reglas

1. **Políticas delegadas.** Origen y destino deben ser dispositivos (`kind: device`) y **ambos** deben estar dentro del alcance. Un delegado no crea políticas `any` ni por prefijo. Así, con el alcance por defecto, un equipo de desarrollo puede conectar entre sí lo que él dio de alta y consigo mismo, y **no** puede crear ninguna política que nombre a un dispositivo ajeno.
2. **Borrado.** Un delegado solo borra políticas creadas por un delegado (él u otro) cuyos dos extremos estén en su alcance. Nunca las de un administrador.
3. **Aprobación.** Solo de dispositivos pendientes que entraron con un código suyo (o cualquiera, con alcance `network`), y la petición lleva la huella completa; si no coincide se rechaza.
4. **Sin escalada.** Un dispositivo no concede, amplía ni delega permisos; no puede cambiar el rol, el nombre administrativo ni la política de unión de la red; no aprueba rutas de subred ni toca DNS o el certificado de emisor.
5. **Ciclo de vida.** Revocar un dispositivo (o que se retire) borra su concesión. Lo que creó se conserva y sigue siendo administrable por un administrador. Un dispositivo que deja de estar aprobado pierde el efecto de la concesión de inmediato.
6. **Auditoría.** Cada acción delegada se registra con `actor_kind = device` y el dispositivo autor; conceder y retirar permisos también.
7. **Límites.** Por dispositivo delegado: tasa de peticiones propia, como mucho 8 códigos vivos y 256 políticas creadas. Los códigos delegados caducan como máximo a la hora.

### 4. Control plane

- **Migración `0013`** (aditiva): tabla `device_grants` (`network_id`, `device_id`, `scopes`, `scope_kind`, `scope_devices`, `granted_by`, `granted_at`); `enrollment_tokens.created_by_device` y `devices.enrolled_by_device` (nulos si lo hizo un usuario); `access_policies.created_by_device` (nulo si lo creó un administrador). En `enrollment_tokens` y `access_policies`, `created_by` pasa a admitir nulo cuando el autor es un dispositivo.
- **Para el administrador** (sesión, `owner` o `admin`): `GET /api/v1/networks/{net}/grants`, `PUT /api/v1/networks/{net}/devices/{dev}/grant` y `DELETE` de la misma ruta.
- **Para el dispositivo** (firma `AtDevice`, mismo anti-repetición y ventana de tiempo que hoy), bajo `/api/v1/device/admin/`: `grant` (los permisos propios), `devices`, `policies` (listar, crear, borrar), `join-codes` (crear), `devices/{dev}/approve` y `devices/{dev}/revoke`. Sin permiso o fuera de alcance responde `403` con un código estable (`grant_missing`, `out_of_scope`); un destino inexistente o de otra red responde `404`.
- **No cambian** el túnel, la configuración firmada ni `/device/peers`. Las políticas delegadas se compilan igual que las demás. El agente solo gana, en la fase 6, tres lecturas para la aplicación (véase §6); no ejecuta ninguna escritura delegada.
- OpenAPI y su prueba de contrato al día.

### 5. CLI

Órdenes nuevas bajo `atenea network`, con `--network` y `--json`: `permissions`, `devices`, `join-code`, `approve <huella>`, `revoke <dispositivo>` y `policy list | add | remove`. Firman con la clave de identidad de la red, así que se ejecutan con los mismos privilegios que `join` (root en Linux y macOS; elevación en Windows). El código de unión se imprime una sola vez y no se guarda en disco.

### 6. Aplicación de escritorio

- Sección **«Administración»** por red, visible solo si el equipo tiene una concesión: inventario, pendientes (aprobar comparando la huella), generar código (se muestra una vez, con copiar y caducidad) y políticas (listar, crear con selector de dispositivos del alcance, borrar).
- **Escrituras:** amplían la lista cerrada de operaciones elevadas (ADR-021, ADR-024) con cuatro: `join-code`, `approve`, `revoke` y `policy add|remove`. Cada una es una sola llamada al CLI fijo con argumentos validados (identificadores, huella, protocolo y puertos), con el diálogo de administrador del sistema o UAC.
- **Lecturas sin diálogo:** el agente, que ya tiene la identidad, consulta `grant`, `devices` y `policies` con su sincronización periódica y los expone en el estado local que la aplicación ya lee. La aplicación no envía nada al agente: el socket de estado sigue admitiendo solo `status`.
- La superficie del servicio vinculado crece; cada método nuevo se enumera y se prueba como los actuales.

### 7. Panel web

En el detalle del dispositivo: permisos concedidos, alcance, quién los concedió y cuándo; conceder, cambiar y retirar (`owner` o `admin`). En políticas y dispositivos, quién los creó o dio de alta. En auditoría, los eventos nuevos.

## Amenazas y límites

| Riesgo | Tratamiento |
| --- | --- |
| Un dispositivo delegado comprometido abre accesos | Solo dentro de su alcance, solo entre dispositivos, sin `any` ni prefijos; con el alcance por defecto no puede nombrar a un dispositivo ajeno. Límites de tasa y de número, y todo queda en la auditoría. No se elimina: quien concede `network` concede la red |
| Dar de alta un equipo hostil con un código delegado | El equipo entra pendiente y solo gana los accesos que el delegado le cree, siempre dentro del alcance del delegado. No hereda permisos |
| Aprobar el dispositivo equivocado | La aprobación exige la huella completa y solo vale para pendientes de un código propio (salvo alcance `network`) |
| La regla inversa de ADR-016 da más acceso del esperado | No cambia: una política delegada tiene la misma regla inversa que cualquier otra, y la interfaz lo dice al crearla |
| Escalada: el delegado se concede permisos o cambia el alcance | No existe ninguna ruta de dispositivo que escriba concesiones; solo sesión de `owner` o `admin` |
| Fuga del código de unión | Un solo uso, caducidad máxima de una hora, tope de códigos vivos; no se guarda en disco ni en registros |
| Más superficie elevada en la aplicación | Cuatro operaciones nuevas, cada una con argumentos de formato cerrado y una prueba de la cadena exacta; las lecturas no elevan |
| Un servidor anterior con una CLI nueva | Las rutas nuevas responden `404`: la CLI lo muestra como «el servidor no admite administración delegada». Se despliega el servidor antes |
| Inventario visible para el delegado | `read:devices` muestra toda la red. Es un permiso que se concede aparte y no incluye secretos |

## Plan por fases

Cada fase es un cambio revisable con CI. No se publica una versión por fase (véase «Cuándo se publica una versión» en [release](../operations/release.md)).

1. **ADR** (este documento).
2. **Control plane, concesiones:** migración `0013`, rutas de administrador, auditoría, OpenAPI.
3. **Control plane, rutas de dispositivo:** permisos, alcance, reglas y límites.
4. **CLI.**
5. **Panel web.**
6. **Agente y aplicación de escritorio:** lecturas en el estado local y las cuatro operaciones elevadas.
7. **Operación:** guía y ensayo de retroceso de la `0013` (hechos); prueba en un despliegue real (pendiente).

## Pruebas de aceptación

- Sin concesión, todas las rutas `/device/admin/` responden `403 grant_missing`.
- Con alcance `enrolled`: el delegado genera un código, un equipo se une, lo aprueba con su huella y crea una política entre ambos; el equipo nuevo aparece como par.
- El mismo delegado recibe `403 out_of_scope` al crear una política que nombra a un dispositivo ajeno, al aprobar un pendiente de otro código y al revocar un dispositivo ajeno.
- Un delegado no puede crear políticas `any` ni por prefijo, ni borrar una política de un administrador.
- Una huella distinta al aprobar se rechaza sin cambiar nada.
- Revocar al delegado borra su concesión y conserva lo que creó.
- Cada acción aparece en la auditoría con el dispositivo autor.
- La aplicación no muestra «Administración» sin concesión; con ella, cancelar el diálogo del sistema no cambia nada.
- Prueba de contrato OpenAPI y de migración (`1..13`) al día; ensayo de retroceso del control plane anterior sobre la `0013`.

## Qué no se hace en esta versión

Etiquetas y grupos; aprobación automática sin huella; una política por defecto de red («todos con todos»); tokens de servicio con escritura; conceder permisos desde la CLI o la aplicación; que un delegado apruebe rutas de subred, cambie DNS, roles o la política de unión; delegación en cadena.
