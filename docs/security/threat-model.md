# Modelo de amenazas (MVP-0 y base del MVP)

- **Estado:** Aceptado (2026-10-24, por el responsable del proyecto). Punto de partida, no garantía.
- **Fecha:** 2026-10-24
- **Relacionados:** ADR-001…005, [configuración firmada v1](../protocol/signed-config-v1.md)

## Activos

Claves privadas de dispositivo; clave raíz y de emisor; configuraciones firmadas (autorización); confidencialidad e integridad del tráfico del túnel; disponibilidad del túnel; metadatos (quién habla con quién, cuándo, cuánto).

## Límites de confianza

```mermaid
flowchart LR
  Admin[Estación de administración · clave raíz] -->|certificado de emisor| Emisor[Emisor online · clave de emisor]
  Emisor -->|configuración firmada| Agente[Agente · claves del dispositivo]
  Agente <-->|Noise IK sobre UDP/TCP| Peer[Agente peer]
  Agente -. datagramas cifrados .-> Relay[Relay · no confiable]
  Relay -.-> Peer
```

El relay, la red y el coordinator se tratan como **no confiables para confidencialidad e integridad del tráfico**. La raíz y el emisor son **confiables para autorización**: su compromiso se acota en ADR-005, no se evita con E2E.

## Amenazas

| # | Amenaza | Mitigación prevista | Prueba / fase | Residual |
| --- | --- | --- | --- | --- |
| T1 | Cliente hostil (dispositivo autorizado) intenta suplantar a otro o salir de su ACL | Validación de IP origen por peer, ACL en origen y destino, denegación por defecto (contrato §3) | Fase 3: paquete con origen falso descartado | Un dispositivo autorizado puede abusar de lo que su ACL permite |
| T2 | Relay hostil lee, altera, reordena, repite o descarta paquetes | E2E con Noise IK, antirreplay y AEAD (ADR-001/003) | Fase 5: captura en el relay sin payload en claro; alteración y replay rechazados | El relay ve IPs, tamaños y tiempos; puede descartar o retrasar (denegación) |
| T3 | Coordinator comprometido anuncia endpoints falsos, desvía tráfico o espía presencia | Los endpoints son pistas no autenticadas; solo un handshake Noise IK con la clave de la configuración firmada crea una ruta. El coordinator no recibe claves; verifica la configuración firmada de cada dispositivo (sin estado) y autoriza `rendezvous` solo si es mutuo | Fase 4: pruebas del coordinator; el E2E de NAT con agentes reales | Puede degradar conectividad (denegación), observar quién anuncia y cuándo, y hacer correlación de metadatos (T15) |
| T4 | Cuenta de administración robada | MFA en el proveedor OIDC (fase 3B); auditoría; la raíz no está en el host expuesto (ADR-005) | Fase 3B | Un administrador robado autoriza dispositivos hasta que se detecte; el límite es el lease |
| T5 | Servidor de control (emisor) comprometido | Certificado de emisor acotado en tiempo y serial; recuperación por la raíz (ADR-005) | Fase 3: emisor revocado por serial; fase 9: ensayo | Hasta la caducidad del certificado puede autorizar identidades maliciosas |
| T6 | Raíz comprometida o perdida | Custodia fuera de línea; dos anclas para rotar (ADR-005) | Fase 9: ensayo de reincorporación | Sin recuperación automática: reincorporar todos los dispositivos |
| T7 | Spoofing de handshake o de endpoint | Noise IK con claves conocidas; prólogo con red y versión; frescura (ADR-001); actualización de endpoint solo tras autenticar | Fase 2 | — |
| T8 | Replay y reordenación | Contador explícito, ventana de 2048 y actualización tras autenticar; sesión efímera (ADR-003) | Fase 2: propiedades y fuzzing | Código propio: revisión externa |
| T9 | Amplificación y reflexión por UDP | El respondedor solo responde a mensajes 1 válidos y autorizados; respuesta ≤ tamaño de entrada; sin respuestas a desconocidos (ADR-001). STUN propio: respuesta ≤ 2,2× la petición (44 frente a 20 bytes), límite por IP de origen compartido entre sus dos puertos y sin respuesta a lo no válido (fase 4) | Fase 4: pruebas del coordinator con sockets UDP reales (basura, límite, factor) | STUN, por diseño, responde a cualquiera: se limita por tasa; factor residual ≤ 2,2 |
| T10 | Agotamiento de CPU/memoria (inundación de mensajes 1, conexiones TCP, ClientHello lento) | Límites de tasa, tablas acotadas, plazos, cierre ante exceso (ADR-001/004) | Fases 2, 5 y 11: pruebas de abuso y carga | Un atacante con recursos suficientes puede saturar el enlace |
| T11 | Host local multiusuario: otro usuario lee claves o controla el agente | Estado y claves 0600 con usuario de servicio; IPC por socket con permisos; CLI sin privilegios de red | Fase 2/7 | Un root local lo controla todo |
| T12 | Rollback de configuración o de reloj para revivir acceso | Estado persistente de versión, serial y hora máxima; fallo cerrado ante anomalía (ADR-002, contrato §3) | Fase 3 | Un atacante con acceso al disco del agente puede borrar su estado |
| T13 | Fuga de secretos al repositorio público | `.gitignore`, escaneo de secretos de GitHub, revisión; sin inventarios ni claves | Fase 1 y CI | Falta herramienta de escaneo local propia |
| T14 | Clonación de VM o snapshot reutiliza estado de sesión (nonces) | Sesiones de vida corta y solo en memoria (ADR-003) | Fase 2: documentado, sin prueba | Riesgo aceptado; revisar con el auditor |
| T15 | Correlación de tráfico por un observador de red o del relay | No se pretende ocultarla; documentado | — | Sin protección en v1: ni relleno ni mezcla |
| T16 | Coordinator hostil dirige sondeos de un agente hacia una víctima | Solo para pares de la propia configuración firmada; ≤ 8 candidatos saneados (sin loopback, enlace local ni difusión), mensaje 1 de 118 bytes cada ~1 s durante 10 s y retardo creciente tras fallar (30 s hasta 5 min) | Fase 4: pruebas unitarias de saneado, acotación y retardo | Tráfico acotado hacia terceros; el coordinator comprometido puede hacerlo |
| T17 | Repetición de un mensaje 1 desde otra IP para redirigir el tráfico del respondedor | El endpoint solo se confirma con un paquete autenticado de la sesión; una repetición recibe la respuesta guardada sin cambiar estado | Fase 4: `an_init_from_another_address_never_redirects_the_confirmed_endpoint` | Una respuesta de 62 bytes a la IP del atacante (menor que la petición de 118) |

## Fuera de alcance

Ataques físicos al dispositivo; compromiso del núcleo del SO; canales laterales de hardware; criptografía postcuántica (sin PSK en v1); anonimato frente al relay; inspección TLS corporativa que impida el fallback TCP 443.

## Supuestos

- NTP razonable en los dispositivos (ADR-002); un reloj fuera de tolerancia deniega.
- Las claves privadas de dispositivo no salen del dispositivo.
- El relay y el coordinator no reciben claves de peers distintas de las públicas.
- No hay auditoría independiente del protocolo: este modelo es un punto de partida, no una garantía (fase 11).
