// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0

import type { APIRoute } from "astro";
import { absoluteUrl } from "./_seo";

export const prerender = true;

export const GET: APIRoute = ({ site }) =>
  new Response(
    [
      "User-agent: *",
      "Allow: /",
      "",
      `Sitemap: ${absoluteUrl(site, "/sitemap-index.xml")}`,
      "",
    ].join("\n"),
    { headers: { "Content-Type": "text/plain; charset=utf-8" } },
  );
