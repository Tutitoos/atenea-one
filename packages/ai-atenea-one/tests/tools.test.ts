// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ALLOWED_TEMPLATES, ApiError, Client } from "../mcp/client.ts";
import { loadConfig } from "../mcp/config.ts";
import { apiGet, getResource, search } from "../mcp/tools.ts";
import { D1, FakeClient, HOSTILE, NET1, NET2, dev } from "./fixtures.ts";

const TOKEN = `atsvc_${"A".repeat(43)}`;

describe("atenea_search", () => {
  it("search_matches_name_hostname_ip_and_role", async () => {
    for (const q of ["servidor-ejemplo", "srv1", "10.77.0.2", "fd00:77::2", "SERVER"]) {
      const r = await search(new FakeClient(), { query: q });
      expect(r.map((x) => x.id), q).toContain(D1);
    }
  });

  it("search_servidor_is_an_alias_of_role_server_even_when_the_name_has_no_such_word", async () => {
    const c = new FakeClient({
      [`/api/v1/networks/${NET1}/devices`]: [
        dev("vps-1", { name: "vps-ejemplo", role: "server" }),
        dev("pc-1", { name: "portatil-ejemplo", role: "device" }),
      ],
      [`/api/v1/networks/${NET2}/devices`]: [],
    });
    for (const q of ["servidor", "servidores", "Servers", "server"]) {
      expect((await search(c, { query: q, kinds: ["device"] })).map((x) => x.id), q).toEqual(["vps-1"]);
    }
  });

  it("search_requires_all_terms", async () => {
    expect((await search(new FakeClient(), { query: "servidor online" })).map((x) => x.id)).toEqual([D1]);
    expect(await search(new FakeClient(), { query: "servidor inexistente" })).toEqual([]);
  });

  it("search_filters_by_kinds_and_network", async () => {
    const r = await search(new FakeClient(), { query: "servidor", kinds: ["device"], network: NET1 });
    expect(r.map((x) => x.id)).toEqual([D1]);
  });

  it("search_limit_is_capped_at_50", async () => {
    const many = Array.from({ length: 120 }, (_, i) => dev(`id-${i}`, { name: `masivo-${i}` }));
    const c = new FakeClient({ [`/api/v1/networks/${NET1}/devices`]: many });
    expect(await search(c, { query: "masivo", limit: 500 })).toHaveLength(50);
    expect(await search(c, { query: "masivo", limit: 3 })).toHaveLength(3);
  });

  it("search_marks_declared_fields_and_truncates_names_over_64_chars_and_strips_bidi", async () => {
    const r = await search(new FakeClient(), { query: "evil" });
    expect(HOSTILE.length).toBeGreaterThan(64);
    expect(r[0].name).not.toMatch(/\u202e/);
    expect([...r[0].name]).toHaveLength(64);
    expect(r[0].declared).toEqual(expect.arrayContaining(["name", "hostname", "role", "report"]));
  });
});

describe("atenea_get", () => {
  it("devuelve el recurso completo y falla si no existe", async () => {
    const r = (await getResource(new FakeClient(), { kind: "device", id: D1 })) as { resource: { hostname: string } };
    expect(r.resource.hostname).toBe("srv1");
    await expect(getResource(new FakeClient(), { kind: "device", id: "nope" })).rejects.toThrow(/no encontrado/);
  });
});

