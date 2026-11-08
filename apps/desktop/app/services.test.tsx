// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-024, fase 5: servicios del par, abrir sin URL, copiar e interruptor de publicación.
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import { resetHistoryStore } from './lib/history-store';
import Device from './routes/device';
import Devices from './routes/devices';
import Diagnostics from './routes/diagnostics';
import Shell from './routes/shell';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const PEER_ID = '0199ab12-3456-7def-8abc-0123456789cd';
const KEY = 'a'.repeat(64);
const net = { network_id: NET_ID, name: 'casa', server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada' };
const linux = { os: 'linux', supported: true, can_join: true, installed: true };
const windows = { os: 'windows', supported: true, can_join: false, installed: true };
const healthy = { level: 'ok', text: 'Conectado', reasons: [] };

const peer = (services?: unknown) => ({
  public_key: KEY, device_id: PEER_ID, addrs: ['10.99.0.3', 'fd99::3'], name: 'equipo-a', endpoint: '192.0.2.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path: 'direct', candidates: 2, paths: [{ kind: 'direct', ready: true, healthy: true, rtt_ms: null }], services,
});
const published = [{ port: 8080, proto: 'tcp', scheme: 'http', label: 'Storybook' }, { port: 8443, proto: 'tcp', scheme: 'https' }];

type World = { peers?: unknown[]; own?: Record<string, unknown>; client?: object; extra?: Record<string, (args: unknown) => unknown> };
function serve(w: World = {}) {
  const own = { ...w.own };
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (w.extra && cmd in w.extra) return w.extra[cmd](args) as never;
    switch (cmd) {
      case 'network_list': return JSON.stringify([net]) as never;
      case 'network_legacy': return '{"present":false}' as never;
      case 'autostart_get': return null as never;
      case 'notifications_get': return true as never;
      case 'client_status': return (w.client ?? linux) as never;
      case 'status': return JSON.stringify({
        status: { public_key: 'c'.repeat(64), addrs: ['10.99.0.2'], hostname: 'equipo-b', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, counters: {}, peers: w.peers ?? [peer(published)], ...own },
        health: healthy,
      }) as never;
      default: throw new Error(`orden inesperada: ${cmd}`);
    }
  });
  return own;
}
const calls = (cmd: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

function app(entry: string) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route element={<Shell />}>
            <Route path="devices" element={<Devices />}><Route path=":id" element={<Device />} /></Route>
            <Route path="diagnostics" element={<Diagnostics />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => { localStorage.clear(); resetHistoryStore(); vi.mocked(invoke).mockReset(); });

it('el detalle del par lista sus servicios con el aviso de que publicar no los hace alcanzables, y axe sale limpio', async () => {
  serve();
  const { container } = app(`/devices/${KEY}`);
  const section = within(await screen.findByRole('region', { name: 'Servicios' }));
  expect(section.getByText(/Que un servicio figure aquí no significa que se pueda alcanzar/)).toHaveTextContent('políticas');
  expect(section.getByText('Storybook', { exact: false })).toBeInTheDocument();
  expect(section.getByText('8443')).toBeInTheDocument();
  expect(section.getAllByRole('button', { name: /^Abrir el puerto/ })).toHaveLength(2);
  expect((await axe(container)).violations).toEqual([]);
});

it('sin servicios (o con el campo ausente) explica que el par no publica', async () => {
  serve({ peers: [peer(undefined)] });
  const { container } = app(`/devices/${KEY}`);
  expect(await screen.findByText(/no publica servicios en esta red/)).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /^Abrir el puerto/ })).not.toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});

it('Abrir llama a la orden con la red, el par y el puerto, sin ninguna URL', async () => {
  serve({ extra: { open_service: () => undefined } });
  app(`/devices/${KEY}`);
  await userEvent.click(await screen.findByRole('button', { name: 'Abrir el puerto 8443' }));
  await waitFor(() => expect(calls('open_service')).toHaveLength(1));
  expect(calls('open_service')[0][1]).toEqual({ network: NET_ID, deviceId: PEER_ID, port: 8443 });
});

it('un fallo al abrir se muestra tal cual', async () => {
  serve({ extra: { open_service: () => { throw 'el par no publica ese servicio'; } } });
  app(`/devices/${KEY}`);
  await userEvent.click(await screen.findByRole('button', { name: 'Abrir el puerto 8080' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('el par no publica ese servicio');
});

it('Copiar dirección copia el esquema publicado y la IPv4 del par', async () => {
  serve();
  app(`/devices/${KEY}`);
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Copiar dirección https://10.99.0.3:8443/' }));
  expect(await navigator.clipboard.readText()).toBe('https://10.99.0.3:8443/');
  await user.click(screen.getByRole('button', { name: 'Copiar dirección http://10.99.0.3:8080/' }));
  expect(await navigator.clipboard.readText()).toBe('http://10.99.0.3:8080/');
});

it('la lista de dispositivos indica cuántos servicios publica cada par', async () => {
  serve();
  app('/devices');
  expect(await screen.findByText('2 servicios')).toBeInTheDocument();
});

it('el interruptor pide el cambio con la red y lo refleja al releer el estado', async () => {
  const own = serve({ own: { services: [{ port: 3000, proto: 'tcp' }], services_truncated: false, services_publish: false }, extra: {
    services_publish_set: (a) => { own.services_publish = (a as { on: boolean }).on; return undefined; },
  } });
  const { container } = app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Publicar mis servicios en esta red' });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.getByText('3000')).toBeInTheDocument();
  expect(screen.getByText(/No se publican mientras el interruptor esté apagado/)).toBeInTheDocument();
  await userEvent.click(toggle);
  await waitFor(() => expect(calls('services_publish_set')[0][1]).toEqual({ network: NET_ID, on: true }));
  await waitFor(() => expect(screen.getByRole('switch', { name: 'Publicar mis servicios en esta red' })).toBeChecked());
  expect((await axe(container)).violations).toEqual([]);
});

it('cancelar el diálogo del sistema no cambia el interruptor y el error se ve', async () => {
  serve({ own: { services_publish: false }, extra: { services_publish_set: () => { throw 'Cancelado: no se ha cambiado nada'; } } });
  app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Publicar mis servicios en esta red' });
  await waitFor(() => expect(toggle).toBeEnabled());
  await userEvent.click(toggle);
  expect(await screen.findByRole('alert')).toHaveTextContent('Cancelado: no se ha cambiado nada');
  expect(toggle).not.toBeChecked();
});

it('en Windows el interruptor no se ofrece (queda desactivado y se explica)', async () => {
  serve({ client: windows, own: { services_publish: false } });
  const { container } = app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Publicar mis servicios en esta red' });
  expect(await screen.findByText(/la aplicación no puede cambiar esta opción/)).toBeInTheDocument();
  expect(toggle).toBeDisabled();
  expect(calls('services_publish_set')).toHaveLength(0);
  expect((await axe(container)).violations).toEqual([]);
});
