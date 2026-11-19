<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-028: Cuentas propias, redes por cuenta e invitaciones

- **Estado:** **Aceptado** por el responsable el 2026-11-18, con las nueve decisiones D1 a D9 tal como se propusieron. Bases elegidas: raíz bajo custodia del usuario, Dex se mantiene, registro propio abierto con límites, organización personal automática e invitado que genera códigos y aprueba. **Implementado en `main`, sin publicar ni desplegar:** control plane (fase 2: migraciones `0015` y `0016`, cuentas locales, organización personal, `GET /api/v1/issuer`, miembros por red, invitaciones, cuotas), panel (fase 3: registro, entrada y recuperación, códigos, asistente de red, Compartir, «Compartidas conmigo», aprobar como `enroller`) y `atenea issuer-cert --issuer-pub`. Guía: [cuentas y redes compartidas](../operations/cuentas-y-redes-compartidas.md). **Fase 4 (CLI): decidido no hacerla** (2026-11-18, opción A): invitar y gestionar miembros son acciones de la cuenta propietaria y el CLI solo tiene la identidad del dispositivo; una credencial de cuenta para el CLI exigiría su propio ADR. Solo se añadió `--issuer-pub`. **Pendiente:** GitHub en Dex, y prueba con cuentas reales en el piloto. Un miembro puede abandonar una red (`DELETE …/membership`, botón en «Compartidas conmigo»).
- **Relacionados:** [ADR-005](ADR-005-custodia-clave-raiz.md) (la raíz no vive en el servidor; se mantiene), [ADR-015](ADR-015-bloque-b-miembros-y-renombrado.md) (miembros por correo), [ADR-020](ADR-020-token-de-servicio-de-solo-lectura.md), [ADR-026](ADR-026-administracion-delegada-por-dispositivo.md) y [ADR-027](ADR-027-concesion-delegada-de-permisos.md) (permisos por dispositivo).

## Contexto

Hechos comprobados en el código de la 0.1.34:

- **El inicio de sesión es solo OIDC** (`auth.rs`), y en el piloto lo da Dex. No hay contraseñas locales ni registro: `users` se identifica por `(oidc_issuer, oidc_subject)`.
- **Las redes cuelgan de una organización** (`networks.org_id`) y el acceso se decide por la pertenencia a ella (`orgs::role`, `networks::access`), con roles `owner`, `admin` y `member`.
- **Crear una red exige aportar una ancla raíz** generada fuera del servidor y subir después un certificado de emisor firmado con esa raíz. El servidor tiene **una sola clave de emisor**; rechaza con `400 wrong_issuer` un certificado de otra clave.
- **La clave pública del emisor del servidor no se expone** hoy por API, por lo que un usuario no puede firmar el certificado sin pedírsela al operador.
- **Añadir un miembro por correo no envía nada** y solo encuentra a quien ya inició sesión. El correo se guarda solo si el proveedor declara `email_verified: true`.
- **No hay SMTP** en el piloto.

Petición del responsable: un servicio con cuentas propias, como ZeroTier, donde cada cuenta crea redes de las que es propietaria y puede invitar a otras cuentas a verlas y a añadir dispositivos.

## Decisión propuesta

### D1. La raíz queda bajo custodia del usuario

El servidor **nunca** genera ni guarda una clave raíz. Cada red sigue anclada a una raíz que el usuario genera y custodia. Las redes actuales del piloto ya funcionan así, así que no se migra nada.

La creación de una red pasa a tres pasos guiados desde el panel, sin cambiar ningún formato firmado:

1. El usuario genera su raíz con `atenea root-init` (herramienta existente). Solo su clave pública sale de su equipo.
2. El panel crea la red con esa ancla y muestra la clave pública del emisor del servidor, que ahora se expone en `GET /api/v1/issuer` (ruta nueva, de lectura, con sesión).
3. El usuario firma el certificado de emisor con su raíz y lo sube; el panel muestra cuánto falta para que caduque.

Generar la raíz en el navegador (WebCrypto Ed25519) queda como **opción a verificar** antes de implementarla: hace falta comprobar el soporte de los navegadores objetivo, la verificación estricta y cómo se protege el fichero exportado. No se inventa ninguna primitiva.

