# Web oficial y documentación (`apps/site/`)

Astro + Starlight + Tailwind 4, sitio estático en español. Estas reglas se suman a las de la raíz y no las relajan.

## Alcance

- La portada no es una página de Starlight: su shell es `src/components/landing/Layout.astro`. Solo `src/content/docs/` lleva el chrome de Starlight. Es oscura por construcción (`ThemeProvider` fija `data-theme="dark"` y no hay selector).
- Solo portada y documentación. Sin blog ni releases hasta que se pidan.
- Repositorio público: ni analítica con claves, ni dominios o IPs reales de servidores, ni capturas reales. El único origen es `PRODUCTION_ORIGIN` en `src/site.mjs` (GitHub Pages) y la ruta es `BASE_PATH`.
- El sitio vive bajo `/atenea-one/`, no en la raíz. Todo enlace o recurso interno de un componente pasa por `withBase()` de `src/lib/url.ts`; en Markdown y MDX se escriben rutas absolutas (`/instalacion/`) y `rehypeBaseLinks` les pone la base. Un dominio propio solo exige `ATENEA_SITE_URL` y `ATENEA_SITE_BASE=/`.
- Los datos que el sitio afirma salen de `docs/` del repositorio: no inventes estado, versiones ni garantías. El estado honesto (MVP de laboratorio, sin auditoría independiente) debe seguir visible.

## Estilo

- `global.css` importa utilities de Tailwind en la capa `utilities` y **no** importa preflight. Una regla de componente sin capa gana a cualquier utility, así que los resets de margen viven solo en el `@layer base` de `Layout.astro`.
- Una sola paleta, en el `@theme` de `global.css`; nadie escribe un literal de color en otro sitio. Texto de prosa `gray-300`, etiquetas `gray-400`; nada más tenue lleva texto (AA, 4,5:1).
- Una sola regla de tipografía: `font-mono` para titulares, etiquetas, identificadores, comandos y cifras; sans para la prosa. Los radios son `0` y no hay sombras.
- Si cambias `PROJECT_TAGLINE`, cambia la misma frase en cualquier contenido que la copie.

## Verificación

`pnpm --filter @atenea/site run lint|typecheck|build`. El build falla si Astro emite un aviso. `scripts/check.sh` ya lo ejecuta.
