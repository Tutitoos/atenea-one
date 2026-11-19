# Identidad visual del sitio en el panel y la app: plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** que `apps/web` (panel) y `apps/desktop` (app) tengan la misma piel que `apps/site`: paleta, tipografía, radios, foco y movimiento, desde una única fuente de tokens.

**Architecture:** un paquete `packages/ui-tokens` con el `@theme` de Tailwind 4 (hoy en `apps/site/src/styles/global.css`) y una base CSS mínima. Los tres proyectos lo importan; ninguno declara literales de color propios. El panel conserva sus clases CSS y su selector de tema (claro/oscuro/sistema, con variante clara nueva); la app pasa a solo oscuro y sus clases Tailwind se renombran a los tokens del sitio.

**Tech Stack:** Tailwind 4.3.5 (`@theme`), Vite 8, React Router 8, Astro 7 (sitio), `@fontsource-variable/geist` y `@fontsource-variable/geist-mono` 5.3.0, Vitest 5, Playwright + axe, Wails (Go) para la CSP de la app.

**Spec:** este documento, sección «Decisiones». Referencia visual: `apps/site/AGENTS.md` (§ Estilo) y `apps/site/src/styles/global.css`.

## Decisiones

Del responsable:
- **Temas:** panel con claro + oscuro + sistema; app solo oscuro (se elimina `prefers-color-scheme` de la app).
- **Formas:** literal del sitio también en la app: radios `0`, sin sombras, `font-mono` en titulares, etiquetas, identificadores, comandos y cifras; sans para prosa. `Dot` y `Pill` dejan de ser redondos.
- **Tokens compartidos:** paquete nuevo `packages/ui-tokens` con tres consumidores (cumple la regla de `AGENTS.md`: sin paquetes sin consumidores).
- **Fuente de la app:** Geist + Geist Mono empaquetadas.

Mías (el responsable puede cambiarlas):
- El sitio sigue siendo dueño solo de lo que usa únicamente él (`--color-graph-*`, `--ec-*`, reset de `Layout.astro`); el paquete lleva fuentes, `accent-*`, `gray-*`, `ok`, `no`, `shell`, `panel`, `raise`, `rule`, `rule-strong`.
- `warn` = `accent-200` (el sitio usa el oro para «en curso»); `bad` = `no`; `ok` = `ok`; `off` = `gray-500` solo en puntos, nunca en texto.
- El acento de acción (`sel` azul) pasa a oro: botón primario `bg-accent-200 text-shell`; el texto blanco sobre oro o sobre verde no pasa AA, por eso `Pill` pasa a contorno con texto del tono.
- Variante clara: valores iniciales abajo, validados por un test de contraste (≥ 4,5:1); en `claro` la escala `gray-*` se invierte (gray-100 es el más oscuro). Se documenta en el paquete.
- Fuera de alcance: contenido, textos, navegación, comportamiento, contrato de API, icono `icon.svg`.

## Global Constraints

- Una sola paleta, en `packages/ui-tokens/theme.css`; ningún literal de color (`#...`, `rgb(`) en `apps/web/app`, `apps/desktop/app` ni `apps/site/src` salvo `--color-graph-*` y `--ec-*`.
- Texto de prosa `gray-300`, etiquetas `gray-400`; nada más tenue lleva texto (AA, 4,5:1).
- Radios `0`, sin `box-shadow`/`shadow-*`/`rounded*` en panel y app.
- `font-mono` para titulares, etiquetas, identificadores, comandos y cifras; sans para prosa.
- Foco: `outline: 1px solid var(--color-accent-200); outline-offset: 2px` en el panel y en la app; `::selection` con `accent-900` y `gray-100`.
- `prefers-reduced-motion: reduce` anula animaciones y transiciones (ya existe en ambos; se conserva).
- Repositorio público: sin secretos, IPs ni capturas reales; las capturas de verificación son de datos sintéticos y no se commitean.
- CSP de la app: `default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'` + solo `font-src 'self'`. CSP del panel (`apps/control-plane/src/panel.rs:576`) no cambia: `default-src 'self'` ya cubre fuentes del mismo origen.
- Sin dependencias nuevas aparte de las dos fuentes `@fontsource-variable/*` ya fijadas en el sitio, en la misma versión.
- Cabecera de licencia (`Copyright 2026 Tutitoos` + `SPDX-License-Identifier: Apache-2.0`) en todo fichero nuevo; `python3 scripts/check-headers.py` debe pasar.
- Español en comentarios y textos.

