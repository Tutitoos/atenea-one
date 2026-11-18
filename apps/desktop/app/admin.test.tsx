// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-026, fase 6: «Administración» del propio equipo.
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
import Shell from './routes/shell';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const ME = '0199ab12-3456-7def-8abc-0123456789cd';
const NEW = '0199ab12-3456-7def-8abc-0123456789ce';
const OTHER = '0199ab12-3456-7def-8abc-0123456789cf';
const FP = '1234-5678-9abc-def0-1234-5678-9abc-def0';
const net = { network_id: NET_ID, name: 'casa', server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada' };
const linux = { os: 'linux', supported: true, can_join: true, installed: true };
const windows = { os: 'windows', supported: true, can_join: true, installed: true };
const dev = (id: string, name: string, status: string, in_scope: boolean, extra = {}) => ({ id, name, role: 'device', status, fingerprint: id === NEW ? FP : `0000-0000-0000-0000-0000-0000-0000-000${id.slice(-1)}`, ipv4: '10.9.0.2', ipv6: null, enrolled_by_device: null, in_scope, ...extra });
const all = ['read:devices', 'read:policies', 'write:join-codes', 'write:approve-devices', 'write:policies', 'write:revoke-devices'];
const grant = (scopes: string[], kind = 'enrolled') => ({ scopes, scope: { kind, devices: [] }, granted_at: '2026-11-17T10:00:00Z' });
const admin = (g: unknown, extra: Record<string, unknown> = {}) => ({
  grant: g,
  devices: [dev(ME, 'este equipo', 'approved', true), dev(NEW, 'nuevo', 'pending', true, { enrolled_by_device: ME }), dev(OTHER, 'ajeno', 'approved', false)],
  policies: [
    { id: '0199ab12-3456-7def-8abc-0123456789d1', src: { kind: 'any' }, dst: { kind: 'device', device_id: ME }, proto: 0, port_min: 0, port_max: 0, created_by_device: null },
    { id: '0199ab12-3456-7def-8abc-0123456789d2', src: { kind: 'device', device_id: ME }, dst: { kind: 'device', device_id: NEW }, proto: 6, port_min: 22, port_max: 22, created_by_device: ME },
  ],
  fetched_unix: 1, last_error: null, ...extra,
});

function serve(adminState: unknown, client: object = linux, reply: (op: string, params: unknown) => unknown = () => 'ok', peers: unknown[] = []) {
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    switch (cmd) {
      case 'network_list': return JSON.stringify([net]) as never;
      case 'network_legacy': return '{"present":false}' as never;
      case 'autostart_get': return null as never;
      case 'notifications_get': return true as never;
      case 'client_status': return client as never;
      case 'admin_apply': { const a = args as { op: string; params: unknown }; return reply(a.op, a.params) as never; }
      case 'status': return JSON.stringify({
        status: { public_key: 'c'.repeat(64), addrs: ['10.9.0.2'], hostname: 'equipo', config: { version: 7, lease_active: true, valid_until_unix: Date.now() / 1000 + 240 }, counters: {}, peers, admin: adminState },
        health: { level: 'ok', text: 'Conectado', reasons: [] },
      }) as never;
      default: throw new Error(`orden inesperada: ${cmd}`);
    }
  });
}
const writes = () => vi.mocked(invoke).mock.calls.filter(([c]) => c === 'admin_apply').map(([, a]) => a);

function open(path = '/devices/self') {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter initialEntries={[path]}>
        <Routes><Route element={<Shell />}><Route path="devices" element={<Devices />}><Route path=":id" element={<Device />} /></Route></Route></Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => { vi.mocked(invoke).mockReset(); resetHistoryStore(); });

it('sin concesión, o con un agente anterior, no aparece «Administración»', async () => {
  serve(admin(null, { devices: null, policies: null }));
  const first = open();
  await screen.findByText('Detalles');
  expect(screen.queryByRole('heading', { name: 'Administración' })).not.toBeInTheDocument();
  first.unmount();
  serve(undefined);
  open();
  await screen.findByText('Detalles');
  expect(screen.queryByRole('heading', { name: 'Administración' })).not.toBeInTheDocument();
});

