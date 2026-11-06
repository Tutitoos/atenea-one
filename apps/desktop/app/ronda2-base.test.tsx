// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { readFileSync } from 'node:fs';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Outlet, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import Server from './routes/server';
import Servers from './routes/servers';
import { ToastProvider } from './toast';
import { button, Empty, Heading } from './ui';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const ID = '0199ab12-3456-7def-8abc-0123456789ab';
const HOST = 'panel.example.org';
const net = (over: object = {}) => ({ network_id: ID, name: 'casa', server: HOST, url: `https://${HOST}`, interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada', ...over });
const now = Math.floor(Date.now() / 1000);
const reply = (days = 6) => JSON.stringify({
  status: {
    public_key: 'c'.repeat(64), addrs: ['10.99.0.2'], hostname: 'equipo-a', config: { version: 1, lease_active: true, valid_until_unix: now + 240 },
    nat: { mapping: 'unknown', reflexive: '198.51.100.9:51820' }, counters: {}, relay: { allocations: 0, entries: [], drops: {} }, peers: [],
    sync: { url: `https://${HOST}`, last_ok_unix: now - 5, last_error: null }, coordinator: null,
    server: { control_plane: { version: '0.1.6', commit: 'abcdef1234567890' }, server_time_unix: now, clock_skew_secs: 0, issuer_cert_not_after_unix: now + days * 86400, fetched_unix: now, last_error: null },
  },
  health: { level: 'ok', text: 'Bien', reasons: [] },
});

function serve(networks: object[], extra: Record<string, (args: unknown) => unknown> = {}) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd in extra) return extra[cmd](args) as never;
    if (cmd === 'network_list') return JSON.stringify(networks) as never;
    return reply() as never;
  });
}

function app(entry: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}><ToastProvider><MemoryRouter initialEntries={[entry]}><Routes>
      <Route element={<Outlet context={{ selectNetwork: vi.fn() }} />}>
        <Route path="servers" element={<Servers />} />
        <Route path="servers/:host" element={<Server />} />
      </Route>
    </Routes></MemoryRouter></ToastProvider></QueryClientProvider>,
  );
}

beforeEach(() => vi.mocked(invoke).mockReset());

it('el foco visible global existe en la hoja de estilos para enlaces, botones y campos', () => {
  const css = readFileSync('app/styles.css', 'utf8');
  const rule = css.split('\n').find((line) => line.includes('button:focus-visible'));
  expect(rule).toMatch(/a:focus-visible.*input:focus-visible.*textarea:focus-visible.*\[role="button"\]:focus-visible.*outline: 2px solid/);
  expect(button).not.toMatch(/outline-none|focus:outline-0/);
});

it('Empty distingue error (alert) y carga (status); sin tono es un párrafo plano', () => {
  render(<><Empty tone="error">fallo</Empty><Empty tone="loading">cargando</Empty><Empty>vacío</Empty></>);
  expect(screen.getByRole('alert')).toHaveTextContent('fallo');
  expect(screen.getByRole('alert')).toHaveClass('text-bad');
  expect(screen.getByRole('status')).toHaveTextContent('cargando');
  expect(screen.getByText('vacío')).not.toHaveAttribute('role');
});

it('Heading es un h2', () => {
  render(<Heading>Sección</Heading>);
  expect(screen.getByRole('heading', { level: 2, name: 'Sección' })).toBeInTheDocument();
});

it('servidor: botones con el estilo compartido, glosas y toast al desconectar', async () => {
  serve([net()], { network_apply: () => '' });
  const { container } = app(`/servers/${HOST}`);
  const card = within(await screen.findByRole('region', { name: 'casa' }));
  const disconnect = card.getByRole('button', { name: 'Desconectar' });
  expect(disconnect).toHaveClass(...button.split(' '));
  expect(card.getByRole('link', { name: 'Ver dispositivos' })).toHaveClass(...button.split(' '));
  expect(card.getByRole('button', { name: 'Abrir en el panel' })).toHaveClass(...button.split(' '));
  expect(await screen.findByText('NAT')).toHaveAttribute('title');
  await userEvent.click(disconnect);
  expect(await screen.findByText('«casa» desconectada', { selector: 'p' })).toBeInTheDocument();
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('servidor: un fallo de lectura es una alerta', async () => {
  serve([], { network_list: () => { throw 'sin conexión con el agente'; } });
  app(`/servers/${HOST}`);
  expect(await screen.findByRole('alert')).toHaveTextContent('sin conexión con el agente');
});

it('servidores: un fallo de lectura se anuncia y «Reintentar» vuelve a leer', async () => {
  serve([], { network_list: () => { throw 'sin conexión con el agente'; } });
  app('/servers');
  expect(await screen.findByRole('alert')).toHaveTextContent('sin conexión con el agente');
  const before = vi.mocked(invoke).mock.calls.filter(([c]) => c === 'network_list').length;
  await userEvent.click(screen.getByRole('button', { name: 'Reintentar' }));
  await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === 'network_list').length).toBeGreaterThan(before));
});

it('servidores: la insignia indica los días restantes y el host largo se trunca con title', async () => {
  const long = `${'subdominio-muy-largo-'.repeat(4)}example.org`;
  serve([net({ url: `https://${long}` })]);
  const { container } = app('/servers');
  expect(await screen.findByText('Certificado: quedan 6 días')).toBeInTheDocument();
  const host = screen.getByText(long);
  expect(host).toHaveAttribute('title', long);
  expect(host).toHaveClass('truncate', 'min-w-0');
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
