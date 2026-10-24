# Fase 2 — resultados del laboratorio

- **Fecha:** 2026-10-24
- **Entorno:** contenedor Linux aarch64 privilegiado (Docker Desktop, Debian 12, Rust 1.99.0), namespaces de red `a`, `b` y `c` unidos por un puente. Sin NAT, IPv4 en la red subyacente.
- **Reproducir:** `tests/network-lab/run.sh` (compila, ejecuta las pruebas unitarias y los casos de red; código de salida distinto de cero si falla alguno).

## Resultados (última ejecución)

| Caso | Resultado |
| --- | --- |
| Pruebas unitarias en Linux (protocolo, crypto, agente, túnel) | 40 pruebas, todas correctas |
| Ping IPv4 e IPv6 en ambos sentidos | correcto |
| TCP 5 MB con hash SHA-256 coincidente, IPv4 e IPv6 | correcto (≈ 500 Mbit/s IPv4, ≈ 1 Gbit/s IPv6, orientativo: un solo flujo, sin NAT, en un contenedor) |
| UDP 300/300 datagramas, IPv4 e IPv6 | correcto |
| MTU 1400 con DF: 1400 pasa y 1401 se rechaza, IPv4 e IPv6 | correcto |
| Captura externa sin payload en claro | el marcador (2700 bytes enviados por el túnel) no aparece; el control fuera del túnel sí (valida el método) |
| Replay de un datagrama capturado | rechazado (`rx_replay`) |
| Contador alterado (etiqueta AEAD inválida) | rechazado (`rx_auth_fail`) |
| Datagrama de 2000 bytes | descartado (`rx_oversize`) |
| Endpoint del peer tras paquetes no autenticados | sin cambios |
| Peer no autorizado (C) | handshake descartado sin respuesta (`rx_init_unknown_peer`); sin conectividad |
| Pérdida 8 %, 20 ms ±10, reordenación 25 % | TCP 2 MB íntegro; UDP 463/500 (la pérdida de la red es la esperada) |
| Reinicio del agente A | sesión nueva en B (handshake adicional) y conectividad recuperada |
| Parada del agente | interfaz, rutas y socket IPC eliminados |

## Qué no demuestra

- No hay NAT, doble NAT, hole punching ni relays (fases 4 y 5).
- No se midió el MTU de caminos reales ni el rendimiento fuera de un contenedor; las cifras son orientativas.
- "Sin reutilizar nonces tras reiniciar" se prueba a nivel de sesión nueva (índice y claves nuevas, handshake adicional y prueba unitaria de que dos sesiones con las mismas claves estáticas cifran distinto); no hay verificación formal.
- La captura sin payload en claro es una búsqueda de un marcador en la captura, no un análisis criptográfico.
- Fuzzing: el parser de datagramas se prueba con entradas aleatorias (proptest). **Falta un fuzzer guiado por cobertura** (`cargo-fuzz`, requiere nightly), exigido por ADR-003.
- La puerta de revisión criptográfica externa (antes de la fase 3) sigue pendiente.
- `snow` arrastra `ring` entre sus dependencias según la resolución actual; se revisará su licencia y presencia en el inventario de dependencias.
