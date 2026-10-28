// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Rate } from './agent';

export type DeviceHistory = { path: string; points: (Rate & { at: number })[]; events: { at: number; text: string }[] };
export type History = { network: string; devices: Record<string, DeviceHistory> };

/** Historial efímero: una red seleccionada, diez minutos de tasas y veinte transiciones por par. */
export function recordHistory(previous: History | undefined, network: string, peers: { public_key: string; path: string }[], rates: Record<string, Rate>, now: number): History {
  const old = previous?.network === network ? previous : undefined;
  const paths = new Map(peers.map((peer) => [peer.public_key, peer.path.startsWith('relay') ? 'relay' : peer.path]));
  const devices: History['devices'] = {};
  for (const key of new Set([...Object.keys(old?.devices ?? {}), ...paths.keys()])) {
    const before = old?.devices[key];
    const path = paths.get(key) ?? 'absent';
    const events = [...(before?.events ?? [])];
    if (old && before?.path !== path) {
      const text = path === 'direct' ? 'Se conectó directamente' : path === 'relay' ? 'Pasó a relay' : path === 'none' ? 'Quedó Inactivo' : path === 'absent' ? 'Se desconectó' : 'Conectando…';
      events.push({ at: now, text });
    }
    const points = (before?.points ?? []).filter((point) => now - point.at < 600000);
    if (rates[key] && points.at(-1)?.at !== now) points.push({ at: now, ...rates[key] });
    devices[key] = { path, events: events.slice(-20), points: points.slice(-300) };
  }
  return { network, devices };
}
