// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// El sitio se publica bajo una ruta (`/atenea-one/` en GitHub Pages) y con un
// dominio propio sería la raíz. Un enlace escrito como `/instalacion/` apuntaría
// fuera del sitio en el primer caso, así que todo enlace o recurso interno pasa
// por aquí y el prefijo sale de `base` en `astro.config.mjs`.

const base = import.meta.env.BASE_URL.replace(/\/$/, "");

/** Antepone la base del sitio a una ruta que empieza por `/`. */
export function withBase(path: string): string {
  return `${base}${path.startsWith("/") ? path : `/${path}`}`;
}

/** Si la ruta ya lleva la base puesta, como `Astro.url.pathname`. */
export function hasBase(path: string): boolean {
  return base === "" || path === base || path.startsWith(`${base}/`);
}
