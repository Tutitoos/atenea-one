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

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const peer = (key: string, addr: string, path: string, name?: string) => ({
  public_key: key, addrs: [addr], name, endpoint: '192.0.2.7:51820', session: 'activa 5s', last_handshake_secs_ago: 5,
  tx_bytes: 2048, rx_bytes: 4096, path, candidates: 2, paths: [{ kind: path, ready: true, healthy: true, rtt_ms: null }],
});
const degraded = { level: 'degraded', text: 'Degradado', reasons: ['algún dispositivo por relay o sin camino'] };
const server = { sync: { url: 'https://control.example.test', last_ok_unix: Math.floor(Date.now() / 1000) - 5, last_error: null, sync_ok: 40, sync_denied: 1, sync_errors: 2 }, coordinator: { url: 'https://control.example.test', last_announce_ok_unix: Math.floor(Date.now() / 1000) - 3, last_error: null, announces: 120, events: 9 } };
const relay = { allocations: 1, drops: {}, entries: [{ peer: 'aa', transport: 'Udp', relay: 'relay-uno', state: 'ready', rtt_ms: 42, tx_bytes: 2048, rx_bytes: 1024 }] };
const agent = (peers = [peer('a'.repeat(64), '10.77.0.3', 'direct', 'servidor-vps'), peer('b'.repeat(64), '10.77.0.4', 'relay-udp')], health: object = degraded, extra: object = { ...server, relay }) => JSON.stringify({
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
            <Route path="diagnostics" element={<Diagnostics />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const OTHER_ID = '0199ab12-3456-7def-8abc-0123456789cd';
const net = (over: object = {}) => ({ network_id: NET_ID, name: 'casa', server: 'control.example.test', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada', ...over });
const info = { name: 'casa', server: 'control.example.test', network_id: NET_ID, root_fingerprint: '6c4b-fd0b-2585-9aae', has_token: false };
const CODE = 'ATENEA1-ABCDEF-GH23';

/** Responde como Rust: `status` con el estado (o el fallo), `network_list` con las redes y el resto por orden. */
function serve(statusReply: string, networks: object[] = [net()], fail?: string, extra: Record<string, (args: unknown) => unknown> = {}) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === 'network_list') return JSON.stringify(networks);
    if (cmd === 'network_legacy' && !(cmd in extra)) return '{"present":false}' as never;
    if (cmd in extra) return extra[cmd](args) as never;
    if (fail) throw fail;
    return statusReply as never;
  });
}
const calls = (cmd: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });

it('la banda de avisos solo aparece si hay motivos que contar', async () => {
  // Caso real de Rust: un par por relay degrada el estado pero no aporta ningún motivo textual.
  serve(agent(undefined, { level: 'degraded', text: 'Degradado: algún dispositivo por relay o sin camino', reasons: [] }));
  const { unmount } = app();
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Degradado'));
  expect(screen.queryByRole('list', { name: 'Avisos' })).not.toBeInTheDocument();
  unmount();
  serve(agent(undefined, { level: 'down', text: 'Sin conexión: sin lease activo', reasons: ['sin lease activo'] }));
  app();
  expect(await screen.findByRole('list', { name: 'Avisos' })).toHaveTextContent('sin lease activo');
});


it('el servidor sale en su propio grupo, aparte de los demás dispositivos, y su detalle muestra servicios y relays', async () => {
  serve(agent());
  app();
  const groups = await screen.findAllByRole('heading', { level: 2, name: /^(Local|Servidor|Dispositivos)$/ });
  expect(groups.map((g) => g.textContent)).toEqual(['Local', 'Servidor', 'Dispositivos']);
  await userEvent.click(await screen.findByRole('link', { name: /control\.example\.test/ }));
  const detail = within(screen.getByRole('main'));
  expect(await detail.findByRole('heading', { level: 2, name: 'control.example.test' })).toBeInTheDocument();
  expect(detail.getByText('Conectado')).toBeInTheDocument();
  expect(detail.getByText('40 correctas · 1 denegadas · 2 con error')).toBeInTheDocument();
  expect(detail.getByText('relay-uno · UDP')).toBeInTheDocument();
  expect(detail.getByText(/ready · 42 ms/)).toBeInTheDocument();
});

it('un error del control plane pone el servidor en rojo y sin control plane configurado no hay grupo Servidor', async () => {
  serve(agent(undefined, degraded, { sync: { ...server.sync, last_error: 'acceso denegado' }, coordinator: server.coordinator }));
  const { unmount } = app('/devices/server');
  expect(await screen.findByText('Control plane: acceso denegado')).toBeInTheDocument();
  unmount();
  serve(agent(undefined, degraded, {}));
  app();
  await screen.findByRole('link', { name: /10\.77\.0\.4/ });
  expect(screen.queryByRole('heading', { level: 2, name: 'Servidor' })).not.toBeInTheDocument();
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

it('un dispositivo marcado como servidor lleva la etiqueta, se puede buscar por ella y se desmarca', async () => {
  serve(agent());
  app(`/devices/${'a'.repeat(64)}`);
  const mark = await screen.findByRole('checkbox', { name: /Marcar como servidor/ });
  expect(mark).not.toBeChecked();
  expect(screen.queryByText('Servidor', { selector: 'span span' })).not.toBeInTheDocument();
  await userEvent.click(mark);
  expect(mark).toBeChecked();
  const list = screen.getByRole('link', { name: /servidor-vps/ });
  expect(within(list).getByText('Servidor')).toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem('atenea-servers') ?? '[]')).toEqual(['a'.repeat(64)]);
  await userEvent.type(screen.getByRole('searchbox', { name: 'Buscar dispositivos' }), 'servidor');
  expect(screen.getByRole('link', { name: /servidor-vps/ })).toBeInTheDocument();
  await userEvent.click(mark);
  expect(within(screen.getByRole('link', { name: /servidor-vps/ })).queryByText('Servidor')).not.toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem('atenea-servers') ?? '[]')).toEqual([]);
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

it('las pantallas no tienen violaciones de accesibilidad detectables', async () => {
  serve(agent());
  for (const entry of ['/devices', `/devices/${'a'.repeat(64)}`, '/networks', '/diagnostics']) {
    const { container, unmount } = app(entry);
    await screen.findByRole('navigation', { name: 'Secciones' });
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Degradado'));
    // jsdom no calcula layout ni contraste: el contraste necesita comprobación en un webview real.
    expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
    unmount();
  }
});
