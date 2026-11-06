// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// Windows (W1): solo se ve el estado de la red instalada; no se ofrece unir, conectar, desconectar ni salir.
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import type { Network } from './lib/networks';
import { phaseOf } from './lib/wizard';
import Networks from './routes/networks';
import Welcome from './routes/welcome';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const windows = { os: 'windows', supported: true, can_join: false, installed: true };
const legacyNet: Network = {
  network_id: '00000000-0000-4000-8000-000000000001', name: 'Red de este equipo', server: 'panel.example.org', url: '',
  interface: 'atenea0', listen_port: 0, join_status: 'aprobada', state: 'conectada',
};
const honest = /En Windows todavía no se puede unir ni gestionar redes desde la aplicación/;

function serve(client: object, networks: Network[]) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'client_status': return client as never;
      case 'network_list': return JSON.stringify(networks) as never;
      case 'network_legacy': return JSON.stringify({ present: false }) as never;
      case 'autostart_get': return null as never;
      default: throw new Error(`orden inesperada: ${cmd}`);
    }
  });
}

function show(element: React.ReactElement) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter><Routes><Route index element={element} /></Routes></MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => { vi.mocked(invoke).mockReset(); });

it('phaseOf en Windows: con el agente y su red va a listo; sin agente pide instalarlo; sin red no ofrece unirse', () => {
  expect(phaseOf({ client: windows, networks: [legacyNet] })).toBe('listo');
  expect(phaseOf({ client: { ...windows, installed: false } })).toBe('cliente');
  expect(phaseOf({ client: windows, networks: [], legacy: true })).toBe('sistema');
});

it('el asistente en Windows con agente muestra el estado y el texto honesto, sin unirse', async () => {
  serve(windows, [legacyNet]);
  const { container } = show(<Welcome />);
  expect(await screen.findByRole('region', { name: 'Ya tienes redes unidas' })).toHaveTextContent(honest);
  expect(screen.queryByRole('link', { name: 'Unirme a otra red' })).not.toBeInTheDocument();
  expect(screen.queryByRole('textbox', { name: 'Código de red' })).not.toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});

it('el asistente en Windows sin agente explica qué hacer y no ofrece una orden de instalación', async () => {
  serve({ ...windows, installed: false }, []);
  show(<Welcome />);
  const step = await screen.findByRole('region', { name: 'Instala el cliente de Atenea One' });
  expect(step).toHaveTextContent('Instala y arranca el servicio AteneaAgent');
  expect(screen.queryByRole('button', { name: /Copiar orden/ })).not.toBeInTheDocument();
  expect(screen.queryByRole('link')).not.toBeInTheDocument();
});

it('Redes en Windows muestra la red sin acciones de gestión ni formulario de unión', async () => {
  serve(windows, [legacyNet]);
  const { container } = show(<Networks />);
  expect(await screen.findByRole('region', { name: 'Red de este equipo' })).toHaveTextContent('Conectada');
  expect(screen.queryByRole('button', { name: /Desconectar|Conectar|Salir de la red/ })).not.toBeInTheDocument();
  expect(screen.queryByRole('textbox', { name: 'Código de red' })).not.toBeInTheDocument();
  expect(screen.getByText(honest)).toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});
