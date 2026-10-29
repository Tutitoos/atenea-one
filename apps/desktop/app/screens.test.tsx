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
import Networks from './routes/networks';
import Shell from './routes/shell';
import Servers from './routes/servers';
import Server from './routes/server';
import type { Peer } from './lib/status';
import { TrafficHistory } from './device-history';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const peer = (key: string, addr: string, path: string, name?: string) => ({
  public_key: key, addrs: [addr], name, endpoint: '192.0.2.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }],
});
const degraded = { level: 'degraded', text: 'Degradado', reasons: ['algún dispositivo por relay'] };
const server = { sync: { url: 'https://control.example.test', last_ok_unix: Math.floor(Date.now() / 1000) - 5, last_error: null, sync_ok: 40, sync_denied: 1, sync_errors: 2 }, coordinator: { url: 'https://control.example.test', last_announce_ok_unix: Math.floor(Date.now() / 1000) - 3, last_error: null, announces: 120, events: 9 } };
const relay = { allocations: 1, drops: {}, entries: [{ peer: 'aa', transport: 'Udp', relay: 'relay-uno', state: 'ready', rtt_ms: 42, tx_bytes: 2048, rx_bytes: 1024 }] };
const agent = (peers: Peer[] = [peer('a'.repeat(64), '10.77.0.3', 'direct', 'servidor-vps'), peer('b'.repeat(64), '10.77.0.4', 'relay-udp')], health: object = degraded, extra: object = { ...server, relay }) => JSON.stringify({
  status: { public_key: 'c'.repeat(64), addrs: ['10.77.0.2'], hostname: 'MacBook de prueba', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, nat: { mapping: 'unknown', reflexive: '203.0.113.9:51820' }, counters: { tx_init: 3 }, relay: { allocations: 0, entries: [], drops: {} }, peers, ...extra },
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
            <Route path="networks" element={<Networks />} />
            <Route path="servers" element={<Servers />} />
            <Route path="servers/:host" element={<Server />} />
            <Route path="diagnostics" element={<Diagnostics />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const OTHER_ID = '0199ab12-3456-7def-8abc-0123456789cd';
const net = (over: object = {}) => ({ network_id: NET_ID, name: 'casa', server: 'control.example.test', url: 'https://control.example.test', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada', ...over });
const info = { name: 'casa', server: 'control.example.test', network_id: NET_ID, root_fingerprint: '6c4b-fd0b-2585-9aae', has_token: false };
const CODE = 'ATENEA1-ABCDEF-GH23';

/** Responde como Rust: `status` con el estado (o el fallo), `network_list` con las redes y el resto por orden. */
function serve(statusReply: string, networks: object[] = [net()], fail?: string, extra: Record<string, (args: unknown) => unknown> = {}) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === 'network_list') return JSON.stringify(networks);
    if (cmd === 'network_legacy' && !(cmd in extra)) return '{"present":false}' as never;
    if (cmd === 'autostart_get' && !(cmd in extra)) return null as never;
    if (cmd in extra) return extra[cmd](args) as never;
    if (fail) throw fail;
    return statusReply as never;
  });
}
const calls = (cmd: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });

it('la banda de avisos solo aparece si hay motivos que contar', async () => {
  // Caso real de Rust: un par por relay degrada el estado pero no aporta ningún motivo textual.
  serve(agent(undefined, { level: 'degraded', text: 'Degradado: algún dispositivo por relay', reasons: [] }));
  const { unmount } = app();
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Degradado'));
  expect(screen.queryByRole('list', { name: 'Avisos' })).not.toBeInTheDocument();
  unmount();
  serve(agent(undefined, { level: 'down', text: 'Sin conexión: sin lease activo', reasons: ['sin lease activo'] }));
  app();
  expect(await screen.findByRole('list', { name: 'Avisos' })).toHaveTextContent('sin lease activo');
});


it('el panel sale de Dispositivos y su detalle vive en Servidores con servicios, NAT y relays', async () => {
  serve(agent());
  app();
  const groups = await screen.findAllByRole('heading', { level: 2, name: /^(Local|Servidor|Dispositivos)$/ });
  expect(groups.map((g) => g.textContent)).toEqual(['Local', 'Dispositivos']);
  expect(screen.queryByRole('link', { name: /control\.example\.test/ })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('link', { name: 'Servidores' }));
  await userEvent.click(await screen.findByRole('link', { name: /control\.example\.test/ }));
  const detail = within(screen.getByRole('main'));
  expect(await detail.findByRole('heading', { level: 1, name: 'control.example.test' })).toBeInTheDocument();
  expect(await detail.findByText('Conectado')).toBeInTheDocument();
  expect(detail.getByText('40 correctas · 1 denegadas · 2 con error')).toBeInTheDocument();
  expect(detail.getByText('relay-uno · UDP')).toBeInTheDocument();
  expect(detail.getByText(/ready · 42 ms/)).toBeInTheDocument();
  expect(detail.getByText('203.0.113.9:51820')).toBeInTheDocument();
});

