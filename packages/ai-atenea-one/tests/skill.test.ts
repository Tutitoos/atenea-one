// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");
const skill = read("skills/atenea-one/SKILL.md");
const files = [skill, read("skills/atenea-one/references/model.md")];

describe("skill atenea-one", () => {
  it("tiene frontmatter válido con name y description", () => {
    const m = /^---\nname: (\S+)\ndescription: (.+)\n---\n/.exec(skill);
    expect(m?.[1]).toBe("atenea-one");
    expect(m?.[2]).toMatch(/redes.*dispositivos.*servidores.*VPN.*Atenea One/);
    expect(m?.[2]).toMatch(/ping/);
  });

  it("cada tool mencionada existe en tools.ts", () => {
    const registered = new Set([...read("mcp/tools.ts").matchAll(/"(atenea_\w+)",\n/g)].map((m) => m[1]));
    expect(registered.size).toBe(4);
    const mentioned = new Set(files.flatMap((f) => f.match(/atenea_\w+/g) ?? []));
    for (const t of mentioned) expect(registered, t).toContain(t);
    for (const t of registered) expect(mentioned, t).toContain(t);
  });

  it("solo usa IPv4 sintéticas de 10.77.0.0/24, 192.0.2.0/24 o 203.0.113.0/24", () => {
    const ips = files.flatMap((f) => f.match(/\b\d{1,3}(?:\.\d{1,3}){3}\b/g) ?? []);
    for (const ip of ips) expect(ip, ip).toMatch(/^(10\.77\.0|192\.0\.2|203\.0\.113)\.\d{1,3}$/);
  });
});
