// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Outlet, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { expect, it, vi } from 'vitest';
import type { Sample } from './lib/agent';
import type { Agent } from './lib/live';
import Device from './routes/device';
import Devices from './routes/devices';
import { Loading, Skeleton } from './ui';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const PEER = 'b'.repeat(64);
const sample = {
  reply: {
    status: {
      public_key: 'c'.repeat(64), addrs: ['10.99.0.2'], hostname: 'equipo-a', config: { version: 1, lease_active: true }, peers: [{
        public_key: PEER, addrs: ['10.99.0.3'], name: 'equipo-b', endpoint: '198.51.100.7:51820', session: 'activa', last_handshake_secs_ago: 5,
        tx_bytes: 1, rx_bytes: 1, path: 'direct', candidates: 1, paths: [{ kind: 'direct', ready: true, healthy: true }],
      }],
    },
    health: { level: 'ok', text: 'Conectado', reasons: [] },
  },
  rates: {},
} as unknown as Sample;

const ctx = (over: Partial<Agent> = {}): Agent => ({ sample, stale: false, network: 'n1', error: null, empty: false, selectNetwork: () => {}, ...over });

function app(agent: Agent, entry = '/devices') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route element={<Outlet context={agent} />}>
            <Route path="devices" element={<Devices />}>
              <Route path=":id" element={<Device />} />
            </Route>
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

it('el esqueleto está oculto a la tecnología de apoyo y la región ocupada lo anuncia con texto', async () => {
  const { container } = render(<main><Loading><Skeleton className="h-4" /></Loading></main>);
  expect(container.querySelector('.skeleton')).toHaveAttribute('aria-hidden', 'true');
  expect(container.querySelector('[aria-busy="true"]')).toHaveTextContent('Cargando…');
  expect((await axe(container)).violations).toEqual([]);
});

it('la primera carga enseña esqueletos y no «Sin datos del agente»', () => {
  const { container } = app(ctx({ sample: undefined }), '/devices/self');
  expect(screen.queryByText('Sin datos del agente.')).not.toBeInTheDocument();
  expect(container.querySelectorAll('[aria-busy="true"]')).toHaveLength(2);
  expect(screen.getAllByText('Cargando…')).toHaveLength(2);
});

it('con error o sin agente se conserva el texto en vez del esqueleto', () => {
  const { container } = app(ctx({ sample: undefined, error: new Error('Agente parado') }), '/devices/self');
  expect(screen.getByText('Sin datos del agente.')).toBeInTheDocument();
  expect(screen.getByText('Agente parado')).toBeInTheDocument();
  expect(container.querySelector('[aria-busy="true"]')).toBeNull();
});

it('con datos obsoletos las acciones por dispositivo se deshabilitan y la fila no navega', async () => {
  const { container } = app(ctx({ stale: true }), `/devices/${PEER}`);
  expect(screen.getByRole('button', { name: 'Acciones de equipo-b' })).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Probar conexión' })).toBeDisabled();
  // El título deja de ser editable.
  expect(screen.queryByRole('button', { name: 'Cambiar nombre local' })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('link', { name: /equipo-a/ }));
  expect(screen.getByRole('heading', { level: 2, name: 'equipo-b' })).toBeInTheDocument();
  expect((await axe(container, { rules: { region: { enabled: false } } })).violations).toEqual([]);
});

it('sin datos obsoletos esas mismas acciones están activas', async () => {
  app(ctx(), `/devices/${PEER}`);
  expect(screen.getByRole('button', { name: 'Acciones de equipo-b' })).toBeEnabled();
  expect(screen.getByRole('button', { name: 'Probar conexión' })).toBeEnabled();
});

it('el evento atenea:buscar enfoca y selecciona el buscador, y se deja de escuchar al desmontar', async () => {
  const { unmount } = app(ctx());
  const search = screen.getByRole('searchbox', { name: 'Buscar dispositivos' });
  await userEvent.type(search, 'equipo');
  await userEvent.click(document.body);
  expect(search).not.toHaveFocus();
  act(() => { window.dispatchEvent(new CustomEvent('atenea:buscar')); });
  expect(search).toHaveFocus();
  expect((search as HTMLInputElement).selectionStart).toBe(0);
  expect((search as HTMLInputElement).selectionEnd).toBe('equipo'.length);
  unmount();
  const spy = vi.spyOn(HTMLElement.prototype, 'focus');
  act(() => { window.dispatchEvent(new CustomEvent('atenea:buscar')); });
  expect(spy).not.toHaveBeenCalled();
  spy.mockRestore();
});
