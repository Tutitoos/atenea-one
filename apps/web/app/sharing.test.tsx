// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-028, fase 3: invitaciones, miembros por red y aprobación como enroller.
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useSearchParams } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api, ApiError, type Model } from '@atenea/api-client';
import { device } from './device.fixture';
import { Devices } from './devices';
import { AcceptInvitation, Sharing, SharedNetworks, useInvitationToken } from './sharing';

beforeEach(() => sessionStorage.clear());
afterEach(() => vi.restoreAllMocks());

const wrap = (node: React.ReactNode, entry = '/') => render(
  <MemoryRouter initialEntries={[entry]}><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>{node}</QueryClientProvider></MemoryRouter>);
const reply = <T,>(data: T) => ({ data, response: new Response() });
const invitation = (extra: Partial<Model<'Invitation'>> = {}): Model<'Invitation'> => ({
  id: '33333333-3333-4333-8333-333333333333', role: 'viewer', created_at: '2026-01-01T00:00:00Z',
  expires_at: '2999-01-01T00:00:00Z', used_at: null, revoked_at: null, ...extra,
});

it('solo el propietario ve la gestión y los demás no hacen ninguna petición', () => {
  const get = vi.spyOn(api, 'GET');
  wrap(<Sharing net="red" canManage={false}/>);
  expect(screen.getByText(/Solo el propietario de la organización puede invitar/)).toBeVisible();
  expect(get).not.toHaveBeenCalled();
});

it('crear una invitación muestra el enlace una vez con el rol elegido y lo copia', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async (path: string) => reply(path.endsWith('members') ? [] : [])) as never);
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply({
    id: invitation().id, token: 'atinv_sintetico', role: 'enroller', expires_at: '2999-01-01T00:00:00Z' }) as never);
  const user = userEvent.setup();
  wrap(<Sharing net="red" canManage/>);
  await user.click(screen.getByRole('radio', { name: 'Ver, generar códigos y aprobar' }));
  expect(screen.getByText(/Podrá generar códigos de unión y aprobar dispositivos escribiendo su huella completa/)).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Crear invitación' }));
  expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/invitations', { params: { path: { net: 'red' } }, body: { role: 'enroller' } });
  expect(await screen.findByText(`${window.location.origin}/?invitacion=atinv_sintetico`)).toBeVisible();
  expect(screen.getByText(/no se volverá a mostrar/)).toBeVisible();
});

it('el límite de invitaciones vigentes se explica y no se muestra enlace', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  vi.spyOn(api, 'POST').mockRejectedValue(new ApiError(409, { error: { code: 'invitation_limit', message: 'invitation_limit' } }));
  const user = userEvent.setup();
  wrap(<Sharing net="red" canManage/>);
  await user.click(screen.getByRole('button', { name: 'Crear invitación' }));
  expect(await screen.findByText(/Hay demasiadas invitaciones vigentes/)).toBeVisible();
  expect(screen.queryByText(/\?invitacion=/)).not.toBeInTheDocument();
});

it('las invitaciones y los miembros se listan, y anular o quitar piden confirmación', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async (path: string) => reply(path.endsWith('members') ? [
    { user_id: '44444444-4444-4444-8444-444444444444', role: 'enroller', display_name: 'Ana', created_at: '2026-01-01T00:00:00Z' },
  ] : [invitation(), invitation({ id: '55555555-5555-4555-8555-555555555555', used_at: '2026-01-02T00:00:00Z' })])) as never);
  const del = vi.spyOn(api, 'DELETE').mockResolvedValue(reply(undefined) as never);
  const user = userEvent.setup();
  wrap(<Sharing net="red" canManage/>);
  expect(await screen.findByText('Ana')).toBeVisible();
  expect(screen.getByText('Usada')).toBeVisible();
  // Solo la vigente se puede anular.
  expect(screen.getAllByRole('button', { name: 'Anular' })).toHaveLength(1);
  await user.click(screen.getByRole('button', { name: 'Anular' }));
  await user.click(screen.getByRole('button', { name: 'Anular invitación' }));
  await waitFor(() => expect(del).toHaveBeenCalledWith('/api/v1/networks/{net}/invitations/{id}', { params: { path: { net: 'red', id: invitation().id } } }));
  await user.click(await screen.findByRole('button', { name: 'Cerrar' }));
  await user.click(await screen.findByRole('button', { name: 'Quitar' }));
  await user.click(await screen.findByRole('button', { name: 'Quitar acceso' }));
  await waitFor(() => expect(del).toHaveBeenCalledWith('/api/v1/networks/{net}/members/{user}', { params: { path: { net: 'red', user: '44444444-4444-4444-8444-444444444444' } } }));
});

