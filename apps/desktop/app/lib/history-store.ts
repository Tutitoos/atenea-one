// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { z } from 'zod';

// Solo transiciones de camino (red, clave pública del par, instante, camino anterior y nuevo): ni tráfico ni nombres.
const KEY = 'atenea.history.v1';
export const MAX_EVENTS_PER_NETWORK = 200;
export const MAX_AGE_MS = 30 * 24 * 3600 * 1000;
export const SAVE_DELAY_MS = 2000;

export type StoredEvent = { at: number; from: string; to: string };
type Data = Record<string, Record<string, StoredEvent[]>>;
const schema = z.object({ v: z.literal(1), data: z.record(z.string(), z.record(z.string(), z.array(z.object({ at: z.number(), from: z.string(), to: z.string() })))) });

export const EMPTY_EVENTS: StoredEvent[] = [];
let data: Data | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();

/** Descarta lo anterior a 30 días y conserva solo los 200 más recientes de cada red. */
function prune(input: Data, now: number): Data {
  const out: Data = {};
  for (const [network, devices] of Object.entries(input)) {
    const all = Object.entries(devices).flatMap(([key, events]) => events.filter((e) => now - e.at < MAX_AGE_MS).map((e) => ({ key, e })));
    all.sort((a, b) => a.e.at - b.e.at);
    const net: Record<string, StoredEvent[]> = {};
    for (const { key, e } of all.slice(-MAX_EVENTS_PER_NETWORK)) (net[key] ??= []).push(e);
    if (all.length) out[network] = net;
  }
  return out;
}

function load(): Data {
  if (!data) {
    data = {};
    try {
      const raw = localStorage.getItem(KEY);
      // JSON corrupto o versión desconocida: se empieza vacío (la siguiente escritura lo sustituye).
      const parsed = raw ? schema.safeParse(JSON.parse(raw)) : undefined;
      if (parsed?.success) data = prune(parsed.data.data, Date.now());
    } catch { /* almacenamiento no disponible o JSON ilegible */ }
  }
  return data;
}

/** Escribe ya lo pendiente. Una cuota llena o un almacenamiento bloqueado no es un fallo de la app. */
export function flushHistory() {
  if (timer === undefined) return;
  clearTimeout(timer);
  timer = undefined;
  try { localStorage.setItem(KEY, JSON.stringify({ v: 1, data: load() })); } catch { /* QuotaExceededError u otro: se pierde la persistencia, no la sesión */ }
}

export const getEvents = (network: string, key: string) => load()[network]?.[key] ?? EMPTY_EVENTS;

export function recordEvents(network: string, transitions: { key: string; at: number; from: string; to: string }[]) {
  if (!transitions.length) return;
  const next: Data = { ...load(), [network]: { ...load()[network] } };
  for (const { key, ...event } of transitions) next[network][key] = [...(next[network][key] ?? []), event];
  data = prune(next, Date.now());
  listeners.forEach((l) => l());
  // Escritura diferida: la app lee cada 2 s y no debe escribir en cada lectura.
  timer ??= setTimeout(flushHistory, SAVE_DELAY_MS);
}

export function subscribeEvents(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** Solo para tests: olvida la caché y cualquier escritura pendiente. */
export function resetHistoryStore() {
  clearTimeout(timer);
  timer = undefined;
  data = undefined;
  listeners.clear();
}

if (typeof window !== 'undefined') window.addEventListener('pagehide', flushHistory);
