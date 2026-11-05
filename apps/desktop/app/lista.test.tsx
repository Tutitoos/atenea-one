// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import { readSection, saveSection } from './lib/section';
import Device from './routes/device';
import Devices from './routes/devices';
import DevicesIndex from './routes/devices-index';
import Home from './routes/home';
import Networks from './routes/networks';
import Shell from './routes/shell';
import { Sparkline } from './sparkline';
import { ToastProvider, useToast } from './toast';
import { CopyButton } from './ui';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const net = { network_id: NET_ID, name: 'casa', server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada' };
const key = (c: string) => c.repeat(64);
const peer = (k: string, name: string, path: string, addrs: string[]) => ({
  public_key: k, addrs, name, endpoint: '198.51.100.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }],
});
const PEERS = [
  peer(key('a'), 'equipo-a', 'direct', ['fd99::3', '10.99.0.3']),
  peer(key('b'), 'equipo-b', 'relay-udp', ['10.99.0.4']),
  peer(key('d'), 'equipo-d', 'none', ['10.99.0.5']),
  peer(key('e'), 'equipo-e', 'probing', ['10.99.0.6']),
];
const status = (peers = PEERS) => JSON.stringify({
  status: { public_key: key('c'), addrs: ['10.99.0.2'], hostname: 'equipo-local', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, nat: { mapping: 'unknown', reflexive: '198.51.100.9:51820' }, counters: {}, relay: { allocations: 0, entries: [], drops: {} }, peers },
  health: { level: 'ok', text: 'Conectado', reasons: [] },
});
function serve(extra: Record<string, (args: unknown) => unknown> = {}) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === 'network_list') return JSON.stringify([net]);
    if (cmd in extra) return extra[cmd](args) as never;
    return status() as never;
  });
}
const calls = (cmd: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

function app(entry = '/devices') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[entry]}>
          <Routes>
            <Route element={<Shell />}>
              <Route path="devices" element={<Devices />}>
                <Route index element={<DevicesIndex />} />
                <Route path=":id" element={<Device />} />
              </Route>
              <Route path="networks" element={<Networks />} />
            </Route>
          </Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}
const row = (name: RegExp) => screen.findByRole('link', { name });
const axeOpts = { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } };

beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });

it('cada par lleva su insignia de camino con texto, el local no, y sin datos no se dibuja una línea falsa', async () => {
  serve();
  app();
  expect(await row(/equipo-a/)).toHaveTextContent('Directo');
  expect(await row(/equipo-b/)).toHaveTextContent('Por relay UDP');
  expect(await row(/equipo-d/)).toHaveTextContent('Inactivo');
  expect(await row(/equipo-e/)).toHaveTextContent('Conectando…');
  expect(await row(/equipo-local/)).not.toHaveTextContent(/Directo|relay|Inactivo|Conectando/);
  expect(screen.queryAllByTestId('sparkline')).toHaveLength(0);
});

it('la mini-gráfica dibuja un punto por muestra y no inventa línea sin puntos o sin tráfico', () => {
  const { container, rerender } = render(<Sparkline points={[{ down: 1, up: 0 }, { down: 4, up: 2 }, { down: 0, up: 0 }]} />);
  expect(container.querySelector('polyline')!.getAttribute('points')!.split(' ')).toHaveLength(3);
  for (const points of [undefined, [], [{ down: 5, up: 1 }], [{ down: 0, up: 0 }, { down: 0, up: 0 }]]) {
    rerender(<Sparkline points={points} />);
    expect(container.querySelector('svg')).toBeNull();
  }
});