const pending = { ...device, status: 'pending' as const };
const base = { accessRole: 'member' as const, net: 'red', elapsed: 0, refresh: async () => {} };

it('un enroller aprueba escribiendo la huella completa y un mismatch se explica', async () => {
  const post = vi.spyOn(api, 'POST').mockRejectedValueOnce(new ApiError(409, { error: { code: 'fingerprint_mismatch', message: 'fingerprint_mismatch' } }))
    .mockResolvedValue(reply({ device_id: device.id, status: 'approved' }) as never);
  const user = userEvent.setup();
  wrap(<Devices {...base} enroll devices={[pending]}/>);
  expect(screen.queryByRole('button', { name: 'Rechazar' })).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Aprobar' }));
  await user.type(screen.getByLabelText('Huella completa'), 'otra-huella');
  await user.click(screen.getByRole('button', { name: 'Confirmar aprobación' }));
  expect(await screen.findByText('La huella no coincide con la del dispositivo pendiente.')).toBeVisible();
  await user.clear(screen.getByLabelText('Huella completa'));
  await user.type(screen.getByLabelText('Huella completa'), ` ${device.fingerprint} `);
  await user.click(screen.getByRole('button', { name: 'Confirmar aprobación' }));
  await waitFor(() => expect(post).toHaveBeenLastCalledWith('/api/v1/networks/{net}/devices/{dev}/approve', {
    params: { path: { net: 'red', dev: device.id } }, body: { fingerprint: device.fingerprint } }));
});

it('quien solo ve no puede aprobar ni rechazar', () => {
  wrap(<Devices {...base} devices={[pending]}/>);
  expect(screen.queryByRole('button', { name: 'Aprobar' })).not.toBeInTheDocument();
  expect(screen.getByText('Solo lectura')).toBeVisible();
});

it('aceptar la invitación pide confirmación y 404 explica que ya no vale y la descarta', async () => {
  const dismiss = vi.fn();
  const post = vi.spyOn(api, 'POST').mockRejectedValue(new ApiError(404, { error: { code: 'not_found', message: 'not_found' } }));
  const user = userEvent.setup();
  wrap(<AcceptInvitation token="atinv_x" done={() => {}} dismiss={dismiss}/>);
  expect(post).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Aceptar invitación' }));
  expect(post).toHaveBeenCalledWith('/api/v1/invitations/accept', { body: { token: 'atinv_x' } });
  expect(await screen.findByText(/Este enlace ya no es válido/)).toBeVisible();
  expect(dismiss).toHaveBeenCalled();
});

it('aceptar con éxito avisa con la red y el rol', async () => {
  vi.spyOn(api, 'POST').mockResolvedValue(reply({ network_id: 'net-1', role: 'viewer' }) as never);
  const done = vi.fn();
  const user = userEvent.setup();
  wrap(<AcceptInvitation token="atinv_x" done={done} dismiss={() => {}}/>);
  await user.click(screen.getByRole('button', { name: 'Aceptar invitación' }));
  await waitFor(() => expect(done).toHaveBeenCalledWith({ network_id: 'net-1', role: 'viewer' }));
});

function Probe() {
  const { token } = useInvitationToken();
  const [params] = useSearchParams();
  return <p>token={token};param={params.get('invitacion') ?? 'ninguno'}</p>;
}

it('el token del enlace se guarda en la sesión del navegador y sale de la URL', async () => {
  wrap(<Probe/>, '/?invitacion=atinv_enlace&net=x');
  expect(await screen.findByText('token=atinv_enlace;param=ninguno')).toBeVisible();
  expect(sessionStorage.getItem('atenea-invitacion')).toBe('atinv_enlace');
});

it('abandonar una red compartida pide confirmación y llama a la ruta propia', async () => {
  const network = { id: 'net-1', org_id: 'o', name: 'red-ana', ipv4_cidr: '10.0.0.0/24', ipv6_cidr: 'fd00::/64',
    root_anchor: 'a'.repeat(64), has_issuer_cert: true, issuer_cert_not_after: null, join_policy: 'cerrada', role: 'viewer' };
  vi.spyOn(api, 'GET').mockResolvedValue(reply([network]) as never);
  const del = vi.spyOn(api, 'DELETE').mockResolvedValue(reply(undefined) as never);
  const user = userEvent.setup();
  wrap(<SharedNetworks choose={() => {}}/>);
  await user.click(await screen.findByRole('button', { name: 'Abandonar' }));
  expect(del).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Abandonar la red' }));
  await waitFor(() => expect(del).toHaveBeenCalledWith('/api/v1/networks/{net}/membership', { params: { path: { net: 'net-1' } } }));
});