it('un error del control plane se muestra en el detalle del servidor', async () => {
  serve(agent(undefined, degraded, { sync: { ...server.sync, last_error: 'acceso denegado' }, coordinator: server.coordinator }));
  app('/servers/control.example.test');
  expect(await screen.findByText('Control plane: acceso denegado')).toBeInTheDocument();
});
it('abre en este dispositivo y al elegir un par enseña su camino, direcciones y tráfico', async () => {
  serve(agent());
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
  serve(agent());
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
  serve(agent());
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
  serve('', [net()], 'Agente parado o sin acceso al estado');
  app();
  expect(await screen.findAllByText('Agente parado o sin acceso al estado')).not.toHaveLength(0);
  expect(screen.getByRole('status')).toHaveTextContent('Sin datos');
  expect(screen.queryByRole('link', { name: /Este dispositivo/ })).not.toBeInTheDocument();
});

it('una respuesta con otra forma se rechaza en la frontera', async () => {
  serve(JSON.stringify({ status: { peers: 3 } }));
  app();
  expect(await screen.findAllByText('El agente respondió con un formato inesperado.')).not.toHaveLength(0);
  expect(screen.getByRole('status')).toHaveTextContent('Sin datos');
});

it('sin redes unidas no hay selector ni estado: Dispositivos y Diagnóstico enseñan el estado vacío y la cabecera «Sin redes»', async () => {
  serve(agent(), []);
  const { unmount } = app();
  await screen.findByRole('navigation', { name: 'Secciones' });
  expect(await screen.findByText(/Sin redes\. Únete a una red desde/)).toBeInTheDocument();
  expect(within(screen.getByRole('main')).getByRole('link', { name: 'Redes' })).toHaveAttribute('href', '/networks');
  expect(screen.getByRole('status')).toHaveTextContent('Sin redes');
  expect(screen.queryByRole('combobox', { name: 'Red' })).not.toBeInTheDocument();
  expect(calls('status')).toHaveLength(0);
  unmount();
  app('/diagnostics');
  expect(await screen.findByText(/Sin redes\. Únete a una red desde/)).toBeInTheDocument();
  expect(calls('status')).toHaveLength(0);
});

it('el selector solo ofrece redes reales: abre con la primera y pide el estado de la elegida', async () => {
  serve(agent(), [net(), net({ network_id: OTHER_ID, name: 'trabajo' })]);
  app();
  const select = await screen.findByRole('combobox', { name: 'Red' });
  expect(within(select).getAllByRole('option').map((o) => o.textContent)).toEqual(['casa', 'trabajo']);
  expect(select).toHaveValue(NET_ID);
  await waitFor(() => expect(calls('status')[0][1]).toEqual({ network: NET_ID }));
  await userEvent.selectOptions(select, 'trabajo');
  await waitFor(() => expect(calls('status').at(-1)![1]).toEqual({ network: OTHER_ID }));
  expect(localStorage.getItem('atenea.network')).toBe(OTHER_ID);
});

it('la red guardada se respeta y, si desaparece, se pasa a la primera', async () => {
  localStorage.setItem('atenea.network', OTHER_ID);
  serve(agent(), [net(), net({ network_id: OTHER_ID, name: 'trabajo' })]);
  const { unmount } = app();
  expect(await screen.findByRole('combobox', { name: 'Red' })).toHaveValue(OTHER_ID);
  unmount();
  vi.mocked(invoke).mockReset();
  serve(agent(), [net()]);
  app();
  expect(await screen.findByRole('combobox', { name: 'Red' })).toHaveValue(NET_ID);
  await waitFor(() => expect(calls('status').at(-1)![1]).toEqual({ network: NET_ID }));
});

it('redes: enseña el estado de cada una y conectar/desconectar pide la operación con su id', async () => {
  serve(agent(), [net(), net({ network_id: '0199ab12-3456-7def-8abc-0123456789cd', name: 'trabajo', state: 'detenida' })], undefined, { network_apply: () => '' });
  app('/networks');
  const casa = await screen.findByRole('region', { name: 'casa' });
  expect(within(casa).getByText(/Conectada/)).toBeInTheDocument();
  await userEvent.click(within(casa).getByRole('button', { name: 'Desconectar' }));
  await waitFor(() => expect(calls('network_apply')).toEqual([['network_apply', { op: 'down', network: NET_ID }]]));
  await userEvent.click(within(screen.getByRole('region', { name: 'trabajo' })).getByRole('button', { name: 'Conectar' }));
  await waitFor(() => expect(calls('network_apply').at(-1)).toEqual(['network_apply', { op: 'up', network: '0199ab12-3456-7def-8abc-0123456789cd' }]));
});

it('un conflicto de red muestra su motivo en rojo', async () => {
  serve(agent(), [net({ state: 'conflicto', detail: 'network_overlap: solapa con otra interfaz' })]);
  app('/networks');
  expect(await screen.findByText('network_overlap: solapa con otra interfaz')).toBeInTheDocument();
});