**Consecuencia que el usuario debe conocer, y que el panel dirá:** si pierde su raíz, **no hay recuperación** (ADR-005). El certificado de emisor caduca y el usuario tiene que volver a firmarlo; sin la raíz, la red deja de emitir configuraciones al caducar y hay que reincorporar cada dispositivo con una raíz nueva. Es el coste de que el servidor no pueda tocar la raíz.

### D2. Dex se mantiene; la contraseña es una identidad local

- **GitHub** se añade como conector de Dex. No cambia el flujo OIDC actual.
- **Email y contraseña** los gestiona el control plane, sin pasar por Dex. Una cuenta local es una fila de `users` con `oidc_issuer = 'local'` y una tabla `local_credentials` aparte (hash con argon2id y parámetros fijados en el ADR de implementación). No se guarda ningún secreto en claro ni se registra en logs.
- Las sesiones, el CSRF y los límites son los de siempre. Una sola sesión por navegador.

### D3. Sin SMTP, sin verificación de correo

- **El correo de una cuenta local no está verificado** y por eso **no se guarda en `users.email`** (sigue la regla actual de que solo se guarda el verificado). Consecuencia: «añadir por correo» (ADR-015) no encuentra cuentas locales; se invita por enlace (D7).
- **Recuperación:** al registrarse se muestran **diez códigos de recuperación de un solo uso**, una única vez, guardados como hash. Quien los pierde y olvida la contraseña pierde la cuenta; el operador puede restablecerla desde la línea de órdenes del servidor, con auditoría. No hay «olvidé mi contraseña» por correo hasta que exista SMTP.
- Cuando haya SMTP se añadirán verificación y recuperación por correo en un ADR propio. Esta decisión no lo impide.

### D4. Registro abierto con límites

Servicio abierto sin verificación, así que los límites son la defensa:

- Registros por IP (agrupando IPv6 por /64) y globales por hora, con `429 rate_limited`.
- Intentos de inicio de sesión por cuenta y por IP, con espera creciente; contraseña mínima de 12 caracteres.
- Cuotas por cuenta, configurables y con tope duro: **3 redes**, **64 dispositivos por red**, **10 invitaciones vigentes por red**.
- Interruptor del operador `registro = abierto | cerrado`, para cerrarlo sin desplegar.
- Los límites del relay y del coordinator por red (ADR-018) siguen aplicando: una cuenta nueva no obtiene más recursos que una antigua.

Sigue sin haber prueba de persona (captcha o similar); se anota como riesgo residual.

### D5. Organización personal automática

Al crear una cuenta (local u OIDC) se crea una organización personal de la que es `owner`. Las redes que cree cuelgan de ella. **No cambia el esquema de organizaciones ni de redes.** La organización personal no admite añadir miembros (sería saltarse D6): las invitaciones van por red.

### D6. Invitar a una red, no a la organización

Añadir a alguien a la organización le daría todas sus redes. Por eso se introduce la pertenencia **por red**:

- Tabla `network_members(network_id, user_id, role, invited_by, created_at)` con `role in ('viewer','enroller')`. `networks::access` pasa a aceptar la pertenencia de organización **o** la de red.
- **`viewer`:** lee la red, sus dispositivos, rutas, DNS y estado. Nada más.
- **`enroller`:** lo de `viewer`, más **crear códigos de unión y aprobar dispositivos**. No crea ni borra políticas, no revoca, no cambia DNS, rutas, política de unión ni certificado.
- **Aprobar exige la huella completa**, como en ADR-026: quien aprueba la escribe, no basta pulsar. Un dispositivo aprobado sigue sin acceso hasta que un `owner` o `admin` cree políticas (deny por defecto, ADR-016).
- Solo el propietario de la red (`owner` de su organización) invita, cambia el rol o expulsa. Un `enroller` no invita.
- Expulsar a un miembro **no** revoca los dispositivos que aprobó: se muestran con «aprobado por», para que el propietario decida.
- Todo queda auditado con el actor, y el `owner` ve quién generó cada código y quién aprobó cada dispositivo.

### D7. Invitación por enlace

Sin correo, la invitación es un **enlace de un solo uso**: token de 256 bits, guardado como hash, que caduca a los 7 días y lleva el rol. El propietario lo copia y lo entrega por el canal que quiera. Quien lo abre inicia sesión o se registra y lo acepta con un clic. Si ya es miembro, falla sin cambiar nada. El propietario puede anularlo mientras no se use. El token no se registra ni se devuelve en listados.

