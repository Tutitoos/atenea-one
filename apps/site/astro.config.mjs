// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

import { unified } from "@astrojs/markdown-remark";
import sitemap from "@astrojs/sitemap";
import starlight from "@astrojs/starlight";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "astro/config";
import rehypeExternalLinks from "rehype-external-links";
import {
  PRODUCTION_ORIGIN,
  PROJECT_NAME,
  PROJECT_TAGLINE,
  REPOSITORY_URL,
} from "./src/site.mjs";

// Sitio estático: no hay nada dinámico y el despliegue es solo servir `dist/`.
// `site` se hornea en el build y de él salen canónicas, sitemap y `og:url`, así
// que el respaldo es el origen real y desarrollo lo sustituye con
// `ATENEA_SITE_URL`.
export default defineConfig({
  site: process.env.ATENEA_SITE_URL ?? PRODUCTION_ORIGIN,
  trailingSlash: "always",
  server: { port: 6767, host: true },
  // Los enlaces que salen del sitio se abren en pestaña nueva; `noopener`
  // impide que la pestaña abierta navegue la actual. El procesador es explícito
  // porque Starlight muta el configurado y solo reconoce dos motores.
  markdown: {
    processor: unified({
      rehypePlugins: [
        [
          rehypeExternalLinks,
          { target: "_blank", rel: ["noopener", "noreferrer"] },
        ],
      ],
    }),
  },
  integrations: [
    // Declarado aquí para que Starlight no añada el suyo.
    sitemap(),
    starlight({
      // La 404 es de este proyecto (`src/pages/404.astro`).
      disable404Route: true,
      title: PROJECT_NAME,
      description: PROJECT_TAGLINE,
      favicon: "/favicon.svg",
      defaultLocale: "root",
      locales: { root: { label: "Español", lang: "es" } },
      // global.css declara los tokens de las dos mitades; docs.css es la piel
      // solo de Starlight. El orden importa: la segunda lee tokens de la primera.
      customCss: ["./src/styles/global.css", "./src/styles/docs.css"],
      // Solo oscuro: el proveedor fija el atributo y el selector no ofrece nada.
      components: {
        Head: "./src/components/starlight/Head.astro",
        ThemeProvider: "./src/components/starlight/ThemeProvider.astro",
        SocialIcons: "./src/components/starlight/SocialIcons.astro",
        ThemeSelect: "./src/components/starlight/ThemeSelect.astro",
      },
      social: [{ icon: "github", label: "GitHub", href: REPOSITORY_URL }],
      sidebar: [
        {
          label: "Empezar",
          items: [
            { label: "Instalación", slug: "instalacion" },
            { label: "Cómo funciona", slug: "como-funciona" },
          ],
        },
        {
          label: "Referencia",
          items: [
            { label: "Frente a Tailscale y ZeroTier", slug: "comparacion" },
            { label: "Rendimiento", slug: "rendimiento" },
            { label: "Seguridad", slug: "seguridad" },
            { label: "Estado y límites", slug: "limites" },
          ],
        },
      ],
    }),
  ],
  vite: { plugins: [tailwindcss()] },
});
