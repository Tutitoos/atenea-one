# Fase 3 — resultados del laboratorio (MVP-0)

- **Fecha:** 2026-10-24
- **Entorno:** el de la fase 2 (contenedor Linux aarch64 privilegiado, namespaces `a`, `b`, `c` en un puente) más un "emisor" que es un bucle de reemisión de configuraciones firmadas. Raíz, emisor y dispositivos se generan en cada ejecución con `atenea root-init`, `issuer-init` y `device-init`.
- **Parámetros de laboratorio:** validez de cada configuración 40 s, reemisión cada 10 s, tolerancia 30 s (ADR-002 propone 300 s / 60 s / 30 s para producción; los valores son parámetros).
- **Reproducir:** `tests/network-lab/run.sh` (salida distinta de cero si falla algún caso).

## Resultados

| Caso | Resultado |
| --- | --- |
| Agentes arrancan con configuración firmada; estado persistente 0600 | correcto |
| Todo el laboratorio de la fase 2 (IPv4/IPv6, TCP/UDP, MTU, sin payload en claro, replay/alteración/sobredimensionado, peer no autorizado, pérdida y reordenación, reinicio, parada limpia) con la autorización procedente de la configuración firmada | correcto |
| ACL: UDP a un puerto fuera de 9000-9100 | descartado en origen (`tx_acl_denied`); TCP a ese puerto no conecta; ICMP sigue permitido |
| Suplantación: paquete con IP de origen que no es del dispositivo | descartado (`tx_bad_source`) |
| Configuración con firma alterada | rechazada: `bad_config_signature` |
| Configuración de un emisor de otra raíz | rechazada: `unknown_root` |
| Configuración caducada | rechazada: `expired` |
| Versión anterior (rollback) aún dentro de ventana | rechazada: `config_rollback` |
| Fichero basura | rechazado (`too_large`, porque la longitud de cuerpo aleatoria excede el límite antes de comprobar la versión: orden de los pasos del contrato) |
| Tras los 5 rechazos | el servicio con la configuración vigente no se interrumpió y una emisión legítima posterior limpia el error |
| Reinicio de un agente | sesión nueva, versión de configuración no retrocede |
| Revocación por peers: el emisor reemite la configuración de B sin A | B deja de aceptar a A en **3 s** (recarga ≤ 1 s + sondeo) |
| Revocación por lease: el emisor deja de renovar a A | A se corta a sí mismo a los **70 s** de su última configuración (validez 40 s + tolerancia 30 s), con `denied_no_lease`; antes de caducar sigue activo |
| Recuperación | tras reemitir, A y B vuelven a comunicarse |

## Qué no demuestra

- "Sin conectividad al emisor" se simula dejando de reemitir; no hay un servicio de emisión ni de red entre ambos.
- El ACL de entrada (el del destino) se prueba con pruebas unitarias del motor, no en el laboratorio; el contador `rx_acl_denied` quedó a 0 en la red porque el origen aplica antes el suyo.
- La anomalía de reloj, el fallo de persistencia del estado y la corrupción del fichero de estado se prueban en pruebas unitarias (`atenea-identity`, `apps/agent/src/state.rs`), no en el laboratorio; no se manipuló el reloj del contenedor.
- El estado persistente solo se protege contra corrupción estructural, no contra manipulación por root local (ver contrato §8).
- Las ACL son sin estado: las respuestas necesitan sus propias reglas.
- La revocación inmediata no existe: el corte máximo medido para un agente que deja de renovarse es validez + tolerancia.
- Sigue pendiente la revisión criptográfica externa del protocolo y del contrato.

## Fuzzing guiado por cobertura (cargo-fuzz, nightly, en Docker)

`fuzz/run.sh 60`: 9 objetivos, 60 s cada uno, sin hallazgos. Este es un humo de un minuto, no fuzzing continuado (fase 11).

| Objetivo | Ejecuciones | Ejec/s |
| --- | --- | --- |
| `datagram_parse` | 30 503 070 | 500 050 |
| `replay_window` (contra modelo exacto) | 116 780 | 1 914 |
| `handshake_init` | 176 825 | 2 898 |
| `session_decrypt` | 11 015 | 180 |
| `hex_decode` | 5 849 338 | 95 890 |
| `signed_config_verify` (con semillas de los vectores) | 154 266 | 2 528 |
| `device_config_parse` (re-codificación idéntica) | 4 297 908 | 70 457 |
| `cert_bundle_state_parse` | 796 543 | 13 058 |
| `ip_classify` (ACL) | 12 385 983 | 203 048 |

Limitación: `handshake_init` no tiene una semilla de mensaje 1 válido (necesita efímeras aleatorias) y apenas llega a las ramas posteriores al descifrado. Durante la creación del objetivo `replay_window` se corrigió un fallo del propio objetivo (bucle sin fin con `arbitrary` agotado), no del código del proyecto.

Pruebas unitarias en Linux: 68 (identity 10, policies 11, crypto 16, agente 24, protocol 6, tunnel 1).