it('con concesión muestra permisos, alcance, pendientes y políticas, y axe sale limpio', async () => {
  serve(admin(grant(all)));
  const { container } = open();
  await userEvent.click(await screen.findByRole('tab', { name: 'Permisos' }));
  await screen.findByRole('heading', { name: 'Administración' });
  expect(screen.getByText(/los dispositivos que este equipo da de alta, y él mismo/)).toBeVisible();
  expect(screen.getByRole('list', { name: 'Permisos concedidos' })).toHaveTextContent('write:approve-devices');
  await userEvent.click(screen.getByRole('tab', { name: 'Dispositivos' }));
  expect(screen.getByRole('region', { name: 'Pendientes' })).toHaveTextContent('nuevo');
  expect(screen.getByText(/ajeno/).textContent).toContain('fuera de tu alcance');
  await userEvent.click(screen.getByRole('tab', { name: 'Políticas' }));
  expect(screen.getByText(/de un administrador/)).toBeVisible();
  expect((await axe(container, { rules: { 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('aprobar muestra la huella completa y envía solo esa huella tras confirmar', async () => {
  serve(admin(grant(all)));
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole('tab', { name: 'Dispositivos' }));
  await user.click(await screen.findByRole('button', { name: 'Aprobar…' }));
  expect(screen.getByLabelText('Huella de nuevo')).toHaveTextContent(FP);
  expect(writes()).toEqual([]);
  await user.click(screen.getByRole('button', { name: 'Confirmar aprobación' }));
  await waitFor(() => expect(writes()).toEqual([{ op: 'approve', network: NET_ID, params: { fingerprint: FP } }]));
});

it('genera un código una vez y solo muestra el código y su caducidad', async () => {
  serve(admin(grant(all)), linux, () => JSON.stringify({ code: 'ATENEA1-SINTETICO', expires_at: '2026-11-17T10:10:00Z' }));
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole('tab', { name: 'Código de unión' }));
  await user.click(await screen.findByRole('button', { name: 'Generar código' }));
  expect(await screen.findByLabelText('Código de unión generado')).toHaveTextContent('ATENEA1-SINTETICO');
  expect(writes()).toEqual([{ op: 'join-code', network: NET_ID, params: { ttl: 600 } }]);
});

it('revocar pide confirmar y solo ofrece dispositivos del alcance', async () => {
  serve(admin(grant(all)));
  const user = userEvent.setup();
  open();
  await screen.findByRole('tab', { name: 'Permisos' });
  await user.click(screen.getByRole('tab', { name: 'Dispositivos' }));
  const revoke = await screen.findByRole('button', { name: 'Revocar nuevo' });
  expect(screen.queryByRole('button', { name: 'Revocar ajeno' })).not.toBeInTheDocument();
  await user.click(revoke);
  expect(writes()).toEqual([]);
  await user.click(screen.getByRole('button', { name: 'Confirmar revocación de nuevo' }));
  await waitFor(() => expect(writes()).toEqual([{ op: 'revoke', network: NET_ID, params: { device: NEW } }]));
});

it('crea políticas solo entre dispositivos del alcance y borra solo las delegadas de su alcance', async () => {
  serve(admin(grant(all)));
  const user = userEvent.setup();
  open();
  await screen.findByRole('tab', { name: 'Permisos' });
  await user.click(screen.getByRole('tab', { name: 'Políticas' }));
  const origin = await screen.findByLabelText('Origen');
  expect([...origin.querySelectorAll('option')].map((o) => o.textContent)).toEqual(['—', 'este equipo']);
  await user.selectOptions(origin, ME);
  await user.selectOptions(screen.getByLabelText('Destino'), ME);
  await user.type(screen.getByLabelText('Puertos'), '22');
  await user.click(screen.getByRole('button', { name: 'Crear política' }));
  await waitFor(() => expect(writes()).toEqual([{ op: 'policy-add', network: NET_ID, params: { from: ME, to: ME, proto: 'tcp', ports: '22' } }]));
  // La política de un administrador no tiene botón de borrar; la delegada con ambos extremos en el alcance, sí.
  expect(screen.getAllByRole('button', { name: /Borrar política/ }).map((b) => b.textContent)).toEqual(['Borrar política este equipo → nuevo']);
});

it('con permisos de lectura no ofrece escrituras y en Windows indica la CLI', async () => {
  serve(admin(grant(['read:devices', 'read:policies'])));
  const first = open();
  await screen.findByRole('tab', { name: 'Permisos' });
  expect(screen.queryByRole('button', { name: 'Generar código' })).not.toBeInTheDocument();
  expect(screen.queryByRole('tab', { name: 'Código de unión' })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('tab', { name: 'Dispositivos' }));
  for (const name of ['Revocar nuevo', 'Aprobar…']) expect(screen.queryByRole('button', { name })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('tab', { name: 'Políticas' }));
  expect(screen.queryByRole('button', { name: 'Crear política' })).not.toBeInTheDocument();
  first.unmount();
  vi.mocked(invoke).mockReset();
  serve(admin(grant(all)), windows);
  open();
  await userEvent.click(await screen.findByRole('tab', { name: 'Permisos' }));
  expect(await screen.findByText(/usa/)).toHaveTextContent('atenea network');
  await userEvent.click(screen.getByRole('tab', { name: 'Código de unión' }));
  expect(screen.getByRole('button', { name: 'Generar código' })).toBeDisabled();
});

it('cancelar el diálogo del sistema muestra el error y no deja nada a medias', async () => {
  serve(admin(grant(all)), linux, () => { throw 'Cancelado'; });
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole('tab', { name: 'Dispositivos' }));
  await user.click(await screen.findByRole('button', { name: 'Revocar nuevo' }));
  await user.click(screen.getByRole('button', { name: 'Confirmar revocación de nuevo' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(/Cancelado|falló/);
});

it('las pestañas se recorren con las flechas y solo se monta la activa', async () => {
  serve(admin(grant(all)));
  const user = userEvent.setup();
  open();
  const general = await screen.findByRole('tab', { name: 'General' });
  expect(general).toHaveAttribute('aria-selected', 'true');
  general.focus();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('tab', { name: 'Permisos' })).toHaveAttribute('aria-selected', 'true');
  expect(screen.getByRole('heading', { name: 'Administración' })).toBeVisible();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('tab', { name: 'Código de unión' })).toHaveAttribute('aria-selected', 'true');
  expect(screen.queryByRole('heading', { name: 'Administración' })).not.toBeInTheDocument();
  await user.keyboard('{End}');
  expect(screen.getByRole('tab', { name: 'Políticas' })).toHaveFocus();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('tab', { name: 'General' })).toHaveFocus();
});

// ADR-027: pestaña «Permisos» de otro dispositivo.
const PEER = '0199ab12-3456-7def-8abc-0123456789d9';
const peerOf = { public_key: 'p'.repeat(64), device_id: PEER, addrs: ['10.9.0.9'], name: 'compañero', tx_bytes: 0, rx_bytes: 0, path: 'direct', candidates: 0, paths: [] };
const giver = (extra: Record<string, unknown> = {}) => ({ ...grant(['read:devices', 'write:grants']), device_id: ME, ...extra });
const state = (g: unknown, grants: unknown[] = []) => admin(g, { devices: [dev(ME, 'este equipo', 'approved', true), dev(PEER, 'compañero', 'approved', true)], grants });
const given = (by: string | null) => ({ device_id: PEER, scopes: ['read:devices'], scope: { kind: 'devices', devices: [ME] }, granted_by_device: by, granted_at: '2026-11-18T10:00:00Z' });

async function openPeer() {
  const user = userEvent.setup();
  open('/devices/' + 'p'.repeat(64));
  await user.click(await screen.findByRole('tab', { name: 'Permisos' }));
  return user;
}

it('sin write:grants, los permisos de otro dispositivo no se ven ni se cambian', async () => {
  serve(state(giver({ scopes: ['read:devices'] })), linux, undefined, [peerOf]);
  await openPeer();
  expect(await screen.findByText(/Solo un administrador de la red, o un equipo con permiso/)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Guardar permisos' })).not.toBeInTheDocument();
});

it('con write:grants concede solo lo que tiene, dentro de su alcance, y nunca write:grants', async () => {
  serve(state(giver()), linux, undefined, [peerOf]);
  const user = await openPeer();
  expect(await screen.findByText(/no tiene permisos de administración en esta red/)).toBeVisible();
  expect(screen.queryByRole('checkbox', { name: /Administrar los permisos/ })).not.toBeInTheDocument();
  const save = screen.getByRole('button', { name: 'Guardar permisos' });
  expect(save).toBeDisabled();
  await user.click(screen.getByRole('checkbox', { name: 'Ver el inventario de la red' }));
  expect(save).toBeDisabled();
  expect(screen.queryByRole('option', { name: /Toda la red/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole('checkbox', { name: 'este equipo' }));
  await user.click(save);
  await waitFor(() => expect(writes()).toEqual([{ op: 'grant', network: NET_ID, params: { device: PEER, permissions: ['read:devices'], scope: 'devices', devices: [ME] } }]));
});

it('solo cambia o retira lo que dio este equipo', async () => {
  serve(state(giver(), [given(ME)]), linux, undefined, [peerOf]);
  const user = await openPeer();
  expect(await screen.findByText(/Concedidos por este equipo/)).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Retirar permisos' }));
  expect(writes()).toEqual([]);
  await user.click(screen.getByRole('button', { name: 'Confirmar retirada de permisos' }));
  await waitFor(() => expect(writes()).toEqual([{ op: 'ungrant', network: NET_ID, params: { device: PEER } }]));
});

it('una concesión de un administrador se ve pero no se toca', async () => {
  serve(state(giver(), [given(null)]), linux, undefined, [peerOf]);
  await openPeer();
  expect(await screen.findByText(/Concedidos por un administrador/)).toBeVisible();
  expect(screen.getByText(/Solo puedes cambiar o retirar los permisos que diste tú/)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Guardar permisos' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Retirar permisos' })).not.toBeInTheDocument();
});

it('en Windows no ofrece escrituras y remite a la CLI', async () => {
  serve(state(giver()), windows, undefined, [peerOf]);
  await openPeer();
  expect(await screen.findByText(/atenea network grant/)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Guardar permisos' })).not.toBeInTheDocument();
});

it('el propio equipo lista lo que ha concedido y, sin concesión, lo dice', async () => {
  serve(state(giver(), [given(ME), { ...given(null), device_id: ME }]), linux, undefined, [peerOf]);
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole('tab', { name: 'Permisos' }));
  const list = within(await screen.findByRole('region', { name: 'Permisos concedidos a otros' }));
  expect(list.getByText('compañero')).toBeVisible();
  expect(list.queryByText('este equipo')).not.toBeInTheDocument();
});

it('sin concesión, la pestaña Permisos del propio equipo explica quién los concede', async () => {
  serve(admin(null, { devices: null, policies: null }));
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole('tab', { name: 'Permisos' }));
  expect(await screen.findByText(/no tiene permisos de administración en esta red/)).toBeVisible();
});
