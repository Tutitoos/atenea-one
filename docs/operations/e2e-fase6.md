# Fase 6 — panel web y MVP administrable (E2E)

- **Fecha:** 2026-10-25
- **Entorno:** contenedor Linux privilegiado (Docker Desktop, kernel linuxkit, nftables, HAProxy 2.6) con Chromium de Playwright 1.64.1. PostgreSQL 17 y Dex (OIDC) en contenedores hermanos. Control plane (que sirve el panel del mismo origen) en el espacio de nombres raíz; coordinator, relay (UDP 443 y TLS/TCP 443 tras HAProxy) en `inet`; dos agentes Linux con TUN tras NAT domésticos (`home`). Validez de configuración 60 s; certificados, claves raíz/emisor y de firma de tokens generados en cada ejecución.
- **Reproducir:** `tests/e2e-panel/run.sh` (necesita Docker; unos 15 GB de espacio libre la primera vez). Código: `apps/web/e2e/` y `tests/e2e-panel/lab.sh`.

## Resultado

`2 passed` en 58 s de prueba (la primera ejecución tarda mucho más por construir imágenes y compilar):

| Paso (todo desde la interfaz, salvo las acciones de laboratorio marcadas) | Resultado |
| --- | --- |
| Hidratación del build estático con CSP `default-src 'self'` | Sin errores de React, sin violaciones de CSP, sin scripts inline |
| Login con Dex (OIDC Code + PKCE real) | Sesión creada |
| Crear organización y red (ancla raíz offline, solo clave pública) | Red creada |
| Subir `issuer.cert` (generado offline por el laboratorio) | Aceptado |
| Incorporar dos dispositivos (token único mostrado una vez; enrolamiento por CLI en el laboratorio) | Quedan `pending` |
| Aprobar comparando la huella del diálogo con la de `device-init` | `approved`, IPv4+IPv6 asignadas |
| Crear política «cualquiera → cualquiera» | Creada |
| Arrancar agentes con el `join` del panel (laboratorio) | Informan al panel: `online` con antigüedad |
| Estado de camino en el panel + transferencia TCP 1 MB | **direct**, íntegra |
| Bloqueo del P2P (laboratorio) | El panel muestra **relay-udp**; transferencia íntegra |
| Bloqueo de todo UDP (laboratorio) | El panel muestra **relay-tls**; transferencia íntegra |
| Vistas Relays, Topología, Auditoría con datos reales | Relay `lab` observado; eventos `device.enroll`, `device.approve`, `policy.create` |
| Accesibilidad (axe, WCAG 2 A/AA y 2.1 AA, **incluye contraste**) en login, dispositivos, relays, topología, tema oscuro y móvil (390 px) | Sin violaciones |
| Revocar un dispositivo desde el panel, comparando huella | `revoked`; el otro agente deja de alcanzarlo en **5 s** (sondeo de sincronización 10 s) |

## Qué demuestra

- Operar una red de dos Linux sin editar archivos ni usar comandos internos del control plane: todo el flujo administrativo se hace desde la interfaz; los comandos de laboratorio se limitan a lo que por diseño ocurre fuera del panel (claves y certificado raíz/emisor offline, enrolamiento del dispositivo, red).
- El estado direct/relay que muestra el panel coincide con lo que el laboratorio fuerza, con antigüedad del dato. Es **declarado por los agentes** (autenticado como cada dispositivo), no verificado por un tercero, y así se rotula.
- Los tipos del cliente se generan desde `docs/api/openapi.json` (sin modelos manuales).

## Qué no demuestra

- Una sola ejecución verde del escenario; no se ha medido su estabilidad (flakiness) con repeticiones. Los tiempos de ruta dependen de que la prueba genere tráfico (sin tráfico no hay handshake).
- Teclado y foco se prueban con las pruebas de componentes y axe, no con un recorrido completo solo con teclado. Los estados cargando/vacío/error/desconectado se cubren con pruebas de componentes (Vitest), no en el E2E; no se cortó el WebSocket del navegador durante el E2E.
- Solo Chromium, un único navegador y dos viewports (escritorio y 390 px). Contraste verificado automáticamente por axe, sin revisión visual humana.
- La revocación tarda segundos con sincronización cada 10 s y lease de 60 s: no es el valor del piloto (300 s).
- El E2E no está en CI (contenedor privilegiado). En CI corren lint, typecheck, pruebas y build del panel y las pruebas de Rust.
- Un solo proceso del control plane (la difusión WebSocket es en memoria); sin HTTPS ni despliegue real; sin revisión criptográfica externa.
- Chunk principal del panel de 758 kB (225 kB gzip): aviso de Vite sin resolver.

## Incidencias de la fase

- La puesta en marcha reveló tres fallos reales del panel que las pruebas jsdom no detectaron: hidratación del documento con scripts externalizados por la CSP, ruta `/` sin coincidencia en React Router 8.5 y un sondeo `new Function` de Zod bloqueado por la CSP. Quedan cubiertos por `e2e/hydration.spec.ts` y pruebas Vitest.
- El E2E dependía de que el disco del anfitrión tuviera margen: con el disco lleno, Docker Desktop se queda en solo lectura.
