// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Getter } from "./client.ts";

export const KINDS = ["org", "network", "device", "route", "policy", "dns"] as const;
export type Kind = (typeof KINDS)[number];

/** Recurso del inventario con el texto buscable ya saneado. `data` es la respuesta original del control plane. */
export type InvRecord = {
  kind: Kind;
  id: string;
  network?: string;
  name: string;
  text: string;
  declared: string[];
  data: Record<string, unknown>;
};

const CACHE_MS = 30_000;
const PROTO: Record<number, string> = { 0: "any", 1: "icmp", 6: "tcp", 17: "udp", 58: "icmpv6" };

// Controles C0/C1 y marcas/aislamientos bidireccionales: un nombre hostil no debe reordenar ni ocultar texto.
export const clean = (s: string): string => s.replace(/[\u0000-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "");
/** Texto no confiable para mostrar: saneado y a 64 caracteres como máximo. */
export const shown = (s: string): string => Array.from(clean(s)).slice(0, 64).join("");

const rows = (x: unknown): Record<string, unknown>[] =>
  Array.isArray(x) ? x.filter((r): r is Record<string, unknown> => typeof r === "object" && r !== null) : [];
const str = (v: unknown): string => (typeof v === "string" || typeof v === "number" ? String(v) : "");
const sel = (v: unknown): string => {
  const r = rows([v])[0] ?? {};
  return r.kind === "device" ? `device:${str(r.device_id)}` : r.kind === "prefix" ? `prefix:${str(r.prefix)}` : "any";
};

async function build(client: Getter): Promise<InvRecord[]> {
  const out: InvRecord[] = [];
  const add = (r: Omit<InvRecord, "text"> & { text: string[] }) => out.push({ ...r, text: clean(r.text.join(" ")).toLowerCase() });
  for (const org of rows(await client.get("/api/v1/orgs"))) {
    const oid = str(org.id);
    add({ kind: "org", id: oid, name: str(org.name), text: [str(org.name), str(org.role)], declared: [], data: org });
    const nets = rows(await client.get(`/api/v1/orgs/${oid}/networks`));
    await Promise.all(
      nets.map(async (net) => {
        const nid = str(net.id);
        const nname = str(net.name);
        const [devices, routes, policies, dns] = await Promise.all([
          client.get(`/api/v1/networks/${nid}/devices`),
          client.get(`/api/v1/networks/${nid}/routes`),
          client.get(`/api/v1/networks/${nid}/policies`),
          client.get(`/api/v1/networks/${nid}/dns`),
        ]);
        add({ kind: "network", id: nid, network: nid, name: nname, text: [nname, str(net.ipv4_cidr), str(net.ipv6_cidr), str(net.join_policy)], declared: [], data: net });
        const names = new Map<string, string>();
        for (const d of rows(devices)) {
          names.set(str(d.id), str(d.name));
          add({
            kind: "device", id: str(d.id), network: nid, name: str(d.name), declared: ["name", "hostname", "role", "report", "connection"], data: d,
            // «servidor» es el rol `server` (valor declarado, no firmado): alias en español para la búsqueda.
            text: [str(d.name), str(d.hostname), str(d.role), d.role === "server" ? "servidor servidores servers" : "", str(d.ipv4), str(d.ipv6), str(d.status), str(d.connection), nname],
          });
        }
        for (const r of rows(routes)) {
          add({ kind: "route", id: str(r.id), network: nid, name: str(r.prefix), declared: ["prefix"], data: r, text: [str(r.prefix), str(r.status), names.get(str(r.device_id)) ?? "", "route"] });
        }
        for (const p of rows(policies)) {
          const name = `${sel(p.src)} -> ${sel(p.dst)} ${PROTO[Number(p.proto)] ?? str(p.proto)} ${str(p.port_min)}-${str(p.port_max)}`;
          add({ kind: "policy", id: str(p.id), network: nid, name, declared: [], data: p, text: [name, "policy", nname] });
        }
        const d = rows([dns])[0] ?? {};
        const servers = Array.isArray(d.servers) ? d.servers.map(str) : [];
        const domains = Array.isArray(d.domains) ? d.domains.map(str) : [];
        add({ kind: "dns", id: nid, network: nid, name: `dns ${nname}`, declared: [], data: d, text: ["dns", nname, ...servers, ...domains] });
      }),
    );
  }
  return out;
}

const cache = new WeakMap<Getter, { at: number; recs: Promise<InvRecord[]> }>();

/** Inventario completo (orgs → redes → dispositivos, rutas, políticas, DNS) con caché de 30 s por cliente. */
export function loadInventory(client: Getter): Promise<InvRecord[]> {
  const hit = cache.get(client);
  if (hit && Date.now() - hit.at < CACHE_MS) return hit.recs;
  const recs = build(client);
  cache.set(client, { at: Date.now(), recs });
  recs.catch(() => cache.delete(client));
  return recs;
}
