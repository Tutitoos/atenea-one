// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-021: el asistente solo guía. Con privilegios ocurre únicamente lo que el usuario confirma, el cliente ausente se
// resuelve con texto copiable y los sistemas no soportados no ofrecen unirse.
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import Home from './routes/home';
import Welcome from './routes/welcome';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const info = { name: 'casa', server: 'control.example.test', network_id: NET_ID, root_fingerprint: '6c4b-fd0b-2585-9aae', has_token: false };
const pending = { network_id: NET_ID, name: 'casa', server: 'control.example.test', url: 'https://control.example.test', interface: 'atenea0', listen_port: 51821, join_status: 'pendiente', state: 'iniciada_sin_configuracion' };
const linux = { os: 'linux', supported: true, installed: true };

type World = {
  client: object | (() => object);
  networks: object[];
  legacy: boolean;
  autostart: boolean | null;
  apply: () => Promise<string> | string;
};

function serve(over: Partial<World> = {}) {
  const w: World = { client: linux, networks: [], legacy: false, autostart: false, apply: () => `huella_dispositivo=ab12-cd34\n`, ...over };
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'client_status': return (typeof w.client === 'function' ? w.client() : w.client) as never;
      case 'network_list': return JSON.stringify(w.networks) as never;
      case 'network_legacy': return JSON.stringify({ present: w.legacy }) as never;
      case 'network_inspect': return JSON.stringify(info) as never;
      case 'network_apply': return (await w.apply()) as never;
      case 'autostart_get': return w.autostart as never;
      case 'autostart_set': return undefined as never;
      case 'open_panel': return undefined as never;
      default: throw new Error(`orden inesperada: ${cmd}`);
    }
  });
  return w;
}
const calls = (cmd: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === cmd);