### D8. Compatibilidad

- Las organizaciones, los roles y los inicios de sesión OIDC existentes no cambian. Los usuarios de Dex siguen igual y reciben su organización personal al entrar de nuevo, sin tocar las que ya tienen.
- Las cinco redes del piloto no cambian. El token de servicio (ADR-020) sigue siendo de solo lectura por organización.
- El control plane 0.1.34 ignora la tabla nueva: un retroceso deja las invitaciones y miembros de red sin efecto pero no rompe las redes. Se ensaya como `rollback-0013` y `rollback-0014`.
- Esto es una versión de superficie nueva: contrato OpenAPI, panel y, para firmar el certificado de emisor, `atenea issuer-cert --issuer-pub`. No hay comandos de CLI para invitar ni para listar miembros (ver Fases).

### D9. Qué no se decide aquí

SMTP y correo; verificación de identidad más allá de GitHub; facturación o planes; cuotas por organización; redes compartidas entre organizaciones; recuperación de la raíz perdida; generar la raíz en el navegador (a verificar, D1); ver la red desde la aplicación de escritorio como invitado.

## Alternativas descartadas

| Opción | Motivo |
| --- | --- |
| Raíz por red en el servidor, como ZeroTier | Contradice ADR-005: un servidor comprometido comprometería todas las redes. Elegido B. |
| Sustituir Dex por Keycloak | Mucha operación para el mismo resultado. Dex con GitHub y registro local basta. |
| Dex con contraseñas | Dex no ofrece registro propio; habría que sustituirlo o parchearlo. |
| Invitar a la organización | Da acceso a todas sus redes; la pertenencia por red es más estrecha. |
| Verificar por correo ya | No hay SMTP; se pospone a su propio ADR. |

## Amenazas y riesgos residuales

- **Registro abierto sin verificación:** abuso de recursos y cuentas desechables. Mitigado por los límites de D4, no eliminado.
- **Enlace de invitación filtrado:** quien lo abra primero entra con el rol. Se mitiga con caducidad corta, un solo uso y anulación, y porque `enroller` no crea políticas ni accede a dispositivos.
- **Un `enroller` malicioso aprueba un dispositivo ajeno:** no obtiene acceso sin una política (deny por defecto), y la aprobación queda auditada con su identidad; exige la huella completa.
- **Raíz perdida o robada:** sin recuperación. Es un riesgo del usuario, no del servidor, y el panel lo avisa al crear la red.
- **Contraseñas débiles o filtradas:** sin MFA hasta que lo dé el proveedor; recomendar GitHub o el proveedor de Dex con MFA.
- **Clave de emisor única:** un servidor comprometido puede emitir para todas las redes hasta que caduquen sus certificados; el radio lo acota la validez del certificado (ADR-005). Esto ya era así.

## Fases

1. Aceptar este ADR.
2. Control plane: migraciones (`local_credentials`, `network_members`, invitaciones, organización personal), registro e inicio de sesión locales, GitHub, `GET /api/v1/issuer`, miembros e invitaciones, cuotas y límites; OpenAPI y pruebas con PostgreSQL real.
3. Panel: registro, organización personal, flujo de creación de red (D1), miembros e invitaciones, estado de caducidad del emisor.
4. CLI: solo `atenea issuer-cert --issuer-pub` (firma con la pública del servidor). **Sin `invitar` ni `miembros`:** el CLI no tiene sesión de cuenta (solo identidad de dispositivo) y una credencial de cuenta para el CLI, o delegar en un permiso de dispositivo, exigiría un ADR propio. Se gestiona desde el panel.
5. Guía de operación, retroceso, y prueba con cuentas reales en el piloto.
6. Una versión solo si el responsable la pide.

## Sin comprobar

Comprobado con un navegador real contra un servidor local: registro, códigos, creación de red con raíz propia, firma con `--issuer-pub`, invitación, aceptación y permisos del `enroller`. **No comprobado:** Dex y GitHub, dos navegadores a la vez, un servidor de producción, la carga con el registro abierto y la raíz generada en el navegador (no se implementó: se mantiene el CLI, D1). Los parámetros de argon2id son los de la biblioteca (19 MiB, 2 pasadas, 1 hilo) y no se midieron en el hardware del piloto.
