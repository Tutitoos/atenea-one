# @atenea/ui-tokens

Identidad visual única de Atenea One. La consumen el sitio (`apps/site`), el panel (`apps/web`) y la app de escritorio (`apps/desktop`).

- `theme.css`: `@theme` de Tailwind 4 con fuentes (Geist y Geist Mono), acento oro, escala `gray-*`, `ok`, `no` y superficies (`shell`, `panel`, `raise`, `rule`, `rule-strong`). Una sola piel, oscura: no hay tema claro.
- `base.css`: fuentes, `box-sizing`, foco (`1px` oro), selección y movimiento reducido. Lo importan el panel y la app; el sitio tiene su base en `Layout.astro`.

Reglas:

- Ningún literal de color fuera de este paquete (salvo `--color-graph-*` y `--ec-*` del sitio).
- Radios `0` y sin sombras; mono para titulares, etiquetas, identificadores, comandos y cifras; sans para la prosa.
- Un token nuevo se añade en `theme.css` y se prueba en `contrast.test.ts` (AA, 4,5:1).