it('salir de una red exige confirmar y se puede cancelar sin llamar a nada', async () => {
  serve(agent(), [net()], undefined, { network_apply: () => '' });
  app('/networks');
  await userEvent.click(await screen.findByRole('button', { name: 'Salir de la red…' }));
  const dialog = screen.getByRole('alertdialog', { name: 'Salir de casa' });
  expect(dialog).toHaveTextContent('Se revocará el dispositivo en el servidor');
  await userEvent.click(within(dialog).getByRole('button', { name: 'Cancelar' }));
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(0);
  await userEvent.click(screen.getByRole('button', { name: 'Salir de la red…' }));
  await userEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Salir de la red' }));
  await waitFor(() => expect(calls('network_apply')).toEqual([['network_apply', { op: 'leave', network: NET_ID }]]));
});

it('unirse: primero se revisa el código y no se cambia nada hasta confirmar', async () => {
  serve(agent(), [], undefined, {
    network_inspect: () => JSON.stringify(info),
    network_apply: () => `red=${NET_ID}\ndispositivo=x\nhuella_dispositivo=aaaa-bbbb-cccc\nestado=pendiente_de_aprobacion`,
  });
  app('/networks');
  await userEvent.type(await screen.findByRole('textbox', { name: 'Código de red' }), CODE);
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));

  const confirm = await screen.findByRole('group', { name: 'Confirmar unión' });
  expect(confirm).toHaveTextContent('control.example.test');
  expect(confirm).toHaveTextContent('6c4b-fd0b-2585-9aae');
  expect(confirm).toHaveTextContent('por otro canal');
  expect(confirm).toHaveTextContent('un administrador debe aprobar');
  expect(calls('network_inspect')).toEqual([['network_inspect', { code: CODE }]]);
  expect(calls('network_apply')).toHaveLength(0);

  await userEvent.click(within(confirm).getByRole('button', { name: 'Unirme a esta red' }));
  await waitFor(() => expect(calls('network_apply')).toEqual([['network_apply', { op: 'join', code: CODE }]]));
  expect(await screen.findByText(/Solicitud enviada a «casa»/)).toBeInTheDocument();
  expect(screen.getByText('aaaa-bbbb-cccc')).toBeInTheDocument();
});

