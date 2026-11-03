// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { z } from "zod";
import type { Config } from "./config.ts";

const ErrorBody = z.object({ error: z.object({ code: z.string().regex(/^\w{1,64}$/) }) });

/**
 * GET de docs/api/openapi.json que un token de servicio puede leer: todos los `get` bajo
 * /api/v1/ salvo join-code, events, session, device/* y service-tokens (ADR-020).
 * tests/tools.test.ts comprueba que sigue siendo idéntica a la derivada del OpenAPI.
 */
export const ALLOWED_TEMPLATES = [
  "/api/v1/networks/{net}",
  "/api/v1/networks/{net}/audit",
  "/api/v1/networks/{net}/devices",
  "/api/v1/networks/{net}/dns",
  "/api/v1/networks/{net}/metrics",
  "/api/v1/networks/{net}/policies",
  "/api/v1/networks/{net}/routes",
  "/api/v1/orgs",
  "/api/v1/orgs/{org}/members",
  "/api/v1/orgs/{org}/networks",
  "/api/v1/relay-ca",
  "/api/v1/server",
  "/api/v1/server/info",
];

// Segmentos solo [\w-]: sin `.`, `/` ni `%`, de modo que no hay `..` ni barras codificadas.
const ALLOWED = ALLOWED_TEMPLATES.map((t) => new RegExp(`^${t.replace(/\{\w+\}/g, "[\\w-]+")}$`));

/** Error HTTP o de red ya saneado: nunca incluye cabeceras, cuerpo ni token. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(`${status} ${code}`);
  }
}

export type Getter = { get(path: string): Promise<unknown> };

export class Client implements Getter {
  constructor(
    private cfg: Config,
    private fetchImpl: typeof fetch = fetch,
  ) {}

  async get(path: string): Promise<unknown> {
    const [pathname, query = ""] = path.split("?", 2);
    if (!ALLOWED.some((re) => re.test(pathname))) throw new ApiError(0, "path_not_allowed");
    let res: Response;
    try {
      res = await this.fetchImpl(`${this.cfg.url}${pathname}${query ? `?${query}` : ""}`, {
        method: "GET",
        headers: { Authorization: `Bearer ${this.cfg.token}`, Accept: "application/json" },
        redirect: "error",
        signal: AbortSignal.timeout(10_000),
      });
    } catch {
      throw new ApiError(0, "network");
    }
    if (!res.ok) {
      let code = "http_error";
      try {
        code = ErrorBody.parse(await res.json()).error.code;
      } catch {
        // cuerpo no JSON: se queda el código genérico
      }
      throw new ApiError(res.status, code);
    }
    return res.json();
  }
}
