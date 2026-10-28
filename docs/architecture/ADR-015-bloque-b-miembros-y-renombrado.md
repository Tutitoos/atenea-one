<!-- Copyright 2026 Tutitoos -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-015: Bloque B del panel: miembros, renombrado e información del servidor

- **Estado:** Propuesto. Implementación solicitada expresamente el 2026-10-28; pendiente de aceptación formal del ADR.
- **Relacionados:** ADR-012, ADR-013 y ADR-014.

## Decisión

Las operaciones del panel usan la sesión existente y CSRF para escrituras. Solo un owner añade miembros por correo y renombra la organización; owner/admin renombra redes. Un usuario ajeno recibe 404 `not_found`, sin revelar la organización.

Todos los miembros ven `display_name`; solo owner/admin ven `email`, y para member siempre es null. Ambos son metadatos de presentación del usuario que ya inició sesión mediante OIDC, nunca identidad ni criterio de autorización. Añadir por correo compara el correo completo sin distinguir mayúsculas, no acepta coincidencias parciales ni crea usuarios. Ausencia: 404 `not_found`. Si varias identidades tienen el mismo correo, no se elige una arbitrariamente: 409 `conflict`.

Se reutiliza el cubo de tokens existente por sesión y operación: añadir por correo permite una ráfaga de 10 y repone 10 solicitudes/minuto; información del servidor permite 60/minuto. Se limita antes de buscar correos, incluidos fallos. Riesgo residual: un owner autorizado puede averiguar qué correos han iniciado sesión; repartir las consultas entre sesiones permite más consultas y reiniciar el proceso repone los cubos. No se ofrece un buscador de usuarios ni una lista global.

PUT y POST comparten las validaciones, protección del último owner y auditoría `member.set`. Su detalle gana `previous_role` (string o null) junto a `role`; se conserva el nombre histórico de la acción para el panel y el historial. DELETE conserva su auditoría actual.

Renombrar reutiliza las reglas de creación y las restricciones de unicidad existentes. `org.rename` y `network.rename` registran nombre anterior y nuevo. Antes de renombrar una red se comprueba que su nombre y la URL pública caben en el código de unión. Un código ya emitido conserva el nombre antiguo, solo informativo: no se invalida ni cambia la identidad, raíz o autorización de la red.

La caducidad del emisor se calcula del certificado almacenado, sin devolverlo: null cuando no hay certificado. `GET /api/v1/server` requiere sesión y al menos una membresía, y expone únicamente versión/commit del control plane y hora. No se inventan versiones de coordinator o relay: no existe un canal real para conocerlas. La autenticación por dispositivo de `/api/v1/server/info` no cambia.

## Alternativas

- Exponer correos a todos los miembros: descartado por privacidad y falta de necesidad administrativa.
- Añadir por identificador únicamente: conserva privacidad pero impide el flujo por correo solicitado; el PUT existente sigue disponible.
- Invitaciones o altas de usuarios por correo: fuera del contrato; exigirían otro ciclo de vida y entrega de correo.
- Ocultar siempre si existe el correo con una respuesta de éxito: sería engañoso y no permitiría confirmar el alta real.
- Limitar por IP: penaliza sesiones distintas detrás de NAT; se elige la sesión autenticada y la comprobación de rol antes de consultar usuarios.
- Invalidar códigos al renombrar: innecesario; el nombre es informativo, no una credencial.

## Límites

Sin migración de datos ni cambios de protocolo firmado, CLI, agente o escritorio. No se modifica el laboratorio de redes ni se despliega nada. La autorización y los límites se comprueban con PostgreSQL real; las mediciones de carga y el flujo OIDC desplegado quedan fuera de esta decisión.
