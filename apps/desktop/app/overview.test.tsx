// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import Overview from './routes/overview';
import Shell from './routes/shell';
import { ToastProvider } from './toast';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const A = '0199ab12-3456-7def-8abc-0123456789aa';
const B = '0199ab12-3456-7def-8abc-0123456789bb';
const net = (id: string, name: string) => ({ network_id: id, name, server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada' });
const key = (c: string) => c.repeat(64);
const peer = (k: string, path: string) => ({
  public_key: k, addrs: ['10.99.0.3'], name: 'equipo-a', endpoint: '198.51.100.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }],
});
const status = JSON.stringify({
  status: { public_key: key('c'), addrs: ['10.99.0.2'], hostname: 'equipo-local', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, nat: { mapping: 'unknown', reflexive: '198.51.100.9:51820' }, counters: {}, relay: { allocations: 0, entries: [], drops: {} }, peers: [peer(key('a'), 'direct'), peer(key('b'), 'relay-udp'), peer(key('d'), 'none')] },
  health: { level: 'degraded', text: 'Degradada', reasons: ['Sin relay disponible'] },
});
function serve(nets: unknown[], failing?: string) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === 'network_list') return JSON.stringify(nets) as never;
    if (failing && (args as { network: string }).network === failing) throw 'El agente no responde.';
    return status as never;
  });
}
function app() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/overview']}>
          <Routes>
            <Route element={<Shell />}>
              <Route path="overview" element={<Overview />} />
              <Route path="devices" element={<p>Pantalla de dispositivos</p>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });

it('pinta una tarjeta por red con sus datos', async () => {
  serve([net(A, 'casa'), net(B, 'oficina')]);
  app();
  const card = await screen.findByRole('button', { name: /casa/ });
  expect(within(card).getByText('panel.example.org')).toBeInTheDocument();
  await waitFor(() => expect(card).toHaveTextContent('3 pares (1 directo, 1 por relay, 1 otro)'));
  expect(card).toHaveTextContent('Conectada');
  expect(card).toHaveTextContent('Lease: quedan');
  expect(card).toHaveTextContent('Sin relay disponible');
  expect(screen.getByRole('button', { name: /oficina/ })).toBeInTheDocument();
  expect((await axe(document.body, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('una red con error no tumba las demás', async () => {
  serve([net(A, 'casa'), net(B, 'oficina')], B);
  app();
  const bad = await screen.findByRole('button', { name: /oficina/ });
  await waitFor(() => expect(bad).toHaveTextContent('El agente no responde.'));
  await waitFor(() => expect(screen.getByRole('button', { name: /casa/ })).toHaveTextContent('3 pares'));
});

it('un clic selecciona la red y navega a Dispositivos', async () => {
  serve([net(A, 'casa'), net(B, 'oficina')]);
  app();
  await userEvent.click(await screen.findByRole('button', { name: /oficina/ }));
  expect(await screen.findByText('Pantalla de dispositivos')).toBeInTheDocument();
  expect(localStorage.getItem('atenea.network')).toBe(B);
});

it('sin redes muestra el estado vacío', async () => {
  serve([]);
  app();
  expect(await screen.findByText(/Sin redes\. Únete/)).toBeInTheDocument();
});
