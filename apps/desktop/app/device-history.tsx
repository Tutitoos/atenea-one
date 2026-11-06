// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { bytes } from './lib/format';
import { eventText, useDeviceEvents, type DeviceHistory } from './lib/history';
import { Heading } from './ui';

type Point = NonNullable<DeviceHistory['points']>[number];

/** Pico, media y total de la ventana. Cada tasa cubre el intervalo que termina en su muestra, así que el primer punto no aporta bytes. */
export function sessionStats(points: Point[]) {
  const stats = { peakDown: 0, peakUp: 0, avgDown: 0, avgUp: 0, totalDown: 0, totalUp: 0 };
  points.forEach((p, i) => {
    const dt = i ? Math.max(0, p.at - points[i - 1].at) / 1000 : 0;
    stats.peakDown = Math.max(stats.peakDown, p.down); stats.peakUp = Math.max(stats.peakUp, p.up);
    stats.avgDown += p.down / points.length; stats.avgUp += p.up / points.length;
    stats.totalDown += p.down * dt; stats.totalUp += p.up * dt;
  });
  return stats;
}

/** Hoy basta la hora; otro día, fecha y hora. */
const when = (at: number) => (new Date(at).toDateString() === new Date().toDateString() ? `hoy ${new Date(at).toLocaleTimeString('es')}` : new Date(at).toLocaleString('es'));

export function TrafficHistory({ history }: { history?: DeviceHistory }) {
  const events = useDeviceEvents(history?.network, history?.id);
  const points = history?.points ?? [];
  const max = Math.max(1, ...points.flatMap((p) => [p.down, p.up]));
  // Eje X anclado a la hora actual: una pausa en el muestreo se ve como hueco a la derecha.
  const end = Date.now();
  const s = sessionStats(points);
  return <>
    <Heading>Tráfico · últimos 10 minutos</Heading>
    <p className="text-sm text-mut">Bajada (línea continua) · Subida (línea discontinua) · escala 0–{bytes(max)}/s</p>
    {points.length > 0 && <p className="mt-1 text-sm" aria-label="Estadísticas de la ventana"><span className="whitespace-nowrap">{`Pico ↓ ${bytes(s.peakDown)}/s ↑ ${bytes(s.peakUp)}/s`}</span>{' · '}<span className="whitespace-nowrap">{`Media ↓ ${bytes(s.avgDown)}/s ↑ ${bytes(s.avgUp)}/s`}</span>{' · '}<span className="whitespace-nowrap">{`Total ↓ ${bytes(s.totalDown)} ↑ ${bytes(s.totalUp)}`}</span></p>}
    {points.length ? <svg role="img" aria-label="Velocidad de bajada y subida en bytes por segundo durante los últimos diez minutos" viewBox="0 0 600 140" className="mt-2 w-full rounded-lg bg-card">
      <title>Tráfico del dispositivo</title>
      <desc>Escala automática de 0 a {bytes(max)} por segundo. Bajada continua y subida discontinua.</desc>
      <path d="M0 130H600" stroke="currentColor" opacity="0.3" />
      {(['down', 'up'] as const).map((direction) => <polyline key={direction} fill="none" stroke="currentColor" strokeWidth="2" strokeDasharray={direction === 'up' ? '6 4' : undefined} className={direction === 'down' ? 'text-sel' : 'text-fg'} points={points.map((point) => `${600 - (end - point.at) / 1000},${130 - point[direction] / max * 120}`).join(' ')} />)}
      {points.length === 1 && <circle cx={600 - (end - points[0].at) / 1000} cy={130 - points[0].down / max * 120} r="3" fill="currentColor" />}
    </svg> : <p className="mt-2 text-mut">Sin muestras de tráfico todavía.</p>}
    <Heading>Historial de eventos</Heading>
    <p className="text-sm text-mut">Se guardan en este equipo los cambios de camino de los últimos 30 días (máximo 200 por red); no se guarda tráfico.</p>
    {events.length ? <ol aria-label="Historial de eventos" className="mt-2 space-y-1">{[...events].reverse().map((event, i) => <li key={`${event.at}-${i}`}>{eventText(event.to)} · {when(event.at)}</li>)}</ol> : <p className="mt-2 text-mut">Sin eventos registrados para este dispositivo.</p>}
  </>;
}