## Review Focus

1. Contraste en claro: texto `gray-400` sobre `raise`, `accent-200` como texto y como fondo de botón, `ok`/`no` sobre `panel`. Esperado: ≥ 4,5:1; `pnpm --filter @atenea/ui-tokens test` y axe del panel en ambos temas.
2. Cadena larga sin espacios (UUID, huella, URL, código de unión) en mono: no desborda tablas ni tarjetas en 390 px (panel) ni a 640 px de ancho de ventana (app).
3. Fuentes bloqueadas por CSP en la app: sin `font-src` Geist cae a fuente del sistema sin error visible. Esperado: la CSP lo permite y el test Go lo fija.
4. Cambio de tema en caliente en el panel con `sistema` y `matchMedia` cambiando: sin parpadeo de color incorrecto tras hidratar.
5. Foco visible sobre `bg-accent-200` (botón primario) y sobre el elemento activo de la barra lateral: esperado: anillo visible de contraste ≥ 3:1.

---

## Mapa de ficheros

| Fichero | Responsabilidad |
|---|---|
| `packages/ui-tokens/package.json` (nuevo) | paquete `@atenea/ui-tokens`, `exports` de los dos CSS, test |
| `packages/ui-tokens/theme.css` (nuevo) | `@theme` compartido + variante `[data-theme=claro]` |
| `packages/ui-tokens/base.css` (nuevo) | foco, selección, `box-sizing`, movimiento reducido, Geist (para panel y app) |
| `packages/ui-tokens/contrast.test.ts` (nuevo) | contraste AA de los pares texto/fondo en `oscuro` y `claro` |
| `apps/site/src/styles/global.css` | sustituye su `@theme` compartido por el import |
| `apps/web/app/styles.css`, `panel.tsx`, `ui.tsx` | panel con los tokens |
| `apps/desktop/app/styles.css`, `ui.tsx`, `routes/*.tsx`, resto de `*.tsx` | app con los tokens |
| `apps/desktop/internal/secure/secure.go` + `secure_test.go` | `font-src 'self'` |
| `apps/web/README.md`, `apps/site/AGENTS.md`, `packages/ui-tokens/README.md` | documentación |

---

### Task 1: paquete `@atenea/ui-tokens`

**Files:**
- Create: `packages/ui-tokens/package.json`, `theme.css`, `base.css`, `contrast.test.ts`, `README.md`, `vitest.config.ts`
- Modify: `pnpm-workspace.yaml` (añadir `- packages/ui-tokens` tras `packages/api-client`)

**Interfaces:**
- Produces: `@atenea/ui-tokens/theme.css` (tokens `--font-sans`, `--font-mono`, `--color-accent-{200,600,900,950}`, `--color-gray-{100,200,300,400,500,700,800,900}`, `--color-ok`, `--color-no`, `--color-shell`, `--color-panel`, `--color-raise`, `--color-rule`, `--color-rule-strong`; variante `:root[data-theme=claro]` que redefine esos mismos colores) y `@atenea/ui-tokens/base.css` (reglas de base; importa las dos fuentes).
- Consumes: nada.

