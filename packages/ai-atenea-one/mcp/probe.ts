// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { execFile } from "node:child_process";
import { Resolver } from "node:dns/promises";
import { connect, isIP, type Socket } from "node:net";
import { promisify } from "node:util";
import type { Getter } from "./client.ts";
import { clean, loadInventory } from "./inventory.ts";

export type ProbeArgs = { device: string; check: "ping" | "tcp" | "ssh-banner" | "dns"; family?: 4 | 6; port?: number; name?: string };
export type ProbeResult = { check: string; target: string; ok: boolean; ms?: number; detail?: string };

/** Efectos inyectables para las pruebas; los valores por defecto son los reales. */
export type ProbeDeps = {
  platform: NodeJS.Platform;
  exec: (file: string, args: string[]) => Promise<{ stdout: string }>;
  connect: (host: string, port: number) => Socket;
  resolve: (servers: string[], name: string, family: 4 | 6) => Promise<string[]>;
};

const run = promisify(execFile);
const defaults: ProbeDeps = {
  platform: process.platform,
  exec: (file, args) => run(file, args, { timeout: 15_000, windowsHide: true }),
  connect: (host, port) => connect({ host, port }),
  async resolve(servers, name, family) {
    const r = new Resolver({ timeout: 3000, tries: 1 });
    r.setServers(servers);
    return family === 6 ? r.resolve6(name) : r.resolve4(name);
  },
};

const TIMEOUT_MS = 3000;
const BANNER_MAX = 255;
const NAME_RE = /^[A-Za-z0-9_][A-Za-z0-9._-]{0,252}$/;

// Una sonda a la vez: se encadenan, no se rechazan.
let queue: Promise<unknown> = Promise.resolve();

export function runProbe(client: Getter, args: ProbeArgs, deps: ProbeDeps = defaults): Promise<ProbeResult> {
  const result = queue.then(() => probe(client, args, deps));
  queue = result.catch(() => undefined);
  return result;
}

/** Abre TCP a (host, port): resuelve en `connect`, en el primer dato o al agotar los 3 s. No escribe nunca. */
function tcp(deps: ProbeDeps, host: string, port: number, banner: boolean): Promise<{ ok: boolean; detail?: string }> {
  const { promise, resolve } = Promise.withResolvers<{ ok: boolean; detail?: string }>();
  const sock = deps.connect(host, port);
  let buf = Buffer.alloc(0);
  const end = (ok: boolean, detail?: string) => {
    clearTimeout(timer);
    sock.destroy();
    resolve({ ok, detail });
  };
  const timer = setTimeout(() => end(false, "timeout"), TIMEOUT_MS);
  sock.on("error", (e: NodeJS.ErrnoException) => end(false, e.code ?? "error"));
  if (!banner) sock.on("connect", () => end(true));
  else
    sock.on("data", (chunk: Buffer) => {
      buf = Buffer.concat([buf, chunk]).subarray(0, BANNER_MAX);
      const nl = buf.indexOf(10);
      if (nl < 0 && buf.length < BANNER_MAX) return;
      const line = clean(buf.subarray(0, nl < 0 ? buf.length : nl).toString("latin1").replace(/\r$/, ""));
      end(line.startsWith("SSH-"), line);
    });
  return promise;
}

async function probe(client: Getter, { device, check, family, port, name }: ProbeArgs, deps: ProbeDeps): Promise<ProbeResult> {
  if (check === "tcp" && !(Number.isInteger(port) && port! >= 1 && port! <= 65535)) throw new Error("port debe ser un entero entre 1 y 65535");
  if (check === "dns" && !(name && NAME_RE.test(name))) throw new Error("name inválido para la consulta DNS");

  // El destino solo sale del inventario: id o nombre exacto, nunca texto libre.
  const recs = await loadInventory(client);
  const found = recs.filter((r) => r.kind === "device" && (r.id === device || r.name === device));
  if (found.length === 0) throw new Error("dispositivo no encontrado en el inventario");
  if (found.length > 1) throw new Error("nombre ambiguo: usa el id del dispositivo");
  const dev = found[0];
  const fam = family ?? (typeof dev.data.ipv4 === "string" ? 4 : 6);
  const addr = dev.data[fam === 4 ? "ipv4" : "ipv6"];
  const t0 = performance.now();
  const ms = () => Math.round(performance.now() - t0);

  if (check === "dns") {
    const servers = recs.find((r) => r.kind === "dns" && r.id === dev.network)?.data.servers;
    const list = Array.isArray(servers) ? servers.filter((s): s is string => typeof s === "string" && isIP(s) !== 0) : [];
    if (list.length === 0) throw new Error("la red no tiene servidores DNS");
    try {
      const answers = await deps.resolve(list, name!, fam);
      return { check, target: list.join(","), ok: answers.length > 0, ms: ms(), detail: answers.slice(0, 8).join(",") };
    } catch (e) {
      return { check, target: list.join(","), ok: false, ms: ms(), detail: (e as NodeJS.ErrnoException).code ?? "error" };
    }
  }

  if (typeof addr !== "string" || isIP(addr) !== fam) throw new Error(`el dispositivo no tiene dirección IPv${fam} válida en el túnel`);

  if (check === "ping") {
    const win = deps.platform === "win32";
    if (win) throw new Error("ping no está soportado en Windows");
    const mac = deps.platform === "darwin";
    // BSD/macOS: -W en milisegundos y ping6 para IPv6; Linux: -W en segundos y -6.
    const [file, argv] =
      fam === 6 && mac ? ["ping6", ["-c", "3", addr]] : [ "ping", [...(fam === 6 ? ["-6"] : []), "-c", "3", "-W", mac ? "2000" : "2", addr]];
    try {
      const { stdout } = await deps.exec(file as string, argv as string[]);
      return { check, target: addr, ok: true, ms: ms(), detail: clean(stdout.trim().split("\n").pop() ?? "").slice(0, 200) };
    } catch {
      return { check, target: addr, ok: false, ms: ms(), detail: "sin respuesta" };
    }
  }

  const r = await tcp(deps, addr, check === "tcp" ? port! : 22, check === "ssh-banner");
  return { check, target: addr, ok: r.ok, ms: ms(), ...(r.detail ? { detail: r.detail } : {}) };
}
