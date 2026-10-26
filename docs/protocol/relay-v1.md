# Relay v1 — sustituido

El responsable aprobó el 2026-10-25 el cutover limpio a [relay-v2](relay-v2.md), junto con [wire-v2](wire-v2.md). Este documento es únicamente una nota de migración, no un contrato compatible.

- Se retiran cabecera sin versión, token de 173 bytes sin audiencia, dominios de firma `/v1` y ALPN `atenea-relay/1`.
- Relay v2 exige versión explícita, `relay_id` de 16 bytes en token y prueba ALLOC, token de 189 bytes, concesiones individuales por relay y ALPN `atenea-relay/2`.
- Wire v2 añade MAC1/MAC2 a INIT y respuesta de cookie, y cambia el prólogo Noise.
- Datagramas v1 se descartan sin responder; relay/TCP informa incompatibilidad mediante ALPN o error v2 y cierre. No hay emulación, negociación hacia v1, alias ni shims.
- El contrato nuevo está pendiente de implementación y revisión; las verificaciones históricas de v1 no verifican v2. No hay despliegues que conservar.
