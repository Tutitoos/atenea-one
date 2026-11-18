// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

// La identidad del proyecto, en ESM plano para que `astro.config.mjs` pueda
// importarla: ese fichero no pasa por Vite y no puede importar `_seo.ts`, que
// depende de `astro:content`. Nada de aquí puede importar de `astro:*`.

/**
 * Origen en el que se publica el sitio: GitHub Pages. `ATENEA_SITE_URL` lo
 * sustituye (en desarrollo, o al pasar a un dominio propio).
 */
export const PRODUCTION_ORIGIN = "https://tutitoos.github.io";

/**
 * Ruta bajo la que se publica: un sitio de proyecto de GitHub Pages vive en
 * `/<repositorio>/`. Con un dominio propio sería `/`, y `ATENEA_SITE_BASE=/` lo
 * cambia sin tocar nada más.
 */
export const BASE_PATH = "/atenea-one";

export const PROJECT_NAME = "Atenea One";

/** Una frase, reutilizada como descripción del sitio y de toda página sin descripción propia. */
export const PROJECT_TAGLINE =
  "VPN P2P self-hosted: una red privada creada desde un panel web, con dispositivos que se comunican cifrados directamente o mediante relays propios.";

export const REPOSITORY_URL = "https://github.com/Tutitoos/atenea-one";

export const LICENSE_NAME = "Apache-2.0";

export const LICENSE_URL = "https://spdx.org/licenses/Apache-2.0.html";

/** Lo que muestra `public/og.png`, para quien lee el texto alternativo en lugar de la imagen. */
export const PREVIEW_ALT =
  "Tarjeta oscura con la marca y el nombre de Atenea One, y el texto: Tu red privada, en tu infraestructura. VPN P2P self-hosted.";