describe("Client", () => {
  const mk = (f: unknown) => new Client({ url: "https://control.example.test", token: TOKEN }, f as typeof fetch);

  it("client_rejects_non_get_and_non_allowlisted_paths", async () => {
    let calls = 0;
    const c = mk(async () => { calls++; return new Response("{}"); });
    for (const p of [
      "/api/v1/networks/n1/join-code", "/api/v1/networks/n1/events", "/api/v1/session", "/api/v1/device/config",
      "/api/v1/device/peers", "/api/v1/orgs/o1/service-tokens", "/api/v1/orgs/o1/members", "/healthz", "/auth/login", "/api/openapi.json",
      "https://evil.example.test/api/v1/orgs", "/api/v1/../x", "/api/v1/networks/a%2Fb/devices", "/api/v1/orgs/", "api/v1/orgs",
    ]) await expect(c.get(p), p).rejects.toThrow();
    expect(calls).toBe(0);
  });

  it("solo envía GET con Bearer y sin seguir redirecciones", async () => {
    let init: RequestInit | undefined;
    let url = "";
    const c = mk(async (u: string, i: RequestInit) => { url = u; init = i; return new Response('{"ok":1}'); });
    expect(await c.get("/api/v1/orgs?limit=5")).toEqual({ ok: 1 });
    expect(url).toBe("https://control.example.test/api/v1/orgs?limit=5");
    expect(init?.method).toBe("GET");
    expect((init?.headers as Record<string, string>).Authorization).toBe(`Bearer ${TOKEN}`);
    expect(init?.redirect).toBe("error");
  });

  it("token_never_appears_in_errors", async () => {
    const c1 = mk(async () => new Response(JSON.stringify({ error: { code: "invalid_session", message: TOKEN } }), { status: 401 }));
    const e1: ApiError = await c1.get("/api/v1/orgs").then(() => { throw new Error("debía fallar"); }, (e: ApiError) => e);
    expect(JSON.stringify(e1) + String(e1.message)).not.toContain(TOKEN);
    expect(e1).toMatchObject({ status: 401, code: "invalid_session" });
    const c2 = mk(async () => { throw new Error(`fallo con ${TOKEN}`); });
    const e2: ApiError = await c2.get("/api/v1/orgs").then(() => { throw new Error("debía fallar"); }, (e: ApiError) => e);
    expect(JSON.stringify(e2) + String(e2.message)).not.toContain(TOKEN);
    expect(e2).toMatchObject({ status: 0, code: "network" });
  });

  it("la lista permitida coincide con los GET de openapi.json", () => {
    const spec = JSON.parse(readFileSync(new URL("../../../docs/api/openapi.json", import.meta.url), "utf8"));
    const derived = Object.entries<Record<string, unknown>>(spec.paths)
      .filter(([p, v]) => "get" in v && p.startsWith("/api/v1/"))
      .map(([p]) => p)
      .filter((p) => !/join-code|events|session|^\/api\/v1\/device\/|service-tokens|members/.test(p))
      .sort();
    expect([...ALLOWED_TEMPLATES].sort()).toEqual(derived);
  });
});

describe("atenea_api_get", () => {
  it("api_get_truncates_at_64KiB", async () => {
    const out = await apiGet({ get: async () => ({ x: "y".repeat(100_000) }) } as never, { path: "/api/v1/orgs" });
    expect(out.truncated).toBe(true);
    expect(Buffer.byteLength((out as { text: string }).text)).toBeLessThanOrEqual(65536);
    const small = await apiGet({ get: async () => [1] } as never, { path: "/api/v1/orgs", query: { limit: 5 } });
    expect(small).toEqual({ truncated: false, data: [1] });
  });

  it("api_get_cleans_every_string_like_atenea_get_before_truncating", async () => {
    const hostile = "a\u202eb\u200fc\u061cd\u2067e\u0007f\u009fg";
    const out = await apiGet({ get: async () => [{ name: hostile, nested: { list: [hostile, 7, null] } }] } as never, { path: "/api/v1/orgs" });
    expect(out).toEqual({ truncated: false, data: [{ name: "abcdefg", nested: { list: ["abcdefg", 7, null] } }] });
    // Lo que `clean` quita no cuenta para el tope: 30 000 marcas bidi son 90 000 bytes en bruto y quedan en nada.
    const marks = await apiGet({ get: async () => ({ x: `${"\u200e".repeat(30_000)}ok` }) } as never, { path: "/api/v1/orgs" });
    expect(marks).toEqual({ truncated: false, data: { x: "ok" } });
  });
});

describe("loadConfig", () => {
  const home = mkdtempSync(join(tmpdir(), "atenea-ai-"));
  const dir = join(home, ".config", "ai-atenea-one");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, "config.toml");
  const body = `url = "https://control.example.test"\ntoken = "${TOKEN}"\n`;
  const msg = (f: () => unknown) => { try { f(); } catch (e) { return (e as Error).message; } return ""; };

  it("config_rejects_world_readable_file", () => {
    writeFileSync(file, body, { mode: 0o644 });
    const m = msg(() => loadConfig({}, home));
    expect(m).toMatch(/0600/);
    expect(m).not.toContain(TOKEN);
  });

  it("lee fichero 0600 y variables de entorno", () => {
    writeFileSync(file, body);
    chmodSync(file, 0o600);
    expect(loadConfig({}, home)).toEqual({ url: "https://control.example.test", token: TOKEN });
    const env = { ATENEA_AI_URL: "http://control.example.test/", ATENEA_AI_TOKEN: TOKEN };
    expect(loadConfig(env, "/nonexistent")).toEqual({ url: "http://control.example.test", token: TOKEN });
  });

  it("falla con mensaje claro si falta o el token no tiene forma de atsvc_", () => {
    expect(msg(() => loadConfig({}, "/nonexistent"))).toMatch(/ATENEA_AI_URL/);
    const bad = { ATENEA_AI_URL: "https://control.example.test", ATENEA_AI_TOKEN: "secreto-raro" };
    expect(msg(() => loadConfig(bad, "/x"))).toMatch(/atsvc_/);
    expect(msg(() => loadConfig(bad, "/x"))).not.toContain("secreto-raro");
  });
});
