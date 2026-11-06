// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useSyncExternalStore } from 'react';
import type { Rate } from './agent';
import { EMPTY_EVENTS, getEvents, subscribeEvents, type StoredEvent } from './history-store';

export type DeviceHistory = { network: string; id: string; path: string; points: (Rate & { at: number })[] };
/** `transitions`: cambios de camino detectados en ESTA lectura; el que llama los persiste. */
export type History = { network: string; devices: Record<string, DeviceHistory>; transitions: (StoredEvent & { key: string })[] };

export const eventText = (to: string) => (to === 'direct' ? 'Se conectó directamente' : to === 'relay' ? 'Pasó a relay' : to === 'none' ? 'Quedó Inactivo' : to === 'absent' ? 'Se desconectó' : 'Conectando…');

/** Muestras en memoria (una red, diez minutos de tasas); las transiciones de camino se devuelven para persistirlas. */
export function recordHistory(previous: History | undefined, network: string, peers: { public_key: string; path: string }[], rates: Record<string, Rate>, now: number): History {
  const old = previous?.network === network ? previous : undefined;
  const paths = new Map(peers.map((peer) => [peer.public_key, peer.path.startsWith('relay') ? 'relay' : peer.path]));
  const devices: History['devices'] = {};
  const transitions: History['transitions'] = [];
  for (const key of new Set([...Object.keys(old?.devices ?? {}), ...paths.keys()])) {
    const before = old?.devices[key];
    const path = paths.get(key) ?? 'absent';
    // Ver un par por primera vez no es una transición: no hay camino anterior.
    if (before && before.path !== path) transitions.push({ key, at: now, from: before.path, to: path });
    const points = (before?.points ?? []).filter((point) => now - point.at < 600000);
    if (rates[key] && points.at(-1)?.at !== now) points.push({ at: now, ...rates[key] });
    devices[key] = { network, id: key, path, points: points.slice(-300) };
  }
  return { network, devices, transitions };
}

/** Eventos persistidos del par, del más antiguo al más reciente; se actualiza al registrarse uno nuevo. */
export function useDeviceEvents(network?: string, deviceId?: string): StoredEvent[] {
  return useSyncExternalStore(subscribeEvents, () => (network && deviceId ? getEvents(network, deviceId) : EMPTY_EVENTS));
}
