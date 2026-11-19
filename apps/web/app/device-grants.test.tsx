// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-026, fase 5: permisos de administración delegada en el detalle del dispositivo.
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { axe } from 'vitest-axe';
import { afterEach, expect, it, vi } from 'vitest';
import { api, type Model } from '@atenea/api-client';
import { device } from './device.fixture';
import { Devices } from './devices';
import { Policies } from './policies';

const lead = { ...device, status: 'approved' as const };
const other = { ...device, id: '22222222-2222-4222-8222-222222222222', device_id: '22222222-2222-4222-8222-222222222222', name: 'Otro equipo', status: 'approved' as const };
const grant: Model<'DeviceGrant'> = {
  device_id: lead.id, scopes: ['read:devices', 'write:policies'], scope: { kind: 'enrolled', devices: [] },
  granted_by: '99999999-9999-4999-8999-999999999999', granted_by_device: null, granted_at: '2026-11-17T10:00:00Z',
};
afterEach(() => vi.restoreAllMocks());

async function open(props: { grants?: Model<'DeviceGrant'>[]; role?: 'admin' | 'member'; devices?: Model<'Device'>[]; refresh?: () => Promise<unknown> }) {
  const view = render(<MemoryRouter><Devices devices={props.devices ?? [lead, other]} accessRole={props.role ?? 'admin'} net="red" elapsed={0}
    refresh={props.refresh ?? (async () => {})} grants={props.grants}/></MemoryRouter>);
  await userEvent.setup().click(screen.getAllByRole('button', { name: `Ver detalle de ${lead.name}` })[0]!);
  return view;
}

it('muestra permisos, alcance y quién los concedió; axe sale limpio', async () => {
  const { container } = await open({ grants: [grant] });
  const list = screen.getByRole('list', { name: `Permisos de ${lead.name}` });
  expect(list).toHaveTextContent('Ver el inventario de la red');
  expect(list).toHaveTextContent('Crear y borrar políticas entre dispositivos');
  expect(screen.getByText(/Alcance: Los dispositivos que él da de alta, y él mismo/)).toBeVisible();
  expect(screen.getByRole('button', { name: 'Retirar permisos' })).toBeVisible();
  expect((await axe(container, { rules: { 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('sin concesión lo dice y ofrece conceder; un miembro solo lee y un servidor sin la ruta no muestra la sección', async () => {
  const first = await open({ grants: [] });
  expect(screen.getByText(/solo actúa sobre sí mismo/)).toBeVisible();
  expect(screen.getByRole('button', { name: 'Conceder permisos' })).toBeVisible();
  first.unmount();
  const member = await open({ grants: [grant], role: 'member' });
  expect(screen.getByText(/Alcance:/)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Cambiar permisos' })).not.toBeInTheDocument();
  member.unmount();
  await open({ grants: undefined });
  expect(screen.queryByText('Administración delegada')).not.toBeInTheDocument();
});

it('un dispositivo pendiente no recibe permisos', async () => {
  await open({ grants: [], devices: [{ ...lead, status: 'pending' }, other] });
  expect(screen.queryByText('Administración delegada')).not.toBeInTheDocument();
});

it('concede con la lista de dispositivos del alcance solo si hay al menos uno', async () => {
  const put = vi.spyOn(api, 'PUT').mockResolvedValue({ data: grant, response: new Response() });
  const refresh = vi.fn(async () => {});
  const user = userEvent.setup();
  await open({ grants: [], refresh });
  await user.click(screen.getByRole('button', { name: 'Conceder permisos' }));
  await user.click(screen.getByLabelText(/Una lista de dispositivos/));
  await user.click(screen.getByRole('button', { name: 'Guardar permisos' }));
  expect(screen.getByText('Elige al menos un dispositivo.')).toBeVisible();
  expect(put).not.toHaveBeenCalled();
  await user.click(screen.getByLabelText('Otro equipo'));
  await user.click(screen.getByLabelText(/Crear y borrar políticas/));
  await user.click(screen.getByRole('button', { name: 'Guardar permisos' }));
  await waitFor(() => expect(put).toHaveBeenCalledWith('/api/v1/networks/{net}/devices/{dev}/grant', {
    params: { path: { net: 'red', dev: lead.id } },
    body: { scopes: ['read:devices', 'write:policies'], scope: { kind: 'devices', devices: [other.id] } },
  }));
  await waitFor(() => expect(refresh).toHaveBeenCalled());
});

it('avisa del alcance de toda la red y no deja guardar sin permisos', async () => {
  const put = vi.spyOn(api, 'PUT').mockResolvedValue({ data: grant, response: new Response() });
  const user = userEvent.setup();
  await open({ grants: [] });
  await user.click(screen.getByRole('button', { name: 'Conceder permisos' }));
  await user.click(screen.getByLabelText(/Toda la red/));
  expect(screen.getByRole('note')).toHaveTextContent('también los de producción');
  await user.click(screen.getByLabelText(/Ver el inventario de la red/));
  await user.click(screen.getByRole('button', { name: 'Guardar permisos' }));
  expect(screen.getByText('Elige al menos un permiso.')).toBeVisible();
  expect(put).not.toHaveBeenCalled();
});

it('retirar los permisos exige confirmar', async () => {
  const del = vi.spyOn(api, 'DELETE').mockResolvedValue({ response: new Response() });
  const user = userEvent.setup();
  await open({ grants: [grant] });
  await user.click(screen.getByRole('button', { name: 'Retirar permisos' }));
  expect(del).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar retirada' }));
  await waitFor(() => expect(del).toHaveBeenCalledWith('/api/v1/networks/{net}/devices/{dev}/grant', { params: { path: { net: 'red', dev: lead.id } } }));
});

it('dice quién dio de alta al dispositivo', async () => {
  await open({ devices: [{ ...lead, enrolled_by_device: other.id }, other], grants: [] });
  expect(screen.getByText('Dado de alta por').nextElementSibling).toHaveTextContent('Otro equipo (delegado)');
});

it('las políticas dicen si las creó un administrador o un dispositivo delegado', async () => {
  const mk = (id: string, by: string | null) => ({ id, src: { kind: 'device' as const, device_id: lead.id }, dst: { kind: 'device' as const, device_id: other.id }, proto: 6, port_min: 22, port_max: 22, created_by_device: by });
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [mk('a', null), mk('b', lead.id)], response: new Response() });
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><Policies net="red" canEdit={false} devices={[lead, other]}/></QueryClientProvider>);
  const items = await screen.findAllByText(/Creada por:/);
  expect(items[0]).toHaveTextContent('Creada por: administrador');
  expect(items[1]).toHaveTextContent(`Creada por: ${lead.name} (delegado)`);
});
