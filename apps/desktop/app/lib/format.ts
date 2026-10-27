// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Level, Peer } from './status';

export type Tone = 'ok' | 'warn' | 'bad' | 'off';

export function bytes(n: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(i ? 1 : 0)} ${units[i]}`;
}

export function ago(secs: number): string {
  return secs < 120 ? `${Math.round(secs)} s` : secs < 7200 ? `${Math.round(secs / 60)} min` : `${Math.round(secs / 3600)} h`;
}

export const ipKind = (addr: string) => (addr.includes(':') ? 'IPv6' : 'IPv4');

const PATHS: Record<string, [string, Tone]> = {
  direct: ['Directo', 'ok'],
  'relay-udp': ['Por relay UDP', 'warn'],
  'relay-tls': ['Por relay TLS', 'warn'],
  probing: ['Sondeando', 'warn'],
  none: ['Sin camino', 'bad'],
};
export const pathLabel = (path: string): { label: string; tone: Tone } => {
  const [label, tone] = PATHS[path] ?? [path, 'off' as Tone];
  return { label, tone };
};

export const levelTone = (level: Level): Tone => ({ ok: 'ok', degraded: 'warn', down: 'bad' } as const)[level];

/** Nombre local si existe; si no, la IP del túnel y, en último caso, la clave abreviada. */
export const peerName = (peer: Peer, names: Record<string, string>) => names[peer.public_key] || peer.addrs[0] || peer.public_key.slice(0, 8);
