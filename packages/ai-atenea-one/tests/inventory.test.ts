// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadInventory } from "../mcp/inventory.ts";
import { FakeClient, NET1 } from "./fixtures.ts";

afterEach(() => vi.useRealTimers());

describe("loadInventory", () => {
  it("recorre orgs, redes y recursos de cada red", async () => {
    const recs = await loadInventory(new FakeClient());
    const kinds = new Set(recs.map((r) => r.kind));
    expect([...kinds].sort()).toEqual(["device", "dns", "network", "org", "policy", "route"]);
    expect(recs.find((r) => r.kind === "device")?.network).toBe(NET1);
  });

  it("cachea 30 s en memoria", async () => {
    vi.useFakeTimers();
    const c = new FakeClient();
    await loadInventory(c);
    const n = c.calls.length;
    await loadInventory(c);
    expect(c.calls.length).toBe(n);
    vi.advanceTimersByTime(31_000);
    await loadInventory(c);
    expect(c.calls.length).toBe(2 * n);
  });
});