it('los filtros cuentan sobre la búsqueda, se combinan con ella, no tocan al local y se recuerdan', async () => {
  const user = userEvent.setup();
  serve();
  const { container, unmount } = app();
  await row(/equipo-a/);
  const group = within(screen.getByRole('group', { name: 'Filtrar por conexión' }));
  expect(['Todos 4', 'Conectados 2', 'Por relay 1', 'Sin conexión 2'].every((n) => group.getByRole('button', { name: n }))).toBe(true);
  await user.click(group.getByRole('button', { name: 'Por relay 1' }));
  expect(screen.getByRole('link', { name: /equipo-b/ })).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: /equipo-a/ })).not.toBeInTheDocument();
  expect(screen.getByRole('link', { name: /equipo-local/ })).toBeInTheDocument();
  expect(group.getByRole('button', { name: 'Por relay 1' })).toHaveAttribute('aria-pressed', 'true');
  expect(localStorage.getItem('atenea.devices.filter')).toBe('relay');
  expect((await axe(container, axeOpts)).violations).toEqual([]);
  unmount();

  // Al reabrir se conserva; con una búsqueda que no encaja, el vacío dice qué lo causa y «Quitar filtros» lo deshace.
  app();
  await row(/equipo-b/);
  const again = within(screen.getByRole('group', { name: 'Filtrar por conexión' }));
  expect(again.getByRole('button', { name: /^Por relay/ })).toHaveAttribute('aria-pressed', 'true');
  await user.type(screen.getByRole('searchbox'), 'equipo-a');
  expect(again.getByRole('button', { name: 'Todos 1' })).toBeInTheDocument();
  expect(again.getByRole('button', { name: 'Por relay 0' })).toBeInTheDocument();
  expect(screen.getByText('Sin resultados.')).toBeInTheDocument();
  expect(screen.getByText(/la búsqueda «equipo-a» y el filtro «Por relay»/)).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Quitar filtros' }));
  expect(screen.getByRole('searchbox')).toHaveValue('');
  expect(localStorage.getItem('atenea.devices.filter')).toBe('all');
  expect(screen.getByRole('link', { name: /equipo-a/ })).toBeInTheDocument();
});

it('«Sin conexión» reúne inactivos y conectando; un valor guardado desconocido vale «Todos»', async () => {
  localStorage.setItem('atenea.devices.filter', 'basura');
  serve();
  app();
  await row(/equipo-a/);
  await userEvent.click(screen.getByRole('button', { name: /^Sin conexión/ }));
  expect(screen.getAllByRole('link').map((l) => l.textContent).filter((t) => /equipo-[a-e]/.test(t ?? '')).map((t) => t!.slice(0, 8))).toEqual(['equipo-d', 'equipo-e']);
});

it('el menú se abre con clic derecho, se recorre con flechas y Escape devuelve el foco a la fila', async () => {
  const user = userEvent.setup();
  serve();
  const { container } = app();
  const link = await row(/equipo-a/);
  expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  fireEvent.contextMenu(link, { clientX: 20, clientY: 30 });
  const menu = within(await screen.findByRole('menu', { name: 'Acciones de equipo-a' }));
  expect(menu.getAllByRole('menuitem').map((i) => i.textContent)).toEqual(['Abrir detalle', 'Copiar dirección', 'Copiar comando ssh', 'Copiar comando ping', 'Renombrar…', 'Probar conexión']);
  expect(menu.getByRole('menuitem', { name: 'Abrir detalle' })).toHaveFocus();
  await user.keyboard('{ArrowDown}');
  expect(menu.getByRole('menuitem', { name: 'Copiar dirección' })).toHaveFocus();
  await user.keyboard('{ArrowUp}{ArrowUp}');
  expect(menu.getByRole('menuitem', { name: 'Probar conexión' })).toHaveFocus();
  expect((await axe(container, axeOpts)).violations).toEqual([]);
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  expect(link).toHaveFocus();
});

it('el menú también se abre con Shift+F10 y con el botón «⋯», y un clic fuera lo cierra', async () => {
  const user = userEvent.setup();
  serve();
  app();
  const link = await row(/equipo-b/);
  link.focus();
  await user.keyboard('{Shift>}{F10}{/Shift}');
  expect(await screen.findByRole('menu', { name: 'Acciones de equipo-b' })).toBeInTheDocument();
  await user.click(document.body);
  expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Acciones de equipo-a' }));
  expect(await screen.findByRole('menu', { name: 'Acciones de equipo-a' })).toBeInTheDocument();
});

it('las acciones de copiar usan la IPv4 preferida y avisan con un aviso', async () => {
  const user = userEvent.setup();
  serve();
  app();
  for (const [item, value, toast] of [['Copiar dirección', '10.99.0.3', 'Copiado: dirección'], ['Copiar comando ssh', 'ssh 10.99.0.3', 'Copiado: comando ssh'], ['Copiar comando ping', 'ping 10.99.0.3', 'Copiado: comando ping']]) {
    await user.click(await screen.findByRole('button', { name: 'Acciones de equipo-a' }));
    await user.click(await screen.findByRole('menuitem', { name: item }));
    expect(await navigator.clipboard.readText()).toBe(value);
    expect(await screen.findByRole('status', { name: 'Notificaciones' })).toHaveTextContent(toast);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  }
});

