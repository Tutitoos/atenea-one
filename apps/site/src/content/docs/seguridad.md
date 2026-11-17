---
title: Seguridad
description: Resumen del modelo de amenazas de Atenea One, en qué se confía y en qué no, y cómo reportar una vulnerabilidad.
---

Este es un resumen. El modelo completo, con cada amenaza, su mitigación y su riesgo residual, está en `docs/security/threat-model.md` del repositorio. Es un punto de partida, **no una garantía**: el protocolo no tiene auditoría criptográfica independiente.

## Límites de confianza

| Componente | Confianza |
| --- | --- |
| Relay, coordinator y red | **No** son de confianza para la confidencialidad ni la integridad del tráfico |
| Clave raíz y emisor | De confianza para la **autorización**; su compromiso se acota por diseño |
| Agente y claves del dispositivo | Las claves privadas no salen del dispositivo |

La raíz se custodia fuera de línea y solo certifica emisores. Un emisor tiene certificado acotado en tiempo y serial, y puede revocarse.

## Qué no se pretende cubrir

- Ataques físicos al dispositivo, compromiso del núcleo del sistema o canales laterales de hardware.
- Criptografía postcuántica.
- Anonimato frente al relay: ve direcciones IP, tamaños y tiempos de los paquetes. No hay relleno ni mezcla.
- Redes con inspección TLS corporativa que impidan el respaldo por TCP 443.

## Riesgos aceptados y documentados

- **Regla inversa sin puertos.** Una política A→B también permite a B iniciar tráfico hacia A. Es una decisión aceptada, visible en el panel y en la documentación.
- **Roaming sin validación de retorno.** Un paquete autenticado puede cambiar el endpoint de origen de una sesión, como en WireGuard.
- **Cuenta de administración robada.** Se mitiga con MFA en el proveedor OIDC y con auditoría; el límite es la vigencia del lease.

## Cadena de suministro

Las acciones de CI se fijan por SHA, las publicaciones solo salen de tags protegidos con procedencia atestada, y se ejecutan `cargo deny`, `pnpm audit --prod`, `govulncheck` y Dependabot. Los releases llevan un manifiesto firmado y un SBOM por objetivo.

## Reportar una vulnerabilidad

Usa **Security → Report a vulnerability** en el repositorio de GitHub, que abre un aviso privado. No publiques detalles explotables en una issue ni en una PR. No se promete un plazo de respuesta.
