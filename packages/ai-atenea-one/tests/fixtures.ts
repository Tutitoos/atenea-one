// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// Inventario sintético (ejemplos, nunca datos reales) servido por un cliente falso.
export const ORG = "00000000-0000-0000-0000-0000000000a1";
export const NET1 = "00000000-0000-0000-0000-0000000000b1";
export const NET2 = "00000000-0000-0000-0000-0000000000b2";
export const D1 = "00000000-0000-0000-0000-0000000000c1";
export const D2 = "00000000-0000-0000-0000-0000000000c2";
export const D3 = "00000000-0000-0000-0000-0000000000c3";
export const D4 = "00000000-0000-0000-0000-0000000000c4";

export const dev = (id: string, o: Record<string, unknown>) => ({
  id,
  device_id: id,
  role: "device",
  name: "equipo-ejemplo",
  name_source: "device",
  hostname: null,
  status: "approved",
  join_origin: "token",
  endpoints: [],
  fingerprint: "ab:cd",
  ipv4: null,
  ipv6: null,
  enrolled_at: "2026-11-01T00:00:00Z",
  approved_at: null,
  revoked_at: null,
  last_config_at: null,
  connection: "unknown",
  report: null,
  ...o,
});

export const HOSTILE = `\u202Eevil${"x".repeat(100)}`;

export const data: Record<string, unknown> = {
  "/api/v1/orgs": [{ id: ORG, name: "org-ejemplo", role: "owner" }],
  [`/api/v1/orgs/${ORG}/networks`]: [
    { id: NET1, org_id: ORG, name: "red-ejemplo", ipv4_cidr: "10.77.0.0/24", ipv6_cidr: "fd00:77::/64", join_policy: "cerrada" },
    { id: NET2, org_id: ORG, name: "red-dos", ipv4_cidr: "192.0.2.0/24", ipv6_cidr: "fd00:78::/64", join_policy: "con_solicitud" },
  ],
  [`/api/v1/networks/${NET1}/devices`]: [
    dev(D1, { name: "servidor-ejemplo", hostname: "srv1", role: "server", ipv4: "10.77.0.2", ipv6: "fd00:77::2", connection: "online" }),
    dev(D2, { ipv4: "10.77.0.3", ipv6: "fd00:77::3", connection: "stale" }),
    dev(D3, { name: HOSTILE, ipv4: "10.77.0.4", ipv6: "fd00:77::4" }),
  ],
  [`/api/v1/networks/${NET2}/devices`]: [
    dev(D4, { name: "servidor-dos", role: "server", ipv4: "192.0.2.2", ipv6: "fd00:78::2", connection: "offline" }),
  ],
  [`/api/v1/networks/${NET1}/routes`]: [
    { id: "00000000-0000-0000-0000-0000000000d1", device_id: D1, prefix: "198.51.100.0/24", status: "approved", snat: true, declared_at: "2026-11-01T00:00:00Z", decided_at: null },
  ],
  [`/api/v1/networks/${NET2}/routes`]: [],
  [`/api/v1/networks/${NET1}/policies`]: [
    { id: "00000000-0000-0000-0000-0000000000e1", src: { kind: "any" }, dst: { kind: "device", device_id: D1 }, proto: 6, port_min: 22, port_max: 22 },
  ],
  [`/api/v1/networks/${NET2}/policies`]: [],
  [`/api/v1/networks/${NET1}/dns`]: { servers: ["10.77.0.1"], domains: ["lab.internal"] },
  [`/api/v1/networks/${NET2}/dns`]: { servers: [], domains: [] },
};

/** Cliente falso de solo GET; `overrides` sustituye respuestas sin tocar el conjunto base. */
export class FakeClient {
  calls: string[] = [];
  constructor(private overrides: Record<string, unknown> = {}) {}
  async get(path: string): Promise<unknown> {
    this.calls.push(path);
    const all = { ...data, ...this.overrides };
    if (!(path in all)) throw new Error(`sin datos sintéticos: ${path}`);
    return all[path];
  }
}
