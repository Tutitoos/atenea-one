# Panel web de Atenea One

SPA estática de React Router en modo framework (`ssr: false`), Vite y TypeScript
estricto. Interfaz en español, fuente Geist local y temas claro/oscuro/sistema.
No genera claves, no implementa identidad ni sustituye las autorizaciones del
control plane.

## Desarrollo

Requisitos: Node >=24 y pnpm 11.23.0 (fijado en `packageManager` de la raíz).

```sh
pnpm install
pnpm --filter @atenea/web dev
```

Vite redirige `/api` (incluido WebSocket) y `/auth` al control plane de desarrollo
en `http://127.0.0.1:8080`. Configura el proveedor OIDC y el `PUBLIC_URL` del
control plane con el origen que uses: el backend comprueba el Origin del
WebSocket. No hace falta CORS. Las cookies inseguras solo se permiten mediante
la opción explícita de desarrollo del backend.

## Contrato y comprobaciones

`packages/api-client` genera sus tipos con `openapi-typescript` desde
`docs/api/openapi.json`. Los tipos no se versionan: `pnpm install` los genera
mediante `postinstall`, y el build vuelve a generarlos. No ejecutes install con
`--ignore-scripts` sin ejecutar después `pnpm --filter @atenea/api-client generate`.
El panel importa exclusivamente modelos generados, nunca modelos de respuesta
redefinidos a mano. Toda llamada REST está verificada estáticamente contra las
rutas del contrato. Los errores HTTP conservan código y mensaje tipados.

```sh
pnpm -r run lint
pnpm -r run typecheck
pnpm -r run test
pnpm -r run build
```

CI instala Node 24, pnpm fijado y el lockfile congelado. `scripts/check.sh`
integra las mismas comprobaciones y el build del panel con las de Rust.

## Build y servicio del mismo origen

```sh
pnpm --filter @atenea/web build
ATENEA_CP_WEB_DIR=/ruta/atenea-one/apps/web/build/client atenea-control-plane
```

El control plane sirve `build/client`, devuelve `index.html` para rutas SPA y
mantiene reservadas `/api`, `/auth` y `/healthz`. En producción no se necesita
servidor Node. `scripts/csp.mjs` externaliza los scripts de arranque generados
por React Router a ficheros con hash; no se relaja `script-src` ni se necesitan
scripts inline. `entry.client.tsx` hidrata solo `#app`: el documento y los
scripts externos quedan fuera de esa frontera. No se debe volver a hidratar
`document`, porque la externalización cambia los nodos de `<Scripts/>` y
provocaría un desajuste de hidratación. La fuente y los assets se sirven localmente.

El panel declara una ruta índice explícita para `/` además del comodín SPA:
en React Router 8.5, un comodín aislado no selecciona el hijo al abrir `/`, y
el `Outlet` raíz quedaría vacío. No requiere descubrimiento remoto de rutas:
el manifiesto SPA ya incluye ambas. Zod se configura con `jitless: true` antes
de crear los esquemas para evitar tanto compilación JIT como sondeos `eval`
incompatibles con la CSP.

La regresión `e2e/hydration.spec.ts` abre el build real sin sesión, exige el
enlace OIDC, CSP y ausencia de scripts inline, errores de página, infracciones
de CSP y llamadas JS a `console.error`. El HTTP 401 de sesión es esperado;
el mensaje de red que muestra Chromium no es una llamada JS a `console.error`.
Usa la configuración E2E existente y se ejecuta en el contenedor del laboratorio;
Vitest cubre la frontera React, el índice `/` y la validación sin `eval`.

## Operación y seguridad

- Inicia sesión mediante `/auth/login`. Una respuesta 401 presenta la pantalla
  de sesión caducada; 403 y 404 tienen mensajes explícitos, sin inferir acceso.
- Selecciona organización y red; la URL conserva ese contexto y la sección.
  `owner` gestiona miembros; `owner` y `admin` operan la red; `member` lee.
- Crea el ancla con `atenea root-init` en la estación offline. Crea o certifica
  el emisor con la CLI offline; solo se sube `issuer.cert` público, nunca las
  claves privadas (ADR-005). El backend valida longitud, red, firma y vigencia.
- El token de incorporación y el documento `join` solo viven en memoria de la
  pestaña Incorporar. Ocultarlos, salir o recargar los elimina; no se almacenan
  en caché de consultas, localStorage ni sessionStorage. Copiar usa el
  portapapeles del navegador, que requiere contexto seguro. Los comandos con
  token pueden quedar en el historial del shell; usa un entorno protegido.
- CSRF solo vive en memoria del cliente. localStorage guarda únicamente el tema.
- Los caminos y relays son **observados/declarados por los agentes**, no pruebas
  independientes de disponibilidad. La edad parte de `age_secs` del servidor y
  suma tiempo local transcurrido; nunca compara relojes del agente/navegador.
- El WebSocket invalida consultas; el estado siempre se reconstruye por REST.
  Hay reconexión exponencial con jitter y sondeo de respaldo cada 30 s. Las vistas
  muestran antigüedad, carga, vacío, errores y advertencia de desconexión.
- Métricas muestra puntos recibidos sin unirlos, interpolar ni rellenar muestras.
  Topología usa SVG y tabla equivalente accesible. No hay datos de demostración
  ni métricas sintéticas en la aplicación; las fixtures solo están en pruebas.

La validación Zod cubre formato y límites locales. La pertenencia de dispositivos,
la contención de prefijos, el solapamiento CIDR, el último owner y la validez
criptográfica se resuelven autoritativamente en el backend.
