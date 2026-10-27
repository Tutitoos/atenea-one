// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import Device from './routes/device';
import Devices from './routes/devices';
import DevicesIndex from './routes/devices-index';
import Diagnostics from './routes/diagnostics';
import Shell from './routes/shell';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const peer = (key: string, addr: string, path: string) => ({
  public_key: key, addrs: [addr], endpoint: '192.0.2.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }],
});
const agent = (peers = [peer('a'.repeat(64), '10.77.0.3', 'direct'), peer('b'.repeat(64), '10.77.0.4', 'relay-udp')]) => JSON.stringify({
  status: { public_key: 'c'.repeat(64), addrs: ['10.77.0.2'], config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, nat: { mapping: 'unknown', reflexive: '203.0.113.9:51820' }, counters: { tx_init: 3 }, relay: { allocations: 0, entries: [], drops: {} }, peers },
  health: { level: 'degraded', text: 'Degradado', reasons: ['algún par por relay o sin camino'] },
});

function app(entry = '/devices') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route element={<Shell />}>
            <Route path="devices" element={<Devices />}>
              <Route index element={<DevicesIndex />} />
              <Route path=":id" element={<Device />} />
            </Route>
            <Route path="diagnostics" element={<Diagnostics />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });

it('abre en este dispositivo y al elegir un par enseña su camino, direcciones y tráfico', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  app();
  expect(await screen.findByRole('heading', { name: 'Este dispositivo', level: 2 })).toBeInTheDocument();
  await userEvent.click(await screen.findByRole('link', { name: /10\.77\.0\.4/ }));
  const detail = within(screen.getByRole('main'));
  expect(await detail.findByText('Por relay UDP', { selector: 'span' })).toBeInTheDocument();
  expect(detail.getByText('IPv4')).toBeInTheDocument();
  expect(detail.getByText('4.0 KiB')).toBeInTheDocument();
  expect(screen.getByRole('status')).toHaveTextContent('Degradado');
});

it('la búsqueda filtra por IP y avisa si no hay resultados', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  app();
  const search = await screen.findByRole('searchbox', { name: 'Buscar dispositivos' });
  await userEvent.type(search, '10.77.0.3');
  expect(screen.getByRole('link', { name: /10\.77\.0\.3/ })).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: /10\.77\.0\.4/ })).not.toBeInTheDocument();
  await userEvent.clear(search);
  await userEvent.type(search, 'no-existe');
  expect(screen.getByText('Sin resultados.')).toBeInTheDocument();
});

it('un nombre local sustituye a la IP y se conserva', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  app(`/devices/${'a'.repeat(64)}`);
  await userEvent.click(await screen.findByRole('button', { name: '10.77.0.3' }));
  await userEvent.type(screen.getByRole('textbox', { name: 'Nombre local' }), 'portátil{Enter}');
  expect(await screen.findByRole('link', { name: /portátil/ })).toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem('atenea-names') ?? '{}')).toEqual({ ['a'.repeat(64)]: 'portátil' });
});

it('si el agente falla no enseña datos y muestra el motivo', async () => {
  vi.mocked(invoke).mockRejectedValue('Agente parado o sin acceso al estado');
  app();
  expect(await screen.findAllByText('Agente parado o sin acceso al estado')).not.toHaveLength(0);
  expect(screen.getByRole('status')).toHaveTextContent('Sin datos');
  expect(screen.queryByRole('link', { name: /Este dispositivo/ })).not.toBeInTheDocument();
});

it('una respuesta con otra forma se rechaza en la frontera', async () => {
  vi.mocked(invoke).mockResolvedValue(JSON.stringify({ status: { peers: 3 } }));
  app();
  expect(await screen.findAllByText('El agente respondió con un formato inesperado.')).not.toHaveLength(0);
  expect(screen.getByRole('status')).toHaveTextContent('Sin datos');
});

it('las pantallas no tienen violaciones de accesibilidad detectables', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  for (const entry of ['/devices', `/devices/${'a'.repeat(64)}`, '/diagnostics']) {
    const { container, unmount } = app(entry);
    await screen.findByRole('navigation', { name: 'Secciones' });
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Degradado'));
    // jsdom no calcula layout ni contraste: el contraste necesita comprobación en un webview real.
    expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
    unmount();
  }
});
