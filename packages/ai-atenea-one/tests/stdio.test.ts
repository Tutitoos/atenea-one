// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// Humo por stdio: el MCP real contra un control plane HTTP falso que imita el contrato (datos sintéticos).
import { createServer, type IncomingMessage, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { D1, data } from "./fixtures.ts";

const TOKEN = `atsvc_${"B".repeat(43)}`;
const seen: { method?: string; url?: string; auth?: string }[] = [];
let http: Server;
let client: Client;

const text = (r: unknown): string => {
  const content = (r as { content: { text: string }[] }).content;
  return content[0].text;
};

beforeAll(async () => {
  http = createServer((req: IncomingMessage, res) => {
    seen.push({ method: req.method, url: req.url, auth: req.headers.authorization });
    const body = req.headers.authorization === `Bearer ${TOKEN}` ? data[(req.url ?? "").split("?")[0]] : undefined;
    res.writeHead(body === undefined ? 401 : 200, { "content-type": "application/json" });
    res.end(JSON.stringify(body ?? { error: { code: "invalid_session", message: "x" } }));
  });
  await new Promise<void>((ok) => http.listen(0, "127.0.0.1", ok));
  const { port } = http.address() as AddressInfo;
  client = new Client({ name: "smoke", version: "0" });
  await client.connect(
    new StdioClientTransport({
      command: process.execPath,
      args: ["mcp/server.ts"],
      cwd: fileURLToPath(new URL("..", import.meta.url)),
      env: { PATH: process.env.PATH ?? "", ATENEA_AI_URL: `http://127.0.0.1:${port}`, ATENEA_AI_TOKEN: TOKEN },
      stderr: "pipe",
    }),
  );
});

afterAll(async () => {
  await client?.close();
  http?.close();
});

describe("MCP por stdio", () => {
  it("tools/list expone las 4 tools", async () => {
    const { tools } = await client.listTools();
    expect(tools.map((t) => t.name).sort()).toEqual(["atenea_api_get", "atenea_get", "atenea_probe", "atenea_search"]);
  });

  it("atenea_search y atenea_get leen el inventario", async () => {
    const found = JSON.parse(text(await client.callTool({ name: "atenea_search", arguments: { query: "servidor", kinds: ["device"] } })));
    expect(found.map((x: { id: string }) => x.id)).toContain(D1);
    const full = JSON.parse(text(await client.callTool({ name: "atenea_get", arguments: { kind: "device", id: D1 } })));
    expect(full.resource.hostname).toBe("srv1");
  });

  it("atenea_probe rechaza destinos fuera del inventario", async () => {
    for (const device of ["10.77.0.99", "no-existe", "10.77.0.2;id"]) {
      const r = await client.callTool({ name: "atenea_probe", arguments: { device, check: "ping" } });
      expect(r.isError, device).toBe(true);
    }
  });

  it("no hay escritura posible ni rutas fuera de la lista", async () => {
    for (const path of ["/api/v1/orgs/o/service-tokens", "/api/v1/networks/n/join-code", "/api/v1/device/config"]) {
      const r = await client.callTool({ name: "atenea_api_get", arguments: { path } });
      expect(r.isError, path).toBe(true);
      expect(text(r)).toContain("path_not_allowed");
    }
    expect(seen.length).toBeGreaterThan(0);
    expect(seen.every((s) => s.method === "GET" && s.auth === `Bearer ${TOKEN}`)).toBe(true);
    expect(seen.some((s) => /join-code|service-tokens|device\//.test(s.url ?? ""))).toBe(false);
  });
});
