// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Outlet, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import type { Agent } from './lib/live';
import { reply } from './lib/status';
import Diagnostics from './routes/diagnostics';
import Networks from './routes/networks';
import { ToastProvider } from './toast';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.mocked(invoke).mockReset(); });

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const OTHER_ID = '0199ab12-3456-7def-8abc-0123456789cd';
const net = (over: object = {}) => ({ network_id: NET_ID, name: 'casa', server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada', ...over });
const info = { name: 'casa', server: 'panel.example.org', network_id: NET_ID, root_fingerprint: '6c4b-fd0b-2585-9aae', has_token: false };

function serve(networks: object[] | (() => object[]), extra: Record<string, (args: unknown) => unknown> = {}) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd in extra) return extra[cmd](args) as never;
    if (cmd === 'network_list') return JSON.stringify(typeof networks === 'function' ? networks() : networks) as never;
    if (cmd === 'network_legacy') return '{"present":false}' as never;
    if (cmd === 'notifications_get') return true as never;
    return null as never;
  });
}

const sample = (): Agent['sample'] => ({
  reply: reply.parse({ status: { public_key: 'c'.repeat(64), addrs: ['10.99.0.2'], config: { version: 1, lease_active: true }, counters: { tx_init: 3 }, peers: [] }, health: { level: 'ok', text: 'Conectado', reasons: [] } }),
  rates: {},
});

function diagnostics(agent: Partial<Agent>) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const full: Agent = { sample: undefined, stale: false, network: 'n', error: null, empty: false, selectNetwork: () => undefined, ...agent };
  const invalidate = vi.spyOn(client, 'invalidateQueries');
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/diagnostics']}>
        <Routes><Route element={<Outlet context={full} />}><Route path="diagnostics" element={<Diagnostics />} /></Route></Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return { ...view, invalidate };
}

function networks() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider><MemoryRouter><Networks /></MemoryRouter></ToastProvider>
    </QueryClientProvider>,
  );
}