- [ ] **Step 1: escribir el test que falla** — `contrast.test.ts` lee `theme.css`, extrae los valores del `@theme` (oscuro) y del bloque `:root[data-theme=claro]` y comprueba `contrast(fg, bg) >= 4.5` para: `gray-300` y `gray-400` y `accent-200` y `ok` y `no` sobre `shell`, `panel` y `raise`; en `claro`, además `shell` (texto del botón primario) sobre `accent-200` (fondo). Test `it.each(['oscuro','claro'])('pares de texto cumplen AA en %s')`.
- [ ] **Step 2:** `pnpm --filter @atenea/ui-tokens test` → FAIL (no existe `theme.css`).
- [ ] **Step 3: `theme.css`.** Copiar del sitio los valores oscuros tal cual (`global.css:10-24,44-59`). Variante clara, valores iniciales a ajustar hasta que el test pase: `shell #f6f6f4`, `panel #ffffff`, `raise #eeeeec`, `rule #dcdcd8`, `rule-strong #b8b8b2`, `gray-100 #141414`, `gray-200 #1f1f1f`, `gray-300 #2e2e2e`, `gray-400 #55555a`, `gray-500 #75757a`, `gray-700 #b8b8b2`, `gray-800 #dcdcd8`, `gray-900 #eeeeec`, `accent-200 #7a5712`, `accent-600 #9a7020`, `accent-900 #f0d58f`, `accent-950 #f7ecd0`, `ok #1f7a4d`, `no #b3262b`. Sin `@theme inline`: las utilities deben leer `var(--color-*)` para que la variante clara surta efecto.
- [ ] **Step 4: `base.css`.** `@import "@fontsource-variable/geist"; @import "@fontsource-variable/geist-mono";`; `*{box-sizing:border-box}`; `:focus-visible` y `::selection` como en `Layout.astro:253-261`; bloque de movimiento reducido de `Layout.astro:265-274`. `color-scheme: dark` en `:root` y `color-scheme: light` en `:root[data-theme=claro]`.
- [ ] **Step 5:** `package.json` con `name`, `private`, `type: module`, `license`, `exports` de los dos CSS, dependencias de las dos fuentes y `vitest` en devDependencies (mismas versiones que el resto); `pnpm install`.
- [ ] **Step 6:** test → PASS; `python3 scripts/check-headers.py` → OK.
- [ ] **Step 7: Commit** `feat(ui-tokens): tokens y base visual compartidos del sitio, panel y app`.

---

### Task 2: el sitio consume el paquete (sin cambio visual)

**Files:**
- Modify: `apps/site/src/styles/global.css:3-60` (el `@theme` compartido sale; se conservan `graph-*`, `--ec-*` y el resto), `apps/site/package.json` (devDependency `@atenea/ui-tokens: workspace:*`)

**Interfaces:**
- Consumes: `@atenea/ui-tokens/theme.css` de Task 1.

- [ ] **Step 1:** guardar `pnpm --filter @atenea/site run build` y copiar `apps/site/dist/_astro/*.css` a un directorio temporal fuera del repo.
- [ ] **Step 2:** sustituir en `global.css` el bloque `@theme` por `@import "@atenea/ui-tokens/theme.css";` (después de los imports de Tailwind, dentro de `layer(theme)` como el resto) y dejar un `@theme` mínimo solo con `--color-graph-*`.
- [ ] **Step 3:** repetir el build y comparar los valores de las propiedades `--color-*` y `--font-*` entre el CSS anterior y el nuevo: idénticos. Si Starlight pierde una variable, mover ese token de vuelta al `@theme` del sitio.
- [ ] **Step 4:** `pnpm --filter @atenea/site run lint|typecheck|build` → sin avisos (el build falla con avisos de Astro).
- [ ] **Step 5: Commit** `refactor(site): el tema compartido sale a @atenea/ui-tokens`.

---

### Task 3: panel (`apps/web`)