it('«Renombrar…» abre el detalle con el editor del nombre ya abierto', async () => {
  const user = userEvent.setup();
  serve();
  app();
  await user.click(await screen.findByRole('button', { name: 'Acciones de equipo-b' }));
  await user.click(await screen.findByRole('menuitem', { name: 'Renombrar…' }));
  const box = await within(screen.getByRole('main')).findByRole('textbox', { name: 'Nombre local' });
  expect(box).toHaveFocus();
});

it('«Probar conexión» abre el detalle y lanza una sola prueba con la IPv4 preferida', async () => {
  const user = userEvent.setup();
  serve({ device_ping: () => '{"sent":5,"received":5,"loss_pct":0}' });
  app();
  await user.click(await screen.findByRole('button', { name: 'Acciones de equipo-a' }));
  await user.click(await screen.findByRole('menuitem', { name: 'Probar conexión' }));
  expect(await screen.findByText('Pérdida: 0 %')).toBeInTheDocument();
  // Dejar pasar una lectura más del agente (2 s): la prueba no se repite.
  await new Promise((r) => setTimeout(r, 2300));
  expect(calls('device_ping')).toEqual([['device_ping', { network: NET_ID, address: '10.99.0.3' }]]);
}, 10000);

it('el dispositivo local solo ofrece abrir el detalle y copiar su dirección', async () => {
  serve();
  app();
  await userEvent.click(await screen.findByRole('button', { name: 'Acciones de equipo-local' }));
  expect(within(await screen.findByRole('menu')).getAllByRole('menuitem').map((i) => i.textContent)).toEqual(['Abrir detalle', 'Copiar dirección']);
});

it('los avisos de copiar: éxito, fallo, desaparecen solos y se apilan como mucho tres', async () => {
  const user = userEvent.setup();
  function Many() { const toast = useToast()!; return <button type="button" onClick={() => ['uno', 'dos', 'tres', 'cuatro'].forEach(toast)}>avisar</button>; }
  render(<ToastProvider><CopyButton value="x" label="huella" /><Many /></ToastProvider>);
  const region = screen.getByRole('status', { name: 'Notificaciones' });
  expect(region).toHaveAttribute('aria-live', 'polite');
  await user.click(screen.getByRole('button', { name: 'Copiar huella' }));
  await waitFor(() => expect(region).toHaveTextContent('Copiado: huella'));
  await waitFor(() => expect(region).toBeEmptyDOMElement(), { timeout: 3000 });
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: () => Promise.reject(new Error('denegado')) } });
  await user.click(screen.getByRole('button', { name: 'Copiar huella' }));
  await waitFor(() => expect(region).toHaveTextContent('No se pudo copiar'));
  // Con aviso, el anuncio no se duplica en un `status` oculto aparte.
  expect(screen.getAllByRole('status')).toHaveLength(1);
  await user.click(screen.getByRole('button', { name: 'avisar' }));
  expect(region.children).toHaveLength(3);
  expect(region).toHaveTextContent('cuatro');
  expect(region).not.toHaveTextContent('uno');
});

it('la sección recordada solo admite las conocidas y Inicio abre la última', async () => {
  expect(readSection()).toBe('/devices');
  localStorage.setItem('atenea.section', '/secreto');
  expect(readSection()).toBe('/devices');
  saveSection('/welcome');
  saveSection('/devices/abc');
  expect(readSection()).toBe('/devices');
  saveSection('/servers/panel.example.org');
  expect(readSection()).toBe('/servers');

  serve();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/']}>
        <Routes><Route index element={<Home />} /><Route path="servers" element={<p>sección servidores</p>} /><Route path="devices" element={<p>sección dispositivos</p>} /></Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  expect(await screen.findByText('sección servidores')).toBeInTheDocument();
});

it('Inicio cae en Dispositivos si lo guardado no es válido, y navegar por el shell guarda la sección', async () => {
  localStorage.setItem('atenea.section', '/secreto');
  serve();
  const { unmount } = render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={['/']}>
        <Routes><Route index element={<Home />} /><Route path="devices" element={<p>sección dispositivos</p>} /></Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  expect(await screen.findByText('sección dispositivos')).toBeInTheDocument();
  unmount();
  app('/networks');
  await screen.findByRole('navigation', { name: 'Secciones' });
  expect(localStorage.getItem('atenea.section')).toBe('/networks');
});