it('un código no válido o un fallo de autorización se muestran y no dejan nada a medias', async () => {
  serve(agent(), [], undefined, {
    network_inspect: (args) => { if ((args as { code: string }).code === 'basura') throw 'código no válido: no es un código de red'; return JSON.stringify(info); },
    network_apply: () => { throw 'Cancelado: no se ha cambiado nada'; },
  });
  app('/networks');
  const box = await screen.findByRole('textbox', { name: 'Código de red' });
  await userEvent.type(box, 'basura');
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('código no válido');
  expect(screen.queryByRole('group', { name: 'Confirmar unión' })).not.toBeInTheDocument();

  await userEvent.clear(box);
  await userEvent.type(box, CODE);
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  await userEvent.click(await screen.findByRole('button', { name: 'Unirme a esta red' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Cancelado: no se ha cambiado nada');
  // Sigue en la confirmación: se puede reintentar o cancelar.
  expect(screen.getByRole('group', { name: 'Confirmar unión' })).toBeInTheDocument();
});

it('sin instalación heredada no hay banner; con ella, importar confirma antes de llamar y manda el nombre opcional', async () => {
  serve(agent());
  const { unmount } = app('/networks');
  await screen.findByRole('region', { name: 'casa' });
  expect(screen.queryByRole('region', { name: 'Instalación heredada' })).not.toBeInTheDocument();
  unmount();

  serve(agent(), [], undefined, { network_legacy: () => '{"present":true}', network_apply: () => '' });
  app('/networks');
  const banner = await screen.findByRole('region', { name: 'Instalación heredada' });
  expect(banner).toHaveTextContent('Hay una instalación heredada pendiente de importar');
  await userEvent.click(within(banner).getByRole('button', { name: 'Importar red existente' }));
  const dialog = within(banner).getByRole('alertdialog', { name: 'Importar red existente' });
  expect(dialog).toHaveTextContent(/se parará la instancia actual/i);
  expect(dialog).toHaveTextContent('unos segundos');
  expect(calls('network_apply')).toHaveLength(0);
  // Cancelar no llama a nada.
  await userEvent.click(within(dialog).getByRole('button', { name: 'Cancelar' }));
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(0);

  await userEvent.click(screen.getByRole('button', { name: 'Importar red existente' }));
  await userEvent.type(screen.getByRole('textbox', { name: /Nombre de la red/ }), ' casa piloto ');
  await userEvent.click(screen.getByRole('button', { name: 'Importar' }));
  await waitFor(() => expect(calls('network_apply')).toEqual([['network_apply', { op: 'migrate', name: 'casa piloto' }]]));
});

it('importar sin nombre no manda `name` y un rechazo de Rust se muestra sin cerrar la confirmación', async () => {
  serve(agent(), [], undefined, { network_legacy: () => '{"present":true}', network_apply: () => { throw 'nombre no válido'; } });
  app('/networks');
  await userEvent.click(await screen.findByRole('button', { name: 'Importar red existente' }));
  await userEvent.click(screen.getByRole('button', { name: 'Importar' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('nombre no válido');
  expect(calls('network_apply')).toEqual([['network_apply', { op: 'migrate' }]]);
  expect(screen.getByRole('alertdialog', { name: 'Importar red existente' })).toBeInTheDocument();
});

it('el device_id real del agente (lista de 16 bytes) no rompe el estado y llega al panel como UUID', async () => {
  const bytes = [0x01, 0x99, 0xab, 0x12, 0x34, 0x56, 0x7d, 0xef, 0x8a, 0xbc, 0x01, 0x23, 0x45, 0x67, 0x89, 0xcd];
  serve(agent([{ ...peer('aa', '10.77.0.3', 'direct'), device_id: bytes } as never]), [net()], undefined, { open_panel: () => undefined });
  app(`/devices/aa`);
  await userEvent.click(await screen.findByRole('button', { name: /panel/i }));
  await waitFor(() => expect(calls('open_panel')[0][1]).toEqual({ network: NET_ID, deviceId: OTHER_ID }));
  // Un valor irreconocible se ignora sin invalidar el estado: la pantalla sigue mostrando al dispositivo.
  serve(agent([{ ...peer('bb', '10.77.0.4', 'direct'), device_id: { raro: true } } as never]), [net()]);
  app(`/devices/bb`);
  expect(await screen.findByRole('link', { name: /10\.77\.0\.4/ })).toBeInTheDocument();
});

it('las tres acciones de copiar de una dirección se distinguen: icono propio y etiqueta visible', async () => {
  serve(agent());
  app(`/devices/${'a'.repeat(64)}`);
  const ssh = await screen.findAllByRole('button', { name: 'Copiar comando SSH' });
  const ping = screen.getAllByRole('button', { name: 'Copiar comando ping' });
  const addr = screen.getAllByRole('button', { name: /^Copiar IPv4 / });
  expect(within(ssh[0]).getByText('ssh')).toBeInTheDocument();
  expect(within(ping[0]).getByText('ping')).toBeInTheDocument();
  expect(addr[0].textContent).toBe('');
  const shape = (b: HTMLElement) => b.querySelector('svg')!.innerHTML;
  expect(new Set([shape(addr[0]), shape(ssh[0]), shape(ping[0])]).size).toBe(3);
});

it('las pantallas no tienen violaciones de accesibilidad detectables', async () => {
  serve(agent());
  for (const entry of ['/devices', `/devices/${'a'.repeat(64)}`, '/networks', '/diagnostics', '/servers', '/servers/control.example.test']) {
    const { container, unmount } = app(entry);
    await screen.findByRole('navigation', { name: 'Secciones' });
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Degradado'));
    // jsdom no calcula layout ni contraste: el contraste necesita comprobación en un webview real.
    expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
    unmount();
  }
});

it('agrupa las redes por host exacto también en el selector', async () => {
  serve(agent(), [net({ url: 'HTTPS://CONTROL.EXAMPLE.TEST:443/api' }), net({ network_id: OTHER_ID, name: 'trabajo' }), net({ network_id: '0199ab12-3456-7def-8abc-0123456789ef', name: 'otra', url: 'https://otro.example.test' })]);
  app('/servers');
  const main = within(screen.getByRole('main'));
  expect(await main.findByRole('link', { name: /control.example.test 2 redes/ })).toBeInTheDocument();
  expect(main.getByRole('link', { name: /otro.example.test 1 red/ })).toBeInTheDocument();
  expect(main.getAllByRole('link')).toHaveLength(2);
  const select = screen.getByRole('combobox', { name: 'Red' });
  const groups = within(select).getAllByRole('group');
  expect(groups.map((g) => g.getAttribute('label'))).toEqual(['control.example.test', 'otro.example.test']);
  expect(within(groups[0]).getAllByRole('option')).toHaveLength(2);
});

it('el servidor prueba otra red conectada si la primera no responde y no usa redes ajenas', async () => {
  serve(agent(), [net(), net({ network_id: OTHER_ID, name: 'trabajo' }), net({ network_id: '0199ab12-3456-7def-8abc-0123456789ef', url: 'https://otro.example.test', name: 'otra' })], undefined, { status: (args) => {
    if (args && typeof args === 'object' && 'network' in args && args.network === NET_ID) throw 'sin acceso';
    return agent();
  } });
  app('/servers/control.example.test');
  expect(await within(screen.getByRole('main')).findByText('40 correctas · 1 denegadas · 2 con error')).toBeInTheDocument();
  expect(calls('status').every(([, args]) => args && typeof args === 'object' && 'network' in args && [NET_ID, OTHER_ID].includes(String(args.network)))).toBe(true);
});

it('sin una red que responda no inventa servicios; permite conectar y ver dispositivos de su red', async () => {
  serve(agent(), [net({ state: 'detenida' }), net({ network_id: OTHER_ID, name: 'trabajo', state: 'detenida' })], undefined, { network_apply: () => '' });
  app('/servers/control.example.test');
  expect(await screen.findByText('Sin redes conectadas a este servidor')).toBeInTheDocument();
  expect(screen.queryByText('Sincronizaciones')).not.toBeInTheDocument();
  const trabajo = within(screen.getByRole('region', { name: 'trabajo' }));
  await userEvent.click(trabajo.getByRole('button', { name: 'Conectar' }));
  await waitFor(() => expect(calls('network_apply').at(-1)).toEqual(['network_apply', { op: 'up', network: OTHER_ID }]));
  await userEvent.click(trabajo.getByRole('link', { name: 'Ver dispositivos' }));
  expect(await screen.findByRole('heading', { level: 1, name: 'Dispositivos' })).toBeInTheDocument();
  expect(screen.getByRole('combobox', { name: 'Red' })).toHaveValue(OTHER_ID);
  expect(localStorage.getItem('atenea.network')).toBe(OTHER_ID);
});

it('si ningún socket de las redes conectadas responde muestra el estado vacío del servidor', async () => {
  serve('', [net()], 'sin acceso');
  app('/servers/control.example.test');
  expect(await screen.findByText('Sin redes conectadas a este servidor')).toBeInTheDocument();
  expect(screen.queryByText('Sincronizaciones')).not.toBeInTheDocument();
});

it('un par sin camino es Inactivo y explica la conexión bajo demanda sin degradar la cabecera', async () => {
  serve(agent([peer('a'.repeat(64), '10.77.0.3', 'none')], { level: 'ok', text: 'Conectado', reasons: [] }));
  app(`/devices/${'a'.repeat(64)}`);
  expect(await screen.findByText('Inactivo', { selector: 'span' })).toBeInTheDocument();
  expect(screen.getByText('Sin camino activo: el agente lo establece al enviar tráfico. Si sigue sin conexión al usarlo, comprueba que el dispositivo esté encendido.')).toBeInTheDocument();
  expect(screen.getByRole('status')).toHaveTextContent('Conectado');
});

it('copiar usa iconos etiquetados para IPv4, IPv6 y clave, con check temporal', async () => {
  const user = userEvent.setup();
  serve(agent([], { level: 'ok', text: 'Conectado', reasons: [] }, { addrs: ['10.77.0.2', 'fd77::2'] }));
  app();
  const button = await screen.findByRole('button', { name: 'Copiar IPv4 10.77.0.2' });
  expect(button).toHaveAttribute('title', 'Copiar IPv4 10.77.0.2');
  expect(button.querySelector('svg rect')).not.toBeNull();
  expect(button).not.toHaveTextContent('Copiar');
  await user.click(button);
  expect(await navigator.clipboard.readText()).toBe('10.77.0.2');
  expect(button).toHaveAttribute('title', 'Copiado');
  expect(button.querySelector('svg rect')).toBeNull();
  await waitFor(() => expect(button).toHaveAttribute('title', 'Copiar IPv4 10.77.0.2'), { timeout: 2500 });
  await user.click(screen.getByRole('button', { name: 'Copiar IPv6 fd77::2' }));
  expect(await navigator.clipboard.readText()).toBe('fd77::2');
  await user.click(screen.getByRole('button', { name: 'Copiar clave pública' }));
  expect(await navigator.clipboard.readText()).toBe('c'.repeat(64));
});

const serverInfo = (days = 10, skew = 45) => ({
  control_plane: { version: '0.1.6', commit: 'abcdef1234567890' },
  server_time_unix: Math.floor(Date.now() / 1000), clock_skew_secs: skew,
  issuer_cert_not_after_unix: Math.floor(Date.now() / 1000) + days * 86400,
  fetched_unix: Math.floor(Date.now() / 1000), last_error: null,
});

it('el rol recibido separa servidores de dispositivos y de sus búsquedas sin adivinar nombres', async () => {
  serve(agent([{ ...peer('a'.repeat(64), '10.77.0.3', 'none', 'servidor-vps'), role: 'server' }, peer('b'.repeat(64), '10.77.0.4', 'direct', 'servidor-antiguo')]));
  app();
  await screen.findByTitle('Cambiar nombre local');
  expect(screen.queryByRole('link', { name: /servidor-vps/ })).not.toBeInTheDocument();
  expect(screen.getByRole('link', { name: /servidor-antiguo/ })).toBeInTheDocument();
  await userEvent.type(screen.getByRole('searchbox'), '10.77.0.3');
  expect(screen.getByText('Sin resultados.')).toBeInTheDocument();
});

it('muestra dispositivos del servidor de todas sus redes legibles y selecciona su red', async () => {
  serve(agent(), [net(), net({ network_id: OTHER_ID, name: 'trabajo' })], undefined, { status: (args) => {
    const second = args && typeof args === 'object' && 'network' in args && args.network === OTHER_ID;
    return agent([{ ...peer(second ? 'b'.repeat(64) : 'a'.repeat(64), second ? '10.78.0.3' : '10.77.0.3', 'none', second ? 'servidor-trabajo' : 'servidor-casa'), role: 'server' }]);
  } });
  app('/servers/control.example.test');
  await screen.findByRole('heading', { name: 'Dispositivos del servidor' });
  const link = await screen.findByRole('link', { name: /servidor-trabajo.*10.78.0.3.*Inactivo.*trabajo/ });
  expect(screen.getByRole('link', { name: /servidor-casa.*10.77.0.3.*Inactivo.*casa/ })).toBeInTheDocument();
  await userEvent.click(link);
  expect(screen.getByRole('combobox', { name: 'Red' })).toHaveValue(OTHER_ID);
  expect(await screen.findByTitle('Cambiar nombre local')).toHaveTextContent('servidor-trabajo');
});

it.each([undefined, null])('el servidor antiguo (%s) no inventa información', async (info) => {
  serve(agent(undefined, degraded, { ...server, server: info }));
  app('/servers/control.example.test');
  expect(await screen.findByText('Este servidor aún no informa de su versión ni de la caducidad del certificado')).toBeInTheDocument();
});

it.each([[15, 'text-mut'], [14, 'text-warn'], [3, 'text-bad'], [0, 'text-bad'], [-1, 'text-bad']])('certificado a %s días: tono e información del servidor', async (days, tone) => {
  serve(agent(undefined, degraded, { ...server, server: serverInfo(Number(days)) }));
  const { container } = app('/servers/control.example.test');
  expect(await screen.findByText('0.1.6 · abcdef12')).toBeInTheDocument();
  expect(screen.getByText(/45 s.*leases dependen del reloj/)).toHaveClass('text-warn');
  expect(screen.getByText(Number(days) <= 0 ? /ha caducado/ : new RegExp(`Quedan ${days} días`))).toHaveClass(String(tone));
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('los servicios opcionales se muestran solo cuando llegan y el desfase pequeño es neutro', async () => {
  serve(agent(undefined, degraded, { ...server, server: { ...serverInfo(20, -30), coordinator: { version: '0.1.5', commit: '1234567890' }, relay: { version: '0.1.4', commit: '9876543210' } } }));
  app('/servers/control.example.test');
  expect(await screen.findByText('0.1.5 · 12345678')).toBeInTheDocument();
  expect(screen.getByText('0.1.4 · 98765432')).toBeInTheDocument();
  expect(screen.getByText('-30 s')).toHaveClass('text-mut');
});

it('el certificado próximo a caducar pone una insignia en su servidor y no en otro', async () => {
  serve(agent(), [net(), net({ network_id: OTHER_ID, url: 'https://otro.example.test' })], undefined, { status: (args) => agent(undefined, degraded, { ...server, server: args && typeof args === 'object' && 'network' in args && args.network === NET_ID ? serverInfo(14) : null }) });
  const { container } = app('/servers');
  const badge = await screen.findByText('Certificado próximo a caducar');
  expect(badge.closest('a')).toHaveAttribute('href', '/servers/control.example.test');
  expect(screen.getAllByText('Certificado próximo a caducar')).toHaveLength(1);
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it.each([undefined, { failures: 0, hold_secs: null, relay_wanted: false }, { failures: 2, hold_secs: 8, relay_wanted: true }, { failures: 0, hold_secs: 0, relay_wanted: false }])('diagnóstico de reintentos opcional: %s', async (probe) => {
  serve(agent([{ ...peer('a'.repeat(64), '10.77.0.3', 'none'), probe }]));
  const { container } = app(`/devices/${'a'.repeat(64)}`);
  await screen.findByTitle('Cambiar nombre local');
  if (probe && (probe.failures > 0 || probe.hold_secs != null)) {
    expect(screen.getByText('Reintentos de conexión')).toBeInTheDocument();
    expect(screen.getByText(`${probe.failures} fallos · ${probe.hold_secs} s hasta el próximo intento`)).toBeInTheDocument();
  } else expect(screen.queryByText('Reintentos de conexión')).not.toBeInTheDocument();
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('la caducidad pertenece a cada red del servidor, no a la primera red que responda', async () => {
  serve(agent(), [net(), net({ network_id: OTHER_ID, name: 'trabajo' })], undefined, { status: (args) => {
    const second = args && typeof args === 'object' && 'network' in args && args.network === OTHER_ID;
    return agent([], degraded, { ...server, server: serverInfo(second ? 3 : 20, second ? -31 : 0) });
  } });
  app('/servers/control.example.test');
  const casa = await screen.findByRole('region', { name: 'Información de casa' });
  const trabajo = await screen.findByRole('region', { name: 'Información de trabajo' });
  expect(within(casa).getByText(/Quedan 20 días/)).toHaveClass('text-mut');
  expect(within(trabajo).getByText(/Quedan 3 días/)).toHaveClass('text-bad');
  expect(within(trabajo).getByText(/-31 s.*leases dependen del reloj/)).toHaveClass('text-warn');
});

it('una información sin certificado no inventa una caducidad ni una insignia', async () => {
  serve(agent([], degraded, { ...server, server: { ...serverInfo(), issuer_cert_not_after_unix: null } }));
  const { unmount } = app('/servers/control.example.test');
  expect(await screen.findByText('no informada')).toBeInTheDocument();
  unmount();
  app('/servers');
  await screen.findByRole('link', { name: /control.example.test 1 red/ });
  expect(screen.queryByText('Certificado próximo a caducar')).not.toBeInTheDocument();
});

it.each([0, 40])('prueba conexión con pérdida del %s %% y refresca', async (loss) => {
  serve(agent(), [net()], undefined, { device_ping: () => JSON.stringify({ sent: 5, received: loss ? 3 : 5, loss_pct: loss, min_ms: 1, avg_ms: 2, max_ms: 3, extra: true }) });
  const { container } = app(`/devices/${'a'.repeat(64)}`);
  await userEvent.click(await screen.findByRole('button', { name: 'Probar conexión' }));
  expect(await screen.findByText(`Pérdida: ${loss} %`)).toBeInTheDocument();
  expect(screen.getByText(/Mín\/med\/máx: 1 \/ 2 \/ 3 ms/)).toBeInTheDocument();
  expect(calls('device_ping')[0][1]).toEqual({ network: NET_ID, address: '10.77.0.3' });
  await waitFor(() => expect(calls('status').length).toBeGreaterThan(1));
  expect((await axe(container)).violations).toEqual([]);
});

it('muestra espera y error exacto de ping, también con IPv6', async () => {
  // El compilador usa ES2022, sin Promise.withResolvers.
  let reject!: (reason: string) => void;
  const promise = new Promise<never>((_, fail) => { reject = fail; });
  serve(agent([peer('aa', 'fd00::3', 'none')]), [net()], undefined, { device_ping: () => promise });
  app('/devices/aa');
  await userEvent.click(await screen.findByRole('button', { name: 'Probar conexión' }));
  expect(screen.getByRole('button', { name: 'Probando…' })).toBeDisabled();
  reject('ping: destino no accesible');
  expect(await screen.findByRole('alert')).toHaveTextContent('ping: destino no accesible');
  expect(calls('device_ping')[0][1]).toEqual({ network: NET_ID, address: 'fd00::3' });
});

it('copia comandos sin corchetes y no ofrece probar el dispositivo local', async () => {
  const user = userEvent.setup();
  serve(agent([peer('aa', 'fd00::3', 'direct')]));
  const { unmount } = app('/devices/aa');
  await user.click(await screen.findByRole('button', { name: 'Copiar comando SSH' }));
  expect(await navigator.clipboard.readText()).toBe('ssh fd00::3');
  await user.click(screen.getByRole('button', { name: 'Copiar comando ping' }));
  expect(await navigator.clipboard.readText()).toBe('ping fd00::3');
  unmount();
  app('/devices/self');
  await screen.findByRole('heading', { name: 'MacBook de prueba' });
  expect(screen.queryByRole('button', { name: 'Probar conexión' })).not.toBeInTheDocument();
});

it('abre el panel con el identificador del par y muestra errores sin alterarlos', async () => {
  serve(agent([{ ...peer('aa', '10.77.0.3', 'direct'), device_id: OTHER_ID }]), [net()], undefined, { open_panel: () => { throw 'No se pudo abrir el navegador'; } });
  app('/devices/aa');
  await userEvent.click(await screen.findByRole('button', { name: 'Abrir en el panel' }));
  expect(calls('open_panel')[0][1]).toEqual({ network: NET_ID, deviceId: OTHER_ID });
  expect(await screen.findByRole('alert')).toHaveTextContent('No se pudo abrir el navegador');
});

it('abre el panel de la red del servidor sin dispositivo', async () => {
  serve(agent(), [net()], undefined, { open_panel: () => undefined });
  app('/servers/control.example.test');
  await userEvent.click(await screen.findByRole('button', { name: 'Abrir en el panel' }));
  expect(calls('open_panel')[0][1]).toEqual({ network: NET_ID, deviceId: null });
});

it('lee y cambia la preferencia de notificaciones', async () => {
  serve(agent(), [net()], undefined, { notifications_get: () => true, notifications_set: () => undefined });
  const { container } = app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Notificaciones del sistema' });
  await waitFor(() => expect(toggle).toBeChecked());
  await userEvent.click(toggle);
  await waitFor(() => expect(toggle).not.toBeChecked());
  expect(calls('notifications_set')[0][1]).toEqual({ on: false });
  expect((await axe(container)).violations).toEqual([]);
});

it('lee y cambia «Abrir al iniciar sesión»', async () => {
  serve(agent(), [net()], undefined, { autostart_get: () => false, autostart_set: () => undefined });
  const { container } = app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Abrir al iniciar sesión' });
  await waitFor(() => expect(toggle).not.toBeChecked());
  await userEvent.click(toggle);
  await waitFor(() => expect(toggle).toBeChecked());
  expect(calls('autostart_set')[0][1]).toEqual({ on: true });
  expect((await axe(container)).violations).toEqual([]);
});

it('mantiene el estado de inicio si guardarlo falla', async () => {
  serve(agent(), [net()], undefined, { notifications_get: () => true, autostart_get: () => true, autostart_set: () => { throw 'No se pudo escribir el fichero de arranque'; } });
  app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Abrir al iniciar sesión' });
  await waitFor(() => expect(toggle).toBeChecked());
  await userEvent.click(toggle);
  expect(await screen.findByRole('alert')).toHaveTextContent('No se pudo escribir el fichero de arranque');
  expect(toggle).toBeChecked();
});

it('no ofrece «Abrir al iniciar sesión» donde el sistema no lo soporta', async () => {
  serve(agent(), [net()], undefined, { notifications_get: () => true, autostart_get: () => null });
  app('/diagnostics');
  await screen.findByRole('switch', { name: 'Notificaciones del sistema' });
  await waitFor(() => expect(screen.queryByRole('switch', { name: 'Abrir al iniciar sesión' })).not.toBeInTheDocument());
});

it('dibuja tráfico con escala automática, leyenda e historial relativo accesibles', async () => {
  const { container, rerender } = render(<main><h2>Dispositivo de prueba</h2><TrafficHistory /></main>);
  expect(screen.getByText('Sin muestras de tráfico todavía.')).toBeInTheDocument();
  expect(screen.queryByRole('img')).not.toBeInTheDocument();
  expect(screen.getByText('Sin eventos desde que se abrió esta red.')).toBeInTheDocument();
  rerender(<main><h2>Dispositivo de prueba</h2><TrafficHistory history={{ path: 'relay', points: [{ at: Date.now() - 2000, down: 2048, up: 1024 }, { at: Date.now(), down: 4096, up: 0 }], events: [{ at: Date.now() - 5000, text: 'Pasó a relay' }] }} /></main>);
  expect(screen.getByRole('img', { name: /Velocidad de bajada/ })).toBeInTheDocument();
  expect(screen.getByText(/escala 0–4.0 KiB\/s/)).toBeInTheDocument();
  expect(screen.getByRole('list', { name: 'Historial de eventos' })).toHaveTextContent('Pasó a relay · hace 5 s');
  expect(screen.getByText(/El historial se pierde al cerrar la app/)).toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});

it('el ping refresca el camino y registra la transición en el historial', async () => {
  let path = 'none';
  serve(agent([peer('prueba-historial', '10.77.0.8', path)]), [net({ network_id: OTHER_ID })], undefined, {
    status: () => agent([peer('prueba-historial', '10.77.0.8', path)]),
    device_ping: () => { path = 'direct'; return JSON.stringify({ sent: 5, received: 5, loss_pct: 0, min_ms: null, avg_ms: null, max_ms: null }); },
  });
  app('/devices/prueba-historial');
  await userEvent.click(await screen.findByRole('button', { name: 'Probar conexión' }));
  expect(await screen.findByRole('list', { name: 'Historial de eventos' })).toHaveTextContent('Se conectó directamente');
  expect(await screen.findByRole('img', { name: /Velocidad de bajada/ })).toBeInTheDocument();
});

it('mantiene la preferencia anterior si guardar notificaciones falla', async () => {
  serve(agent(), [net()], undefined, { notifications_get: () => true, notifications_set: () => { throw 'No se pudo guardar la preferencia'; } });
  app('/diagnostics');
  const toggle = await screen.findByRole('switch', { name: 'Notificaciones del sistema' });
  await waitFor(() => expect(toggle).toBeChecked());
  await userEvent.click(toggle);
  expect(await screen.findByRole('alert')).toHaveTextContent('No se pudo guardar la preferencia');
  expect(toggle).toBeChecked();
});

it('la lista no lleva acciones: probar la conexión está en el detalle, prefiere IPv4 y tolera latencias ausentes', async () => {
  serve(agent([{ ...peer('aa', 'fd00::3', 'none'), addrs: ['fd00::3', '10.77.0.3'] }]), [net()], undefined, { device_ping: () => '{"sent":5,"received":0,"loss_pct":100}' });
  const { container } = app('/devices');
  await screen.findByRole('link', { name: /10\.77\.0\.3|fd00::3/ });
  expect(screen.queryByRole('button', { name: /Probar conexión/ })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('link', { name: /10\.77\.0\.3|fd00::3/ }));
  await userEvent.click(await screen.findByRole('button', { name: 'Probar conexión' }));
  expect(await screen.findByText('Pérdida: 100 %')).toBeInTheDocument();
  expect(screen.getByText('Mín/med/máx: — / — / — ms')).toBeInTheDocument();
  expect(calls('device_ping')[0][1]).toEqual({ network: NET_ID, address: '10.77.0.3' });
  expect((await axe(container)).violations).toEqual([]);
});

it('rechaza una respuesta ping mal formada sin inventar resultados', async () => {
  serve(agent([peer('aa', '10.77.0.3', 'none')]), [net()], undefined, { device_ping: () => '{"sent":"cinco"}' });
  app('/devices/aa');
  await userEvent.click(await screen.findByRole('button', { name: 'Probar conexión' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('La prueba respondió con un formato inesperado.');
  expect(screen.queryByText(/Pérdida:/)).not.toBeInTheDocument();
});

it('muestra el error de lectura de notificaciones y el de abrir el servidor', async () => {
  serve(agent(), [net()], undefined, { notifications_get: () => { throw 'Preferencia no legible'; }, open_panel: () => { throw 'Navegador no disponible'; } });
  const { unmount } = app('/diagnostics');
  expect(await screen.findByRole('alert')).toHaveTextContent('Preferencia no legible');
  expect(screen.getByRole('switch', { name: 'Notificaciones del sistema' })).toBeDisabled();
  unmount();
  const { container } = app('/servers/control.example.test');
  await userEvent.click(await screen.findByRole('button', { name: 'Abrir en el panel' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Navegador no disponible');
  expect((await axe(container)).violations).toEqual([]);
});