**Files:**
- Modify: `apps/web/app/styles.css` (reescritura de tokens y formas; se conservan los selectores), `apps/web/app/panel.tsx:15-27` (selector `Theme`), `apps/web/app/ui.tsx` (insignias), `apps/web/package.json` (dependencia `@atenea/ui-tokens`; se quita `geist` y su `@font-face`), `pnpm-workspace.yaml` (el override `next` solo existía por `geist`: eliminarlo si ya no se usa)
- Test: `apps/web/e2e/panel.spec.ts` (axe en ambos temas), tests unitarios existentes

**Interfaces:**
- Consumes: `theme.css` y `base.css` de Task 1.
- Produces: contrato de temas inalterado: `document.documentElement.dataset.theme` ∈ {`oscuro`,`claro`}, clave `localStorage['atenea-theme']` ∈ {`sistema`,`claro`,`oscuro`}.

- [ ] **Step 1: test que falla** en `panel.spec.ts`: tras `selectOption('claro')` y `selectOption('oscuro')`, comprobar `getComputedStyle(document.body).fontFamily` incluye `Geist` y que `getComputedStyle(document.querySelector('.card')).borderRadius === '0px'`; mantener las llamadas `axeClean` de cada tema. Ejecutar → FAIL por radio `8px`.
- [ ] **Step 2:** `styles.css`: importar `@atenea/ui-tokens/theme.css` y `base.css` tras `tailwindcss`; quitar `@font-face` y los dos bloques `:root` (líneas 10-29 y 506-536); los alias del panel pasan a leer tokens: `--bg: var(--color-shell)`, `--surface: var(--color-panel)`, `--soft: var(--color-raise)`, `--line: var(--color-rule)`, `--muted: var(--color-gray-400)`, `--fg: var(--color-gray-100)`, `--sel: var(--color-accent-200)`, `--ok: var(--color-ok)`, `--bad: var(--color-no)`, `--warn: var(--color-accent-200)`, `--radius: 0`. (Los alias son internos del panel y evitan reescribir cientos de usos; ningún literal de color queda.)
- [ ] **Step 3:** formas: `border-radius` → `0` en todo `styles.css`; sin `box-shadow`; `h1,h2,h3,th,caption,code,pre,.eyebrow,.brand,.badge,.short-id,.join-code,button,.button,label,legend,summary` en `font-family: var(--font-mono)`; `p`, `td` de prosa, `.field-help` y `input` en sans; `.button.primary` pasa a `background: var(--color-accent-200); color: var(--color-shell); border-color: var(--color-accent-200)`; `.button.danger` a contorno `var(--color-no)`; foco con el anillo común (se quita el `outline: 3px solid #377de8`); `.badge::before` cuadrado, no círculo.
- [ ] **Step 4:** `panel.tsx` `Theme()`: sin cambios de lógica; solo asegurarse de que `useEffect` aplica el tema antes de pintar o, si hay parpadeo, que el valor por defecto sea el oscuro (el sitio es oscuro): `useState('sistema')` se queda.
- [ ] **Step 5:** `ui.tsx`: `StatusBadge` con contorno y texto del tono, sin relleno saturado.
- [ ] **Step 6:** `grep -nE '#[0-9a-fA-F]{3,8}\b|rgb\(' apps/web/app/*.css apps/web/app/*.tsx` → sin resultados.
- [ ] **Step 7:** `pnpm --filter @atenea/web run lint|typecheck|test|build` y `pnpm --filter @atenea/web exec playwright test` → PASS, incluido `axeClean` en `oscuro`, `claro` y 390 px.
- [ ] **Step 8:** abrir el panel construido (datos sintéticos de `global-setup.ts`) en Resumen, Dispositivos, Incorporar, Políticas y la pantalla de login en ambos temas y a 390 px; comparar con la portada del sitio: mismas superficies, oro y mono. Ajustar `styles.css` hasta que coincidan.
- [ ] **Step 9: Commit** `feat(web): el panel adopta la piel del sitio (tokens, Geist Mono, radios 0)`.

---

### Task 4: app, base y CSP (`apps/desktop`)

