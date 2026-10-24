# Fase 4 — conexión directa tras NAT (E2E)

- **Fecha:** 2026-10-24
- **Entorno:** contenedor Linux privilegiado (Docker Desktop, kernel linuxkit, nftables). Topologías en namespaces (`tests/network-lab/nat/lib.sh`); el coordinator y su STUN corren en el namespace `inet` (`198.51.100.10`, `2001:db8:100::10`); dos agentes reales con TUN en `a` y `b`. Las configuraciones firmadas **no llevan endpoints**: la ruta directa solo puede salir de STUN propio + coordinator + hole punching. Validez de configuración 240 s, reemisión cada 20 s.
- **Reproducir:** `tests/network-lab/nat/run-e2e.sh` (el comportamiento de los NAT, sin Atenea, se verifica aparte con `tests/network-lab/nat/run.sh`).

## Resultados

| Topología | NAT visto por STUN (a / b) | Resultado | Endpoint confirmado de A |
| --- | --- | --- | --- |
| flat (sin NAT) | independiente / independiente | **direct**, TCP 2 MB íntegro | `198.51.100.12:51820` (pública) |
| home (dos NAT domésticos) | independiente / independiente | **direct**, TCP íntegro | `198.51.100.2:51820` (reflexivo de B) |
| double (CGNAT + NAT) | independiente / independiente | **direct**, TCP íntegro | reflexivos de ambos |
| symmetric (mapeo dependiente del destino en A) | **dependiente** / independiente | **sin P2P**: no hay túnel, `path = probing`, `probe_failed ≥ 1` | — (a la espera del relay de la fase 5) |
| misma LAN, sin hairpin | independiente / independiente | **direct** por la **dirección local** (192.168.1.x), sin pasar por el NAT | `192.168.1.11:51820` |
| misma LAN, con hairpin | independiente / independiente | **direct** por la dirección local | `192.168.1.11:51820` |
| IPv6 con cortafuegos con estado (sin NAT) | — | **direct** por IPv6, TCP íntegro | `[2001:db8:b::10]:51820` |
| Cambio de red de A (home → café, otro NAT) | — | se recupera: endpoint de A visto por B `198.51.100.1:51820 → 198.51.100.3:51820` en **47 s** | ver abajo |
| Presencia con TTL | — | 2 presencias activas; **0** tras parar los agentes (≤ TTL 60 s + barrido) | — |

## Qué demuestra

- Para cada caso el estado del agente informa cuándo hay `direct` y cuándo no (`path`, `endpoint`, `nat.mapping`, `nat.reflexive`).
- El agente detecta el **mapeo dependiente del destino** (NAT simétrico) comparando las respuestas de STUN de los puertos 3478 y 3479 desde el socket de datos, y no finge una ruta: tras 10 s de sondeo falla con `probe_failed` y retarda el siguiente intento (30 s, creciente hasta 5 min).
- La ruta solo existe tras un paquete Noise **autenticado**. El respondedor ya no cambia el endpoint al aceptar un mensaje 1; una repetición del mismo mensaje desde otra IP solo recibe la respuesta guardada (prueba unitaria `an_init_from_another_address_never_redirects_the_confirmed_endpoint`).
- Un solo mensaje 1 se reenvía a todos los candidatos cada ~1 s con jitter; el respondedor repite su respuesta ante retransmisiones idénticas (necesario cuando la respuesta se pierde en un NAT que aún no tenía el agujero).
- No depende de STUN público: los dos puertos STUN los sirve el propio coordinator.

## Qué no demuestra

- La recuperación tras un cambio de red tarda ≈ 45 s (el tiempo de ruta muerta, `PATH_DEAD_AFTER`). No se detecta aún el cambio de interfaz para reaccionar antes (mejora pendiente: avisar al par tras un cambio de candidatos propios).
- Los NAT son emulaciones de Linux/nftables: el NAT simétrico usa rangos de puertos disjuntos y el hairpin solo preinstala dos puertos (ver `tests/network-lab/nat/README.md`). No hay full-cone, filtrado solo por dirección ni routers reales ni UPnP.
- Con hairpin o sin él, en la misma LAN la ruta va por el candidato local; el caso "misma LAN con solo candidatos públicos" (donde el hairpin importa) no se aísla.
- Una sola instancia del coordinator, sin alta disponibilidad (fase 9); HTTP sin TLS dentro del laboratorio.
- Los coordinators y STUN se configuran en el TOML local del agente; su distribución por configuración firmada v2 queda para la fase 5.
- Con el NAT simétrico no hay camino alternativo todavía: el relay es la fase 5.
- No se midió el rendimiento ni el MTU de rutas reales.
- La revisión criptográfica externa sigue pendiente.

## Fuzzing y regresiones

- `fuzz/run.sh 60 stun_parse`: 31 959 842 ejecuciones (≈ 524 000/s), sin hallazgos. Humo de un minuto, no fuzzing continuado.
- Pruebas unitarias en Linux del workspace completo y clippy `-D warnings`: limpios. El agente pasa de 34 a 41 pruebas (sondeo, candidatos saneados, endpoint no redirigible, keepalive y ruta muerta, cliente del coordinator).
- Regresiones con el agente nuevo (keepalive cada 15 s, endpoint solo confirmado por paquetes autenticados): `tests/network-lab/run.sh` (fases 2 y 3) y `tests/network-lab/run-e2e.sh` (control plane con Dex y PostgreSQL) siguen en verde.
