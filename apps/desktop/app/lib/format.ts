// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import type { Level, Peer } from './status';

export type Tone = 'ok' | 'warn' | 'bad' | 'off';

const decimal = (n: number) => n.toLocaleString('es', { minimumFractionDigits: 1, maximumFractionDigits: 1 });

/** Número entero con separador de miles en español. */
export const int = (n: number) => (Number.isFinite(n) ? Math.round(n).toLocaleString('es') : '—');

/** Porcentaje con un decimal y coma: `pct(1, 540)` → «0,2 %». Sin muestras no hay porcentaje. */
export const pct = (part: number, total: number) => (total > 0 && part >= 0 ? `${decimal(part / total * 100)} %` : '—');

export function bytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${i ? decimal(n) : int(n)} ${units[i]}`;
}

/** Antigüedad abreviada; trunca (nunca adelanta un límite) y pasa a días a partir de 48 h. */
export function ago(secs: number): string {
  if (!Number.isFinite(secs)) return '—';
  const s = Math.floor(Math.max(0, secs));
  return s < 120 ? `${s} s` : s < 7200 ? `${Math.floor(s / 60)} min` : s < 172800 ? `${Math.floor(s / 3600)} h` : `${Math.floor(s / 86400)} d`;
}

/** Resumen de la red: tasa total y reparto de pares por tipo de camino (`none`/`probing` cuentan como otros). */
export function networkSummary(peers: { public_key: string; path: string }[], rates: Record<string, { down: number; up: number }>) {
  const s = { down: 0, up: 0, peers: peers.length, direct: 0, relay: 0, other: 0 };
  for (const p of peers) {
    s.down += rates[p.public_key]?.down ?? 0; s.up += rates[p.public_key]?.up ?? 0;
    if (p.path === 'direct') s.direct++; else if (p.path.startsWith('relay')) s.relay++; else s.other++;
  }
  return s;
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

