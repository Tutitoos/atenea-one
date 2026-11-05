// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Outlet, Route, Routes } from 'react-router';
import { beforeEach, expect, it, vi } from 'vitest';
import { sessionStats, TrafficHistory } from './device-history';
import { invoke } from './lib/bridge';
import type { Agent } from './lib/live';
import { ago, bytes, networkSummary, pct } from './lib/format';
import { reply } from './lib/status';
import Device from './routes/device';
import Devices from './routes/devices';
import Diagnostics, { groupCounters } from './routes/diagnostics';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));
beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); vi.mocked(invoke).mockImplementation(async (cmd: string) => (cmd === 'notifications_get' ? true : null) as never); });

const A = 'a'.repeat(64);
const B = 'b'.repeat(64);
const peer = (key: string, addr: string, path: string, extra: object = {}) => ({
  public_key: key, addrs: [addr], endpoint: '198.51.100.7:51820', session: 'activa', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }], ...extra,
});
const sample = (status: object = {}, rates = {}): Agent['sample'] => ({
  reply: reply.parse({ status: { public_key: 'c'.repeat(64), addrs: ['10.99.0.2'], config: { version: 1, lease_active: true }, counters: {}, peers: [], ...status }, health: { level: 'ok', text: 'Conectado', reasons: [] } }),
  rates,
});
function view(path: string, agent: Partial<Agent>, routes: 'diag' | 'devices' = 'diag') {
  const full: Agent = { sample: undefined, stale: false, network: 'n', error: null, empty: false, selectNetwork: () => undefined, ...agent };
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route element={<Outlet context={full} />}>
            <Route path="diagnostics" element={<Diagnostics />} />
            {routes === 'devices' && <Route path="devices" element={<Devices />}><Route path=":id" element={<Device />} /></Route>}
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

it('bytes y ago no inventan valores con NaN o negativos y respetan los límites', () => {
  expect(bytes(NaN)).toBe('—');
  expect(bytes(-5)).toBe('—');
  expect(bytes(1536)).toBe('1,5 KiB');
  expect(bytes(1023)).toBe('1023 B');
  expect(bytes(12345678)).toBe('11,8 MiB');
  expect(ago(-3)).toBe('0 s');
  expect(ago(NaN)).toBe('—');
  expect(ago(119.6)).toBe('119 s');
  expect(ago(120)).toBe('2 min');
  expect(ago(7199)).toBe('119 min');
  expect(ago(47 * 3600)).toBe('47 h');
  expect(ago(48 * 3600)).toBe('2 d');
  expect(pct(1, 540)).toBe('0,2 %');
});

it('las preferencias siguen visibles con el agente parado y sin redes', () => {
  const { unmount } = view('/diagnostics', { error: new Error('agente parado') });
  expect(screen.getByRole('switch', { name: 'Notificaciones del sistema' })).toBeInTheDocument();
  expect(screen.getByText('agente parado')).toBeInTheDocument();
  unmount();
  view('/diagnostics', { empty: true });
  expect(screen.getByRole('switch', { name: 'Notificaciones del sistema' })).toBeInTheDocument();
  expect(screen.getByText(/Sin redes\./)).toBeInTheDocument();
});

it('los contadores se agrupan por familia, ocultan los ceros y marcan los errores', async () => {
  const counters = { path_lost: 2, path_switches: 0, relay_renewal_failed: 1, tx_init: 12345, tx_acl_denied: 3, config_rejected: 0, algo_nuevo: 4 };
  expect(groupCounters(counters, false).map((g) => [g.title, g.items.map(([k]) => k)])).toEqual([
    ['Caminos', ['path_lost']], ['Relay', ['relay_renewal_failed']], ['Handshakes', ['tx_init']], ['Tráfico denegado o descartado', ['tx_acl_denied']], ['Otros', ['algo_nuevo']],
  ]);
  expect(groupCounters(counters, true).find((g) => g.title === 'Configuración y lease')?.items).toEqual([['config_rejected', 0]]);

  view('/diagnostics', { sample: sample({ counters }) });
  const relay = within(screen.getByRole('region', { name: 'Relay' }));
  expect(relay.getByText('1')).toHaveClass('text-warn');
  expect(screen.getByText('12.345')).not.toHaveClass('text-warn');
  expect(screen.queryByText('path_switches')).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('checkbox', { name: /Mostrar todos/ }));
  expect(screen.getByText('path_switches')).toBeInTheDocument();
});

