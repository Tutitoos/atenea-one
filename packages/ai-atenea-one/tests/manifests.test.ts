// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const raw = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");
const json = (p: string) => JSON.parse(raw(p));
const PATHS = [".claude-plugin/plugin.json", ".claude-plugin/marketplace.json", ".codex-plugin/plugin.json", ".agents/plugins/marketplace.json", ".omp-plugin/marketplace.json", ".mcp.json", "package.json"];

describe("manifiestos del plugin", () => {
  const pkg = json("package.json");
  const claude = json(".claude-plugin/plugin.json");
  const codex = json(".codex-plugin/plugin.json");
  const omp = json(".omp-plugin/marketplace.json");
  const mcp = json(".mcp.json");

  it("declaran el mismo servidor MCP", () => {
    const server = mcp.mcpServers["atenea-one"];
    // Claude (copia el plugin sin los enlaces de pnpm) y Codex lo lanzan vía launcher.
    expect(server).toEqual({ command: "node", args: ["scripts/launch-codex-mcp.ts", "mcp/server.ts"], cwd: "." });
    expect(codex.mcpServers["atenea-one"]).toEqual(server);
    expect(Object.keys(mcp.mcpServers)).toEqual(["atenea-one"]);
  });

  it("comparten versión y nombre, y exponen la skill", () => {
    for (const v of [claude.version, codex.version, omp.metadata.version, omp.plugins[0].version]) expect(v).toBe(pkg.version);
    for (const n of [claude.name, codex.name, omp.plugins[0].name, pkg.omp.name]) expect(n).toBe("ai-atenea-one");
    expect(claude.skills).toBe("./skills/");
    expect(codex.skills).toBe("./skills/");
  });

  it("no contienen tokens ni URLs salvo example.test", () => {
    for (const p of PATHS) {
      const text = raw(p);
      expect(text, p).not.toMatch(/atsvc_/);
      for (const url of text.match(/https?:\/\/[^\s"']+/g) ?? []) expect(url, p).toMatch(/\.example\.test|\/\/example\.test/);
    }
  });
});
