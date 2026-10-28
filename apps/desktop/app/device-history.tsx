// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { ago, bytes } from './lib/format';
import type { DeviceHistory } from './lib/history';
import { Heading } from './ui';

export function TrafficHistory({ history }: { history?: DeviceHistory }) {
  const points = history?.points ?? [];
  const max = Math.max(1, ...points.flatMap((p) => [p.down, p.up]));
  const end = points.at(-1)?.at ?? Date.now();
  return <>
    <Heading>Tráfico · últimos 10 minutos</Heading>
    <p className="text-sm text-mut">Bajada (línea continua) · Subida (línea discontinua) · escala 0–{bytes(max)}/s</p>
    {points.length ? <svg role="img" aria-label="Velocidad de bajada y subida en bytes por segundo durante los últimos diez minutos" viewBox="0 0 600 140" className="mt-2 w-full rounded-lg bg-card">
      <title>Tráfico del dispositivo</title>
      <desc>Escala automática de 0 a {bytes(max)} por segundo. Bajada continua y subida discontinua.</desc>
      <path d="M0 130H600" stroke="currentColor" opacity="0.3" />
      {(['down', 'up'] as const).map((direction) => <polyline key={direction} fill="none" stroke="currentColor" strokeWidth="2" strokeDasharray={direction === 'up' ? '6 4' : undefined} className={direction === 'down' ? 'text-sel' : 'text-fg'} points={points.map((point) => `${600 - (end - point.at) / 1000},${130 - point[direction] / max * 120}`).join(' ')} />)}
      {points.length === 1 && <circle cx="600" cy={130 - points[0].down / max * 120} r="3" fill="currentColor" />}
    </svg> : <p className="mt-2 text-mut">Sin muestras de tráfico todavía.</p>}
    <Heading>Historial de eventos</Heading>
    <p className="text-sm text-mut">El historial se pierde al cerrar la app o cambiar de red.</p>
    {history?.events.length ? <ol aria-label="Historial de eventos" className="mt-2 space-y-1">{[...history.events].reverse().map((event, i) => <li key={`${event.at}-${i}`}>{event.text} · hace {ago(Math.max(0, Date.now() - event.at) / 1000)}</li>)}</ol> : <p className="mt-2 text-mut">Sin eventos desde que se abrió esta red.</p>}
  </>;
}
