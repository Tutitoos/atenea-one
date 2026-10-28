// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { invoke } from '@tauri-apps/api/core';
import { reply, type Reply } from './status';

export type Rate = { down: number; up: number };
export type Sample = { reply: Reply; rates: Record<string, Rate> };

/** Calcula bytes/s por par entre dos lecturas consecutivas (la primera no tiene tasa). */
export function createSampler() {
  let prev: { at: number; bytes: Map<string, { rx: number; tx: number }> } | undefined;
  return (current: Reply, now: number): Sample => {
    const dt = prev ? (now - prev.at) / 1000 : 0;
    const rates: Record<string, Rate> = {};
    const bytes = new Map<string, { rx: number; tx: number }>();
    for (const p of current.status.peers) {
      const old = prev?.bytes.get(p.public_key);
      if (old && dt > 0) rates[p.public_key] = { down: Math.max(0, p.rx_bytes - old.rx) / dt, up: Math.max(0, p.tx_bytes - old.tx) / dt };
      bytes.set(p.public_key, { rx: p.rx_bytes, tx: p.tx_bytes });
    }
    prev = { at: now, bytes };
    return { reply: current, rates };
  };
}

/** Un muestreador por red: al cambiar de red no se calculan tasas contra la lectura de otra. */
const samplers = new Map<string, ReturnType<typeof createSampler>>();

/** Lee el estado sin modificar los contadores del muestreador de tráfico. */
export async function readReply(network: string): Promise<Reply> {
  let raw: string;
  try {
    raw = await invoke<string>('status', { network });
  } catch (e) {
    // Tauri rechaza con el texto devuelto por la orden, no con un Error.
    throw new Error(typeof e === 'string' ? e : 'No se pudo leer el estado del agente.', { cause: e });
  }
  const parsed = reply.safeParse(JSON.parse(raw));
  if (!parsed.success) throw new Error('El agente respondió con un formato inesperado.');
  return parsed.data;
}

/** Lectura y muestreo por red: solo la vista de dispositivos actualiza sus tasas. */
export async function readAgent(network: string): Promise<Sample> {
  const current = await readReply(network);
  if (!samplers.has(network)) samplers.set(network, createSampler());
  return samplers.get(network)!(current, Date.now());
}