**Files:**
- Modify: `apps/desktop/app/styles.css` (reescritura), `apps/desktop/package.json` (`@atenea/ui-tokens`), `apps/desktop/internal/secure/secure.go:17`
- Test: `apps/desktop/internal/secure/secure_test.go`

**Interfaces:**
- Consumes: `theme.css` y `base.css`.
- Produces: utilidades Tailwind de la app, base para Task 5: `bg-shell`, `bg-panel`, `bg-raise`, `border-rule`, `border-rule-strong`, `text-gray-100|300|400`, `text-accent-200`, `bg-accent-200`, `text-ok`, `text-no`, `font-mono`.

- [ ] **Step 1: test Go que falla** en `secure_test.go`: la constante `CSP` contiene `font-src 'self'` y no contiene `unsafe-inline` ni orígenes remotos. `go test ./internal/secure/...` → FAIL.
- [ ] **Step 2:** `secure.go`: `const CSP = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'"`, actualizar el comentario de la línea 16. → PASS.
- [ ] **Step 3:** `styles.css`: `@import 'tailwindcss'; @import '@atenea/ui-tokens/theme.css'; @import '@atenea/ui-tokens/base.css';`; eliminar `:root{--bg...}`, el `@media (prefers-color-scheme: dark)` y `@theme inline`; `body { background: var(--color-shell); color: var(--color-gray-300); font: 13px/1.5 var(--font-sans) }` (se conservan `user-select: none`, `.stale-fade`, `.toast`, `.skeleton` y el bloque de movimiento reducido); sin `outline` azul propio (lo da `base.css`).
- [ ] **Step 4:** `pnpm --filter @atenea/desktop run build` y comprobar que `build/client/assets/` contiene los `.woff2` de Geist y de Geist Mono (referencias relativas, sin `http`).
- [ ] **Step 5: Commit** `feat(desktop): base visual del sitio y font-src en la CSP`.

---

### Task 5: app, componentes y pantallas

**Files:**
- Modify: `apps/desktop/app/ui.tsx`, `toast.tsx`, `context-menu.tsx`, `shortcuts-help.tsx`, `sparkline.tsx`, `device-history.tsx`, `admin.tsx`, `services.tsx`, `join.tsx`, `welcome.tsx`, `preferences.tsx`, `root.tsx`, `routes/*.tsx` y los tests que consulten clases

**Interfaces:**
- Consumes: utilidades de Task 4.
- Produces: `Dot`, `Pill`, `Heading`, `Card`, `Row`, `CopyButton`, `Empty`, `Skeleton`, `Loading`, `button`, `primary`, `danger`, `NoNetworks` con las mismas firmas que hoy.

Tabla de renombrado (los literales viejos desaparecen, sin alias):

| Antes | Después |
|---|---|
| `bg-bg` | `bg-shell` |
| `bg-side` | `bg-panel` |
| `bg-card` | `bg-panel border border-rule` (si ya lleva `border` solo `bg-panel`) |
| `border-line`, `divide-line` | `border-rule`, `divide-rule` |
| `text-fg` | `text-gray-100` |
| `text-mut` | `text-gray-400` |
| `bg-sel text-white` | `bg-accent-200 text-shell` |
| `text-sel`, `border-sel`, `outline-sel` | `text-accent-200`, `border-accent-200`, `outline-accent-200` |
| `bg-fg/10`, `hover:bg-fg/10` | `bg-raise`, `hover:bg-raise` |
| `bg-ok`, `bg-warn`, `bg-bad`, `bg-off` (puntos) | `bg-ok`, `bg-accent-200`, `bg-no`, `bg-gray-500` |
| `text-bad`, `bg-bad/10` | `text-no`, `bg-no/10` |
| `text-warn`, `bg-warn/15` | `text-accent-200`, `bg-accent-200/10` |
| `rounded`, `rounded-md`, `rounded-lg`, `rounded-[10px]`, `rounded-full` | se eliminan |
| `shadow-lg`, `shadow-xl` | se eliminan; los pop-ups llevan `border border-rule-strong` |

