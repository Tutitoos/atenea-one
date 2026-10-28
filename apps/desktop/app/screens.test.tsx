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

const peer = (key: string, addr: string, path: string, name?: string) => ({
  public_key: key, addrs: [addr], name, endpoint: '192.0.2.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }],
});
const degraded = { level: 'degraded', text: 'Degradado', reasons: ['algún par por relay o sin camino'] };
const agent = (peers = [peer('a'.repeat(64), '10.77.0.3', 'direct', 'servidor-vps'), peer('b'.repeat(64), '10.77.0.4', 'relay-udp')], health: object = degraded) => JSON.stringify({
  status: { public_key: 'c'.repeat(64), addrs: ['10.77.0.2'], hostname: 'MacBook de prueba', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, nat: { mapping: 'unknown', reflexive: '203.0.113.9:51820' }, counters: { tx_init: 3 }, relay: { allocations: 0, entries: [], drops: {} }, peers },
  health,
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

it('la banda de avisos solo aparece si hay motivos que contar', async () => {
  // Caso real de Rust: un par por relay degrada el estado pero no aporta ningún motivo textual.
  vi.mocked(invoke).mockResolvedValue(agent(undefined, { level: 'degraded', text: 'Degradado: algún par por relay o sin camino', reasons: [] }));
  const { unmount } = app();
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Degradado'));
  expect(screen.queryByRole('list', { name: 'Avisos' })).not.toBeInTheDocument();
  unmount();
  vi.mocked(invoke).mockResolvedValue(agent(undefined, { level: 'down', text: 'Sin conexión: sin lease activo', reasons: ['sin lease activo'] }));
  app();
  expect(await screen.findByRole('list', { name: 'Avisos' })).toHaveTextContent('sin lease activo');
});

it('abre en este dispositivo y al elegir un par enseña su camino, direcciones y tráfico', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  app();
  // Este dispositivo se llama como el sistema; el par con nombre de la red usa ese nombre y el otro, su IP.
  expect(await screen.findByRole('heading', { name: 'MacBook de prueba', level: 2 })).toBeInTheDocument();
  expect(screen.getByRole('link', { name: /servidor-vps/ })).toBeInTheDocument();
  await userEvent.click(await screen.findByRole('link', { name: /10\.77\.0\.4/ }));
  const detail = within(screen.getByRole('main'));
  expect(await detail.findByText('Por relay UDP', { selector: 'span' })).toBeInTheDocument();
  expect(detail.getByText('IPv4')).toBeInTheDocument();
  // Puede ir seguido de la tasa si hubo una lectura anterior (el muestreador vive a nivel de módulo).
  expect(detail.getByText(/^4\.0 KiB/)).toBeInTheDocument();
  expect(screen.getByRole('status')).toHaveTextContent('Degradado');
});

it('la búsqueda filtra por IP y avisa si no hay resultados', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  app();
  const search = await screen.findByRole('searchbox', { name: 'Buscar dispositivos' });
  await userEvent.type(search, 'servidor');
  expect(screen.getByRole('link', { name: /servidor-vps/ })).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: /10\.77\.0\.4/ })).not.toBeInTheDocument();
  await userEvent.clear(search);
  await userEvent.type(search, 'no-existe');
  expect(screen.getByText('Sin resultados.')).toBeInTheDocument();
});

it('un alias local gana al nombre de la red y vaciarlo lo devuelve', async () => {
  vi.mocked(invoke).mockResolvedValue(agent());
  app(`/devices/${'a'.repeat(64)}`);
  // El nombre de la red (el del sistema del otro equipo) es el título hasta que haya un alias local.
  await userEvent.click(await screen.findByRole('button', { name: 'servidor-vps' }));
  await userEvent.type(screen.getByRole('textbox', { name: 'Nombre local' }), 'portátil{Enter}');
  expect(await screen.findByRole('link', { name: /portátil/ })).toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem('atenea-names') ?? '{}')).toEqual({ ['a'.repeat(64)]: 'portátil' });
  expect(within(screen.getByRole('main')).getByText('servidor-vps')).toBeInTheDocument();
  // Vaciar el alias devuelve el nombre de la red.
  await userEvent.click(screen.getByRole('button', { name: 'portátil' }));
  await userEvent.clear(screen.getByRole('textbox', { name: 'Nombre local' }));
  await userEvent.keyboard('{Enter}');
  expect(await screen.findByRole('link', { name: /servidor-vps/ })).toBeInTheDocument();
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