it('cada relay muestra pérdida de pings en %, paquetes, peer_ready y el último error', () => {
  const entries = [{ peer: 'aabbccdd11', transport: 'Udp', state: 'ready', rtt_ms: 42, pings_sent: 540, pings_lost: 1, tx_bytes: 2048, tx_packets: 7, rx_bytes: 1024, rx_packets: 5, peer_ready: false, last_error: 'ping_timeout' }];
  view('/diagnostics', { sample: sample({ relay: { allocations: 1, drops: {}, entries } }) });
  expect(screen.getByText(/42 ms · pings perdidos 0,2 % \(1\/540\) · tx 2,0 KiB \(7 paq\.\) · rx 1,0 KiB \(5 paq\.\) · par no listo · error: ping_timeout/)).toBeInTheDocument();
  expect(screen.getByText(/no equivalen a los bytes de datos/)).toBeInTheDocument();
});

it('la latencia es «al relay» por relay y «no medida» en directo', async () => {
  const peers = [
    peer(A, '10.99.0.3', 'relay-udp', { paths: [{ kind: 'relay-udp', ready: true, healthy: false, rtt_ms: 21, last_rx_secs: 6 }], last_handshake_secs_ago: null }),
    peer(B, '10.99.0.4', 'direct'),
  ];
  const { unmount } = view(`/devices/${A}`, { sample: sample({ peers }) }, 'devices');
  expect(screen.getByText('Latencia al relay')).toBeInTheDocument();
  expect(screen.getByText('21 ms')).toBeInTheDocument();
  expect(screen.getByText('listo · con fallos · sin recibir desde hace 6 s')).toBeInTheDocument();
  expect(screen.getByText('Última sesión').nextSibling).toHaveTextContent('—');
  unmount();
  view(`/devices/${B}`, { sample: sample({ peers }) }, 'devices');
  expect(screen.getByText('No medida en conexión directa')).toBeInTheDocument();
  expect(screen.queryByText('Latencia al relay')).not.toBeInTheDocument();
});

it('el resumen de red suma las tasas y cuenta los caminos', () => {
  const peers = [{ public_key: A, path: 'direct' }, { public_key: B, path: 'relay-tls' }, { public_key: 'c', path: 'none' }];
  expect(networkSummary(peers, { [A]: { down: 100, up: 10 }, [B]: { down: 50, up: 5 } })).toEqual({ down: 150, up: 15, peers: 3, direct: 1, relay: 1, other: 1 });

  view('/devices', { sample: sample({ peers: [peer(A, '10.99.0.3', 'direct'), peer(B, '10.99.0.4', 'relay-udp')] }, { [A]: { down: 2048, up: 0 }, [B]: { down: 1024, up: 512 } }) }, 'devices');
  expect(screen.getByLabelText('Resumen de la red')).toHaveTextContent('↓ 3,0 KiB/s · ↑ 512 B/s · 2 pares (1 directo, 1 relay)');
});

it('las estadísticas de sesión salen de los puntos: pico, media y total', () => {
  const points = [{ at: 0, down: 100, up: 0 }, { at: 2000, down: 300, up: 50 }, { at: 3000, down: 200, up: 10 }];
  expect(sessionStats(points)).toEqual({ peakDown: 300, peakUp: 50, avgDown: 200, avgUp: 20, totalDown: 300 * 2 + 200, totalUp: 100 + 10 });
  expect(sessionStats([])).toMatchObject({ peakDown: 0, totalUp: 0 });

  render(<TrafficHistory history={{ path: 'direct', events: [], points: [{ at: Date.now() - 2000, down: 1024, up: 0 }, { at: Date.now(), down: 2048, up: 512 }] }} />);
  expect(screen.getByLabelText('Estadísticas de la ventana')).toHaveTextContent('Pico ↓ 2,0 KiB/s ↑ 512 B/s · Media ↓ 1,5 KiB/s ↑ 256 B/s · Total ↓ 4,0 KiB ↑ 1,0 KiB');
});
