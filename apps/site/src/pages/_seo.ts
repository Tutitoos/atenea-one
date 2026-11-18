// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

import type { CollectionEntry } from "astro:content";
import { PROJECT_TAGLINE } from "../site.mjs";
import { hasBase, withBase } from "../lib/url";

// Los hechos que repiten varias superficies (cabeceras, JSON-LD)
// viven en `src/site.mjs` y se reexportan aquí. El guion bajo evita que Astro
// enrute este fichero.

export {
  LICENSE_NAME,
  LICENSE_URL,
  PREVIEW_ALT,
  PRODUCTION_ORIGIN,
  PROJECT_NAME,
  PROJECT_TAGLINE,
  REPOSITORY_URL,
} from "../site.mjs";

/** Una página de la colección `docs`. */
export type DocEntry = CollectionEntry<"docs">;

/**
 * Une una ruta al `site` configurado, con la base del sitio puesta. Acepta tanto
 * `/og.png` como `Astro.url.pathname`, que ya la lleva: no se duplica.
 */
export function absoluteUrl(site: URL | undefined, pathname: string): string {
  const full = hasBase(pathname) ? pathname : withBase(pathname);
  return new URL(full, site ?? "http://localhost").href;
}

/** Ruta HTML de una página de documentación (siempre con barra final). */
export function docPathname(id: string): string {
  return id === "" ? "/" : `/${id}/`;
}

/** Si la entrada es la ruta 404, que no es documentación. */
export function isNotFoundPage(id: string): boolean {
  return id === "404" || id.endsWith("/404");
}

/** La descripción de una página, con la del proyecto como respaldo. */
export function docDescription(entry: DocEntry): string {
  return entry.data.description ?? PROJECT_TAGLINE;
}
