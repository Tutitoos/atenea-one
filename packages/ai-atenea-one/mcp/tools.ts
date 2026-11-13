// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import { ApiError, type Getter } from "./client.ts";
import { KINDS, clean, loadInventory, shown, type Kind } from "./inventory.ts";
import { runProbe, type ProbeArgs, type ProbeDeps } from "./probe.ts";

const MAX_RESPONSE = 64 * 1024;
const MAX_LIMIT = 50;

export type SearchArgs = { query: string; kinds?: Kind[]; network?: string; limit?: number };

const field = (v: unknown): string | null => (typeof v === "string" ? shown(v) : null);

/** Todos los términos deben aparecer en el texto del recurso; devuelve solo campos mínimos y saneados. */
export async function search(client: Getter, { query, kinds, network, limit = 20 }: SearchArgs) {
  const recs = await loadInventory(client);
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const nets = network
    ? new Set(recs.filter((r) => r.kind === "network" && (r.id === network || r.name.toLowerCase() === network.toLowerCase())).map((r) => r.id))
    : undefined;
  return recs
    .filter((r) => (!kinds || kinds.includes(r.kind)) && (!nets || (r.network !== undefined && nets.has(r.network))))
    .filter((r) => terms.every((t) => r.text.includes(t)))
    .slice(0, Math.min(Math.max(limit, 1), MAX_LIMIT))
    .map((r) => ({
      id: r.id,
      kind: r.kind,
      network: r.network ?? null,
      name: shown(r.name),
      role: field(r.data.role),
      ipv4: field(r.data.ipv4),
      ipv6: field(r.data.ipv6),
      connection: field(r.data.connection),
      declared: r.declared,
    }));
}

/** Recurso completo tal como lo devolvió el control plane, con los textos saneados. */
export async function getResource(client: Getter, { kind, id }: { kind: Kind; id: string }) {
  const r = (await loadInventory(client)).find((x) => x.kind === kind && x.id === id);
  if (!r) throw new Error(`no encontrado: ${kind} ${shown(id)}`);
  const resource: unknown = JSON.parse(JSON.stringify(r.data, (_k, v: unknown) => (typeof v === "string" ? clean(v) : v)));
  return { kind: r.kind, network: r.network ?? null, declared: r.declared, resource };
}

/** GET libre sobre la lista permitida (la valida el cliente); textos saneados y respuesta acotada a 64 KiB. */
export async function apiGet(client: Getter, { path, query }: { path: string; query?: Record<string, string | number> }) {
  const qs = query ? new URLSearchParams(Object.entries(query).map(([k, v]) => [k, String(v)])).toString() : "";
  const data = await client.get(qs ? `${path}?${qs}` : path);
  const body = JSON.stringify(data, (_k, v: unknown) => (typeof v === "string" ? clean(v) : v));
  const bytes = Buffer.from(body);
  if (bytes.length <= MAX_RESPONSE) return { truncated: false as const, data: JSON.parse(body) as unknown };
  return { truncated: true as const, text: bytes.subarray(0, MAX_RESPONSE).toString() };
}

async function reply(run: () => Promise<unknown>) {
  try {
    return { content: [{ type: "text" as const, text: JSON.stringify(await run()) }] };
  } catch (e) {
    const error = e instanceof ApiError ? { status: e.status, code: e.code } : { message: e instanceof Error ? e.message : "error" };
    return { isError: true, content: [{ type: "text" as const, text: JSON.stringify({ error }) }] };
  }
}

export function registerTools(server: McpServer, client: Getter, probeDeps?: ProbeDeps) {
  const read = { readOnlyHint: true, openWorldHint: false };
  server.registerTool(
    "atenea_search",
    {
      description:
        "Busca en el inventario de Atenea One (organizaciones, redes, dispositivos, rutas, políticas, DNS). Todos los términos deben aparecer (nombre, hostname, rol, IP, prefijo, estado, conexión). Los campos de `declared` son declarados por el dispositivo, no verificados.",
      inputSchema: {
        query: z.string().min(1).max(200),
        kinds: z.array(z.enum(KINDS)).optional(),
        network: z.string().max(100).optional(),
        limit: z.number().int().min(1).max(MAX_LIMIT).optional(),
      },
      annotations: read,
    },
    (args) => reply(() => search(client, args)),
  );
  server.registerTool(
    "atenea_get",
    {
      description: "Recurso completo del inventario por tipo e id (el que devuelve atenea_search).",
      inputSchema: { kind: z.enum(KINDS), id: z.string().min(1).max(100) },
      annotations: read,
    },
    (args) => reply(() => getResource(client, args)),
  );
  server.registerTool(
    "atenea_api_get",
    {
      description:
        "GET de solo lectura a una ruta permitida de /api/v1 (p. ej. /api/v1/networks/{net}/audit, /metrics). Respuesta truncada a 64 KiB.",
      inputSchema: { path: z.string().min(1).max(300), query: z.record(z.string(), z.union([z.string(), z.number()])).optional() },
      annotations: read,
    },
    (args) => reply(() => apiGet(client, args)),
  );
  server.registerTool(
    "atenea_probe",
    {
      description:
        "Sonda de verificación contra UN dispositivo del inventario (nunca descubrimiento): ping, tcp, ssh-banner (sin autenticar) o dns (solo servidores DNS de la red). Un fallo no implica que el equipo esté caído.",
      inputSchema: {
        device: z.string().min(1).max(100),
        check: z.enum(["ping", "tcp", "ssh-banner", "dns"]),
        family: z.union([z.literal(4), z.literal(6)]).optional(),
        port: z.number().int().optional(),
        name: z.string().max(253).optional(),
      },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    (args: ProbeArgs) => reply(() => runProbe(client, args, probeDeps)),
  );
}
