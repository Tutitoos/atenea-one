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
  probing: ['Conectando…', 'off'],
  none: ['Inactivo', 'off'],
};
export const pathLabel = (path: string): { label: string; tone: Tone } => {
  const [label, tone] = PATHS[path] ?? [path, 'off' as Tone];
  return { label, tone };
};

export const levelTone = (level: Level): Tone => ({ ok: 'ok', degraded: 'warn', down: 'bad' } as const)[level];

/** Alias local > nombre de la red (el del sistema o el fijado por un administrador) > IP del túnel > clave abreviada. */
export const peerName = (peer: Peer, names: Record<string, string>) => names[peer.public_key] || peer.name || peer.addrs[0] || peer.public_key.slice(0, 8);

/** Este dispositivo: alias local > nombre del sistema > texto genérico. */
export const selfName = (status: { public_key: string; hostname?: string | null }, names: Record<string, string>) => names[status.public_key] || status.hostname || 'Este dispositivo';

/** Lectura segura de campos de objetos sin esquema fijo (entradas de relay). */
export const text = (v: unknown) => (typeof v === 'string' ? v : undefined);
export const num = (v: unknown) => (typeof v === 'number' ? v : 0);


/** El servidor es el control plane (sincronización) más el coordinator; sin sincronización configurada no hay servidor. */
export function serverTone(status: { sync?: { last_error?: string | null } | null; coordinator?: { last_error?: string | null } | null }): Tone {
  if (status.sync?.last_error) return 'bad';
  return status.coordinator?.last_error ? 'warn' : 'ok';
}