it('Diagnóstico: un h1, secciones h2, contadores h3 y las preferencias después de los datos', async () => {
  serve([]);
  const { container } = diagnostics({ sample: sample() });
  expect(screen.getAllByRole('heading', { level: 1 }).map((h) => h.textContent)).toEqual(['Diagnóstico']);
  const h2 = screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
  expect(h2).toEqual(expect.arrayContaining(['Informe', 'Errores', 'Relays (0)', 'Contadores', 'Notificaciones']));
  expect(screen.getAllByRole('heading', { level: 3 }).length).toBeGreaterThan(0);
  const toggle = await screen.findByRole('switch', { name: 'Notificaciones del sistema' });
  const lastData = screen.getByRole('heading', { level: 2, name: 'Contadores' });
  expect(lastData.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Copiar informe de diagnóstico' })).toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});

it('Diagnóstico: etiquetas explicadas, «ninguno» neutro y relays sin asignaciones explicados', () => {
  serve([]);
  diagnostics({ sample: sample() });
  expect(screen.getByText('Control plane (servidor de control)')).toBeInTheDocument();
  expect(screen.getByText('Coordinator (coordinador)')).toBeInTheDocument();
  expect(screen.getAllByText('ninguno')[0]).not.toHaveClass('text-bad');
  expect(screen.getByText('Este equipo no usa ningún relay ahora')).toBeInTheDocument();
});

it('Diagnóstico: un error de lectura es una alerta con «Reintentar», que vuelve a pedir la lectura del agente', async () => {
  serve([]);
  const { invalidate, container } = diagnostics({ error: new Error('agente parado') });
  expect(screen.getByRole('alert')).toHaveTextContent('agente parado');
  await userEvent.click(screen.getByRole('button', { name: 'Reintentar' }));
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ['agent'] });
  // Las preferencias siguen disponibles aunque no haya datos.
  expect(await screen.findByRole('switch', { name: 'Notificaciones del sistema' })).toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});

it('Redes: un fallo al listar es una alerta y «Reintentar» vuelve a listar', async () => {
  let fail = true;
  serve(() => { if (fail) throw 'no se pudo listar'; return [net()]; });
  networks();
  expect(await screen.findByRole('alert')).toHaveTextContent('no se pudo listar');
  fail = false;
  await userEvent.click(screen.getByRole('button', { name: 'Reintentar' }));
  expect(await screen.findByRole('region', { name: 'casa' })).toBeInTheDocument();
});

it('Redes: «Salir de la red» es destructivo, enfoca Cancelar, devuelve el foco al cancelar y avisa al salir', async () => {
  serve([net()], { network_apply: () => '' });
  const { container } = networks();
  const opener = await screen.findByRole('button', { name: 'Salir de la red…' });
  await userEvent.click(opener);
  const dialog = screen.getByRole('alertdialog', { name: 'Salir de casa' });
  expect(within(dialog).getByText('No se puede deshacer.').tagName).toBe('STRONG');
  expect(dialog.firstElementChild?.firstElementChild).toHaveTextContent('No se puede deshacer.');
  expect(within(dialog).getByRole('button', { name: 'Salir de la red' })).toHaveClass('bg-bad');
  expect(within(dialog).getByRole('button', { name: 'Cancelar' })).toHaveFocus();
  expect((await axe(container)).violations).toEqual([]);

  await userEvent.click(within(dialog).getByRole('button', { name: 'Cancelar' }));
  expect(opener).toHaveFocus();

  await userEvent.click(opener);
  await userEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Salir de la red' }));
  expect(await screen.findByRole('status', { name: 'Notificaciones' })).toHaveTextContent('Saliste de «casa»');
});

it('Redes: conectar y desconectar avisan con un aviso breve', async () => {
  serve([net(), net({ network_id: OTHER_ID, name: 'trabajo', state: 'detenida' })], { network_apply: () => '' });
  networks();
  await userEvent.click(within(await screen.findByRole('region', { name: 'casa' })).getByRole('button', { name: 'Desconectar' }));
  await waitFor(() => expect(screen.getByRole('status', { name: 'Notificaciones' })).toHaveTextContent('Desconectaste «casa»'));
  await userEvent.click(within(screen.getByRole('region', { name: 'trabajo' })).getByRole('button', { name: 'Conectar' }));
  await waitFor(() => expect(screen.getByRole('status', { name: 'Notificaciones' })).toHaveTextContent('Conectaste «trabajo»'));
});

it('Redes: la espera de autorización aparece solo en la tarjeta de la red afectada', async () => {
  serve([net(), net({ network_id: OTHER_ID, name: 'trabajo', state: 'detenida' })], { network_apply: () => new Promise(() => undefined) });
  networks();
  await userEvent.click(within(await screen.findByRole('region', { name: 'trabajo' })).getByRole('button', { name: 'Conectar' }));
  expect(await within(screen.getByRole('region', { name: 'trabajo' })).findByText('Esperando la autorización del sistema…')).toBeInTheDocument();
  expect(within(screen.getByRole('region', { name: 'casa' })).queryByText('Esperando la autorización del sistema…')).not.toBeInTheDocument();
  expect(screen.getAllByText('Esperando la autorización del sistema…')).toHaveLength(1);
});

it('Redes: sin redes el estado vacío dice qué hacer después', async () => {
  serve([]);
  networks();
  expect(await screen.findByText(/pega un código de red/)).toBeInTheDocument();
});

it('Unirse: la confirmación recibe el foco y al cancelar vuelve al campo del código', async () => {
  serve([], { network_inspect: () => JSON.stringify(info) });
  networks();
  await userEvent.type(await screen.findByRole('textbox', { name: 'Código de red' }), 'ATENEA1-ABCDEF-GH23');
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  const confirm = await screen.findByRole('group', { name: 'Confirmar unión' });
  expect(confirm).toHaveFocus();
  await userEvent.click(within(confirm).getByRole('button', { name: 'Cancelar' }));
  expect(await screen.findByRole('textbox', { name: 'Código de red' })).toHaveFocus();
});

it('Importar: explica la instalación heredada, el nombre vacío y asocia la regla del nombre al campo', async () => {
  serve([], { network_legacy: () => '{"present":true}' });
  const { container } = networks();
  const banner = await screen.findByRole('region', { name: 'Instalación heredada' });
  expect(banner).toHaveTextContent('versión anterior de Atenea One');
  await userEvent.click(within(banner).getByRole('button', { name: 'Importar red existente' }));
  const field = screen.getByRole('textbox', { name: /si lo dejas vacío se usa el nombre del servidor/ });
  expect(field).toHaveAccessibleDescription('Letras, números, espacio, punto, guion y guion bajo.');
  expect((await axe(container)).violations).toEqual([]);
});