- [ ] **Step 1: test que falla** en `ui.test` nuevo (`apps/desktop/app/ui-skin.test.tsx`): renderizar `Dot`, `Pill`, `Card`, `button`/`primary`/`danger` y comprobar por `className` que no contienen `rounded`, `shadow` ni `text-white`, y que `Pill` contiene `border` y `font-mono`. Ejecutar → FAIL.
- [ ] **Step 2:** `ui.tsx`: `Dot` cuadrado (`size-2` sin radio); `Pill` con `border border-current font-mono text-xs uppercase`, color del tono, sin relleno; `Heading` y `Row` `dt` en `font-mono`, valores numéricos `tabular-nums`; `button`/`primary`/`danger` según tabla (`danger`: contorno `border-no text-no`).
- [ ] **Step 3:** aplicar la tabla a todos los `.tsx` con un script desechable fuera del repo (mapa literal clase a clase, solo dentro de `className`), revisar el diff entero y borrar el script. `grep -nE "rounded|shadow|text-white|bg-sel|text-sel|border-line|text-mut|bg-card|bg-side|bg-bg|text-fg" apps/desktop/app --include=*.tsx` → sin resultados salvo los tests de este task.
- [ ] **Step 4:** `font-mono` en: título y subtítulo de `shell.tsx`, enlaces de la barra lateral, `h2/h3` de cada ruta, huellas, claves públicas, direcciones IP, puertos y cifras de `overview`, `device-history`, `diagnostics`. La prosa (párrafos, ayudas, avisos) queda en sans.
- [ ] **Step 5:** `pnpm --filter @atenea/desktop run lint|typecheck|test|build` → PASS (actualizar los tests que consultaban las clases antiguas).
- [ ] **Step 6:** smoke visual: `pnpm --filter @atenea/desktop run dev` y recorrer Resumen, Dispositivos (lista y detalle), Servidores, Redes, Diagnóstico, asistente de bienvenida, menú contextual, atajos y toast con el agente simulado de `apps/desktop/scripts/smoke.py`; comparar con la portada: oro, mono, radios 0, sin sombras. Redimensionar a la ventana mínima y comprobar desbordes (Review Focus 2).
- [ ] **Step 7: Commit** `feat(desktop): componentes y pantallas con la piel del sitio`.

---

### Task 6: documentación y cierre

**Files:**
- Modify: `apps/web/README.md:4` (temas y fuente), `apps/site/AGENTS.md` (§ Estilo: la paleta vive en `@atenea/ui-tokens`), `apps/desktop` README si menciona el aspecto nativo
- Create: `packages/ui-tokens/README.md` (uso, inversión de `gray-*` en claro, regla de no literales)

- [ ] **Step 1:** actualizar los tres textos; el README del paquete incluye la regla «un token nuevo se añade aquí y se prueba en `contrast.test.ts`».
- [ ] **Step 2:** `scripts/check.sh` completo → PASS.
- [ ] **Step 3: Commit** `docs: identidad visual compartida (ui-tokens)`.

---

## Self-review

- **Cobertura:** temas (T1, T3, T4), formas (T3, T5), paquete compartido (T1, T2), fuentes y CSP (T1, T4); cada decisión del responsable tiene tarea.
- **Tipos:** los nombres de tokens son los del sitio y se usan igual en T1–T5; las firmas de `ui.tsx` no cambian.
- **Riesgos:** (a) `@theme` importado desde otro paquete en Astro/Starlight: T2 lo comprueba comparando el CSS construido; (b) renombrado masivo en la app: T5 lo hace con mapa literal y `grep` de cierre; (c) la variante clara es diseño nuevo, no copia del sitio: los valores iniciales son propuesta y el test de contraste manda.
