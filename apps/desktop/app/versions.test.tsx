// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-025, fase 5: versiones instaladas en «Detalles» (propio y de cada par) y sus tres estados.
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import { resetHistoryStore } from './lib/history-store';
import Device from './routes/device';
import Devices from './routes/devices';
import Shell from './routes/shell';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const PEER_ID = '0199ab12-3456-7def-8abc-0123456789cd';
const KEY = 'a'.repeat(64);
const OWN = 'c'.repeat(64);
const net = { network_id: NET_ID, name: 'casa', server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada' };

const peer = (extra: Record<string, unknown>) => ({
  public_key: KEY, device_id: PEER_ID, addrs: ['10.99.0.3'], name: 'equipo-a', endpoint: '192.0.2.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 1, rx_bytes: 1, path: 'direct', candidates: 1, paths: [{ kind: 'direct', ready: true, healthy: true, rtt_ms: null }], ...extra,
});

function serve(peers: unknown[], own: Record<string, unknown> = {}) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'network_list': return JSON.stringify([net]) as never;
      case 'network_legacy': return '{"present":false}' as never;
      case 'autostart_get': return null as never;
      case 'notifications_get': return true as never;
      case 'client_status': return { os: 'linux', supported: true, can_join: true, installed: true } as never;
      case 'status': return JSON.stringify({
        status: { public_key: OWN, addrs: ['10.99.0.2'], hostname: 'equipo-b', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, counters: {}, peers, ...own },
        health: { level: 'ok', text: 'Conectado', reasons: [] },
      }) as never;
      default: throw new Error(`orden inesperada: ${cmd}`);
    }
  });
}

function app(id: string) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={[`/devices/${id}`]}>
        <Routes>
          <Route element={<Shell />}>
            <Route path="devices" element={<Devices />}><Route path=":id" element={<Device />} /></Route>
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const value = (label: string) => screen.getByText(label).nextElementSibling?.textContent;
const COMMIT = 'dfe72aef0123456789abcdef0123456789abcdef';

beforeEach(() => { localStorage.clear(); resetHistoryStore(); vi.mocked(invoke).mockReset(); });

it('el par que informó muestra la versión con el commit corto y «No instalada» para lo que falta, y axe sale limpio', async () => {
  serve([peer({ versions: [{ component: 'agent', version: '0.1.31', commit: COMMIT }, { component: 'cli', version: '0.1.31', commit: 'unknown' }], versions_at: '2026-11-16T10:00:00Z' })]);
  const { container } = app(KEY);
  await screen.findByText('Versión del agente');
  expect(value('Versión del agente')).toBe('0.1.31 (dfe72ae)');
  expect(value('Versión de la CLI')).toBe('0.1.31');
  expect(value('Versión de la aplicación')).toBe('No instalada');
  expect((await axe(container)).violations).toEqual([]);
});

it('un par que nunca informó (o un agente anterior sin el campo) es «Sin informar», no «No instalada»', async () => {
  serve([peer({ versions: [], versions_at: null })]);
  app(KEY);
  await screen.findByText('Versión del agente');
  for (const row of ['Versión del agente', 'Versión de la CLI', 'Versión de la aplicación']) expect(value(row)).toBe('Sin informar');
});

it('informar la lista vacía con fecha significa que no hay nada instalado', async () => {
  serve([peer({ versions: [], versions_at: '2026-11-16T10:00:00Z' })]);
  app(KEY);
  await screen.findByText('Versión del agente');
  expect(value('Versión del agente')).toBe('No instalada');
});

it('el propio equipo: `null` hasta la primera detección, después lo detectado; una entrada rara se descarta', async () => {
  serve([], { versions: null });
  const first = app('self');
  await first.findByText('Versión del agente');
  expect(value('Versión de la aplicación')).toBe('Sin informar');
  first.unmount();
  serve([], { versions: [{ component: 'agent', version: '0.1.32', commit: COMMIT }, { component: 'otro', version: '1', commit: 'x' }, { component: 'desktop', version: '0.1.32', commit: 'unknown' }] });
  app('self');
  await screen.findByText('Versión del agente');
  expect(value('Versión del agente')).toBe('0.1.32 (dfe72ae)');
  expect(value('Versión de la CLI')).toBe('No instalada');
  expect(value('Versión de la aplicación')).toBe('0.1.32');
});
