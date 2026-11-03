// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { EventEmitter } from "node:events";
import type { Socket } from "node:net";
import { afterEach, describe, expect, it, vi } from "vitest";
import { runProbe, type ProbeDeps } from "../mcp/probe.ts";
import { D1, D2, FakeClient, NET1, dev } from "./fixtures.ts";

afterEach(() => vi.useRealTimers());

class FakeSocket extends EventEmitter {
  write = vi.fn();
  destroy = vi.fn();
}

function deps(over: Partial<ProbeDeps> = {}) {
  const sock = new FakeSocket();
  const d = {
    platform: "linux" as NodeJS.Platform,
    exec: vi.fn(async () => ({ stdout: "3 packets transmitted, 3 received\nrtt min/avg/max = 1/2/3 ms\n" })),
    connect: vi.fn(() => sock as unknown as Socket),
    resolve: vi.fn(async () => ["10.77.0.50"]),
    ...over,
  };
  return { d, sock };
}

describe("atenea_probe", () => {
  it("rejects_device_not_in_inventory", async () => {
    const { d } = deps();
    await expect(runProbe(new FakeClient(), { device: "no-existe", check: "ping" }, d)).rejects.toThrow(/inventario/);
    expect(d.exec).not.toHaveBeenCalled();
  });

  it("rejects_address_not_from_inventory_even_if_valid_ip", async () => {
    const { d } = deps();
    for (const device of ["10.77.0.99", "10.77.0.2;id", "-c", "192.0.2.1 -f"]) {
      await expect(runProbe(new FakeClient(), { device, check: "ping" }, d), device).rejects.toThrow(/inventario/);
    }
    expect(d.exec).not.toHaveBeenCalled();
    expect(d.connect).not.toHaveBeenCalled();
  });

  it("target_is_resolved_only_from_tunnel_addrs", async () => {
    const { d } = deps();
    const c = new FakeClient({ [`/api/v1/networks/${NET1}/devices`]: [dev(D1, { name: "srv", hostname: "evil.example.test", ipv4: "10.77.0.2", ipv6: "fd00:77::2" })] });
    expect((await runProbe(c, { device: "srv", check: "ping" }, d)).target).toBe("10.77.0.2");
    expect((await runProbe(c, { device: D1, check: "ping", family: 6 }, d)).target).toBe("fd00:77::2");
    // un valor de IP manipulado en el inventario tampoco se usa
    const bad = new FakeClient({ [`/api/v1/networks/${NET1}/devices`]: [dev(D1, { name: "srv", ipv4: "-f; reboot" })] });
    await expect(runProbe(bad, { device: "srv", check: "ping" }, d)).rejects.toThrow(/dirección/);
  });

  it("rejects ambiguous names", async () => {
    const c = new FakeClient({ [`/api/v1/networks/${NET1}/devices`]: [dev(D1, { name: "dup", ipv4: "10.77.0.2" }), dev(D2, { name: "dup", ipv4: "10.77.0.3" })] });
    await expect(runProbe(c, { device: "dup", check: "ping" }, deps().d)).rejects.toThrow(/ambig/);
  });

  it("ping_uses_fixed_argv_without_shell", async () => {
    const { d } = deps();
    const r = await runProbe(new FakeClient(), { device: D1, check: "ping" }, d);
    expect(d.exec).toHaveBeenCalledWith("ping", ["-c", "3", "-W", "2", "10.77.0.2"]);
    expect(r).toMatchObject({ check: "ping", target: "10.77.0.2", ok: true });
    await runProbe(new FakeClient(), { device: D1, check: "ping", family: 6 }, d);
    expect(d.exec).toHaveBeenLastCalledWith("ping", ["-6", "-c", "3", "-W", "2", "fd00:77::2"]);
  });

  it("ping falla sin lanzar cuando el ejecutor devuelve error", async () => {
    const { d } = deps({ exec: vi.fn(async () => { throw new Error("exit 1"); }) });
    expect(await runProbe(new FakeClient(), { device: D1, check: "ping" }, d)).toMatchObject({ ok: false });
  });

  it("tcp_times_out_at_3s", async () => {
    vi.useFakeTimers();
    const { d, sock } = deps();
    const p = runProbe(new FakeClient(), { device: D1, check: "tcp", port: 22 }, d);
    await vi.advanceTimersByTimeAsync(2_999);
    expect(sock.destroy).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(await p).toMatchObject({ check: "tcp", ok: false, detail: "timeout" });
    expect(d.connect).toHaveBeenCalledWith("10.77.0.2", 22);
  });

  it("tcp ok al conectar", async () => {
    const { d, sock } = deps();
    const p = runProbe(new FakeClient(), { device: D1, check: "tcp", port: 22 }, d);
    await vi.waitFor(() => expect(d.connect).toHaveBeenCalled());
    sock.emit("connect");
    expect(await p).toMatchObject({ ok: true });
  });

  it("ssh_banner_reads_only_first_line_max_255_bytes_and_never_authenticates", async () => {
    const a = deps();
    const p = runProbe(new FakeClient(), { device: D1, check: "ssh-banner" }, a.d);
    await vi.waitFor(() => expect(a.d.connect).toHaveBeenCalledWith("10.77.0.2", 22));
    a.sock.emit("data", Buffer.from("SSH-2.0-OpenSSH_9.9\r\nsegunda línea\n"));
    expect(await p).toMatchObject({ ok: true, detail: "SSH-2.0-OpenSSH_9.9" });
    expect(a.sock.write).not.toHaveBeenCalled();
    expect(a.sock.destroy).toHaveBeenCalled();

    const b = deps();
    const q = runProbe(new FakeClient(), { device: D1, check: "ssh-banner" }, b.d);
    await vi.waitFor(() => expect(b.d.connect).toHaveBeenCalled());
    b.sock.emit("data", Buffer.alloc(400, "A"));
    const r = await q;
    expect(r.detail?.length).toBeLessThanOrEqual(255);
    expect(r.ok).toBe(false);
    expect(b.sock.write).not.toHaveBeenCalled();
  });

  it("dns_queries_only_network_dns_servers", async () => {
    const { d } = deps();
    const r = await runProbe(new FakeClient(), { device: D1, check: "dns", name: "srv1.lab.internal" }, d);
    expect(d.resolve).toHaveBeenCalledWith(["10.77.0.1"], "srv1.lab.internal", 4);
    expect(r).toMatchObject({ check: "dns", target: "10.77.0.1", ok: true });
    await expect(runProbe(new FakeClient(), { device: D1, check: "dns", name: "-x; id" }, d)).rejects.toThrow(/name/);
    await expect(runProbe(new FakeClient(), { device: "servidor-dos", check: "dns", name: "a.example.test" }, d)).rejects.toThrow(/DNS/);
  });

  it("port_out_of_range_rejected", async () => {
    const { d } = deps();
    for (const port of [0, 65536, -1, 1.5, undefined]) {
      await expect(runProbe(new FakeClient(), { device: D1, check: "tcp", port }, d), String(port)).rejects.toThrow(/port/);
    }
    expect(d.connect).not.toHaveBeenCalled();
  });
});
