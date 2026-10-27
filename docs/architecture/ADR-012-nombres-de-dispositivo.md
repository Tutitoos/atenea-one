# ADR-012: Nombres de dispositivo (nombre del sistema, personalizable)

- **Estado:** **Propuesto** (2026-10-27). No hay código: espera la aceptación del responsable.
- **Relacionados:** ADR-005 (contrato y autenticación de dispositivo), ADR-007, ADR-011 (aplicación de escritorio).

## Contexto

La lista de pares de la aplicación de escritorio solo puede enseñar claves abreviadas o IP, porque la configuración firmada (`DeviceConfig`) lleva claves y direcciones pero **no nombres**. El control plane sí guarda un nombre por dispositivo (`devices.name`), pero es texto libre que se teclea a mano al enrolar (`atenea enroll --name`, obligatorio) y **no llega a los agentes**. Se quiere que cada dispositivo aparezca con el **nombre que tiene en su sistema** (p. ej. el «Nombre» de Ajustes del Sistema en macOS) y que ese nombre se pueda **cambiar**.

## Decisión

### 1. Origen del nombre: el del sistema, y un nombre propio que gana

- Nuevo origen del nombre: `devices.name_source` ∈ {`device`, `admin`}.
  - `device`: el nombre es el nombre del sistema que informa el propio agente; se actualiza solo si el usuario renombra su equipo.
  - `admin`: un administrador lo ha fijado desde el panel (`PATCH` de dispositivo, que ya existe); **no se sobrescribe** con informes del agente.
- Columna adicional `devices.hostname` (último nombre del sistema informado) para poder **volver** al nombre del sistema: `PATCH` con `use_system_name: true` pone `name := hostname` y `name_source := 'device'`.
- Migración: los dispositivos existentes pasan a `admin` (su nombre se tecleó a mano y no debe cambiar solo).
- `atenea enroll --name` pasa a ser opcional: sin él se usa el nombre del sistema (`name_source = 'device'`); con él, `admin`.

### 2. Nombre del sistema

macOS: `scutil --get ComputerName` y, si falla, `hostname`. Windows: `hostname`. Linux: `/proc/sys/kernel/hostname`. Se normaliza para mostrarlo (ver §4). Se comparte entre el agente y el CLI con el mismo mecanismo `#[path]` que ya usan para `private_files.rs`, sin crate nuevo.

### 3. Reparto a los pares: metadato de visualización **no firmado**, por un endpoint firmado de dispositivo

- El agente envía su nombre en el informe existente (`POST /api/v1/device/status`, campo opcional `hostname`).
- Nuevo `GET /api/v1/device/peers`, autenticado igual que `GET /api/v1/device/config` (petición firmada por el dispositivo, solo aprobados, mismo límite de tasa). Devuelve `[{"device_id", "name"}]` **solo de los pares que aparecen en la configuración que ese dispositivo recibiría** (misma política): un dispositivo no aprende los nombres de equipos que no puede ver.
- El agente lo pide con cada sincronización, lo guarda en memoria y lo expone en el estado (`peers[].name` y `hostname` propio). **El nombre nunca se usa para identificar, autorizar, resolver DNS ni enrutar**: solo para mostrar.
- Compatibilidad: todos los campos son opcionales; un agente nuevo contra un control plane anterior recibe 404 y sigue sin nombres; un agente anterior no envía ni pide nada. Orden de despliegue: control plane, luego agentes.

### 4. Tratamiento del texto

El nombre es entrada de otro dispositivo: se acota a 64 caracteres, se recortan espacios y se **rechazan** controles (U+0000–U+001F, U+007F) y los caracteres bidireccionales de formato (U+202A–U+202E, U+2066–U+2069) en el control plane (al recibirlo) y de nuevo en el agente (al aceptarlo). La interfaz lo pinta siempre como texto, nunca como HTML.

### 5. Tres niveles de personalización en la interfaz

Prioridad al mostrar un par: **alias local** (almacenamiento del webview de la aplicación, ya existe) > **nombre global** (`name`: el del sistema, o el fijado por un administrador) > IP del túnel > clave abreviada. Cambiar el nombre **para todos** exige autenticarse en el control plane como usuario: se hace en el panel web. La aplicación de escritorio **sigue siendo de solo lectura**; poder renombrarse a sí mismo desde ella exigiría una orden de control del agente y queda fuera.

## Alternativas descartadas

- **Nombre dentro de `DeviceConfig` firmada.** Es lo único que haría el nombre *autenticado* por el emisor, pero cambia el formato firmado (parser, vectores, fuzz, `inspect_config` del coordinator) y choca con la numeración que ya reserva el ADR-010 para L2. Es un coste alto para un dato que no decide nada de seguridad. Si algún día el nombre se usara para DNS o políticas, habría que revisarlo.
- **Descubrimiento local (mDNS).** Fuera de la red de la malla y sin autenticación; ADR-010 ya lo trata como opt-in.
- **Solo alias local.** Es lo que hay hoy: cada equipo tendría que renombrar a todos y los nombres no coincidirían entre equipos.

## Riesgos y límites

- **Un control plane comprometido puede mentir sobre nombres** (no sobre claves ni direcciones, que van firmadas). Efecto: suplantación visual en la lista, p. ej. un nombre engañoso. Se mitiga mostrando también la IP del túnel y la clave en el detalle, no por criptografía.
- **Un dispositivo aprobado elige su propio nombre** y puede poner uno confuso; el administrador lo corrige con `admin`.
- La aplicación y el panel muestran el nombre «tal cual» tras la normalización: no se garantiza unicidad.
- Cambia la API del control plane (`DeviceReportInput`, nuevo endpoint, `PATCH`) y por tanto el OpenAPI y `packages/api-client`; hace falta una migración SQL (`0006`).
- Sin implementar y sin medir nada todavía. No se despliega nada sin autorización explícita (control plane del piloto y agentes) y los agentes del soak no se tocan hasta que termine.