function show(entry = '/welcome') {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route index element={<Home />} />
          <Route path="welcome" element={<Welcome />} />
          <Route path="devices" element={<p>pantalla de dispositivos</p>} />
          <Route path="devices/self" element={<p>pantalla de dispositivos</p>} />
          <Route path="networks" element={<p>pantalla de redes</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

async function review() {
  await userEvent.type(await screen.findByRole('textbox', { name: 'Código de red' }), 'ATENEA1-ABCDEF-GH23');
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  return within(await screen.findByRole('group', { name: 'Confirmar unión' }));
}

beforeEach(() => { vi.mocked(invoke).mockReset(); });

it('un sistema no soportado no ofrece unirse ni consulta redes', async () => {
  serve({ client: { os: 'windows', supported: false, installed: false } });
  show();
  expect(await screen.findByRole('region', { name: 'Este sistema todavía no está soportado' })).toHaveTextContent('Windows');
  expect(screen.queryByRole('textbox', { name: 'Código de red' })).not.toBeInTheDocument();
  expect(calls('network_list')).toHaveLength(0);
  expect(calls('network_apply')).toHaveLength(0);
});

it.each([
  ['linux', 'sudo apt install ./atenea-cli-*-linux-*.deb ./atenea-desktop-*-linux-*.deb'],
  ['darwin', 'sudo installer -pkg atenea-one-*-macos-*.pkg -target /'],
])('sin cliente (%s) enseña la orden para copiar y no ofrece unirse', async (os, command) => {
  serve({ client: { os, supported: true, installed: false } });
  show();
  const step = within(await screen.findByRole('region', { name: 'Instala el cliente de Atenea One' }));
  expect(step.getByText(command)).toBeInTheDocument();
  expect(step.getByRole('button', { name: 'Copiar orden de instalación' })).toBeInTheDocument();
  expect(screen.queryByRole('link')).not.toBeInTheDocument(); // sin enlaces: la aplicación no abre URL
  expect(screen.queryByRole('textbox', { name: 'Código de red' })).not.toBeInTheDocument();
  expect(calls('network_list')).toHaveLength(0);
  expect(calls('network_apply')).toHaveLength(0);
});

it('«Comprobar de nuevo» vuelve a mirar y, cuando el cliente aparece, pasa a unirse', async () => {
  let installed = false;
  serve({ client: () => ({ ...linux, installed }) });
  show();
  await userEvent.click(await screen.findByRole('button', { name: 'Comprobar de nuevo' }));
  expect(await screen.findByText('Todavía no se detecta el cliente.')).toBeInTheDocument();
  expect(calls('client_status')).toHaveLength(2);
  installed = true;
  await userEvent.click(screen.getByRole('button', { name: 'Comprobar de nuevo' }));
  expect(await screen.findByRole('textbox', { name: 'Código de red' })).toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(0);
});

it('unirse: nada con privilegios hasta «Unirme a esta red», una sola vez, y luego espera la aprobación con la huella', async () => {
  const w = serve();
  show();
  const confirm = await review();
  expect(calls('network_apply')).toHaveLength(0); // revisar el código no cambia nada
  w.networks = [pending];
  await userEvent.click(confirm.getByRole('button', { name: 'Unirme a esta red' }));
  const step = within(await screen.findByRole('region', { name: 'Esperando la aprobación de «casa»' }));
  expect(calls('network_apply')).toEqual([['network_apply', { op: 'join', code: 'ATENEA1-ABCDEF-GH23' }]]);
  expect(step.getByText('ab12-cd34')).toBeInTheDocument();
  expect(step.getByRole('button', { name: 'Copiar huella del dispositivo' })).toBeInTheDocument();
  expect(await step.findByText(/Estado: Sin configuración \(pendiente de aprobación\)/)).toBeInTheDocument();
  await userEvent.click(step.getByRole('button', { name: 'Abrir en el panel' }));
  expect(calls('open_panel')).toEqual([['open_panel', { network: NET_ID, deviceId: null }]]);
  await userEvent.click(step.getByRole('button', { name: 'Continuar sin esperar' }));
  expect(await screen.findByRole('region', { name: 'Listo' })).toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(1); // ningún paso del asistente vuelve a elevar por su cuenta
});

it('cancelar el diálogo del sistema deja la confirmación y permite reintentar', async () => {
  let attempt = 0;
  serve({ apply: () => { attempt += 1; if (attempt === 1) throw 'Cancelado: no se ha cambiado nada'; return 'huella_dispositivo=ab12-cd34\n'; } });
  show();
  const confirm = await review();
  await userEvent.click(confirm.getByRole('button', { name: 'Unirme a esta red' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Cancelado: no se ha cambiado nada');
  expect(screen.getByRole('group', { name: 'Confirmar unión' })).toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(1);
  await userEvent.click(screen.getByRole('button', { name: 'Unirme a esta red' }));
  await waitFor(() => expect(calls('network_apply')).toHaveLength(2));
});

it('con una instalación heredada ofrece importarla y permite unirse a otra sin importar', async () => {
  serve({ legacy: true });
  show();
  const step = within(await screen.findByRole('region', { name: 'Hay una instalación anterior' }));
  expect(step.getByRole('button', { name: 'Importar red existente' })).toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(0);
  await userEvent.click(step.getByRole('button', { name: 'Unirme a otra red en su lugar' }));
  expect(await screen.findByRole('textbox', { name: 'Código de red' })).toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(0);
});

it('con redes ya unidas no ofrece unirse y enseña el interruptor de inicio apagado', async () => {
  serve({ networks: [{ ...pending, state: 'conectada' }] });
  show();
  const step = within(await screen.findByRole('region', { name: 'Ya tienes redes unidas' }));
  expect(screen.queryByRole('textbox', { name: 'Código de red' })).not.toBeInTheDocument();
  const toggle = await step.findByRole('switch', { name: 'Abrir al iniciar sesión' });
  expect(toggle).not.toBeChecked();
  await userEvent.click(toggle);
  await waitFor(() => expect(calls('autostart_set')).toEqual([['autostart_set', { on: true }]]));
  expect(step.getByRole('link', { name: 'Ver dispositivos' })).toHaveAttribute('href', '/devices');
});

it('si el sistema no admite el inicio con la sesión no hay interruptor', async () => {
  serve({ networks: [{ ...pending, state: 'conectada' }], autostart: null });
  show();
  const step = within(await screen.findByRole('region', { name: 'Ya tienes redes unidas' }));
  await waitFor(() => expect(calls('autostart_get')).toHaveLength(1));
  expect(step.queryByRole('switch')).not.toBeInTheDocument();
});

it('un fallo al comprobar el estado se muestra y se puede reintentar', async () => {
  vi.mocked(invoke).mockRejectedValue('no se pudo');
  show();
  expect(await screen.findByRole('alert')).toHaveTextContent('no se pudo');
  expect(screen.getByRole('button', { name: 'Reintentar' })).toBeInTheDocument();
  expect(calls('network_apply')).toHaveLength(0);
});

it('el inicio lleva al asistente cuando no hay redes y a Dispositivos cuando las hay', async () => {
  serve({ networks: [] });
  const first = show('/');
  expect(await screen.findByRole('heading', { name: 'Primeros pasos' })).toBeInTheDocument();
  first.unmount();
  serve({ networks: [{ ...pending, state: 'conectada' }] });
  show('/');
  expect(await screen.findByText('pantalla de dispositivos')).toBeInTheDocument();
});

it.each([
  ['cliente ausente', { client: { os: 'linux', supported: true, installed: false } }, 'Instala el cliente de Atenea One'],
  ['unirse', {}, 'Únete a una red'],
  ['listo', { networks: [{ ...pending, state: 'conectada' }] }, 'Ya tienes redes unidas'],
])('sin infracciones de accesibilidad: %s', async (_, world, region) => {
  serve(world as Partial<World>);
  const { container } = show();
  await screen.findByRole('region', { name: region });
  // jsdom no calcula layout ni contraste: el contraste necesita comprobación en un webview real.
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
