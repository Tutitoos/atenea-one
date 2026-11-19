// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-029: crear una red con solo un nombre, comando de unión y política en un clic.
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, expect, it, vi } from 'vitest';
import { api, ApiError, type Model } from '@atenea/api-client';
import { Organizations } from './organizations';
import { QuickStart } from './quick-start';
import { NetworkSettings } from './network-settings';

afterEach(() => vi.restoreAllMocks());

const wrap = (node: React.ReactNode) => render(
  <MemoryRouter><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>{node}</QueryClientProvider></MemoryRouter>);
const reply = <T,>(data: T) => ({ data, response: new Response() });
const org: Model<'Organization'> = { id: '00000000-0000-4000-8000-000000000001', name: 'Personal', role: 'owner' };
const created = { id: '00000000-0000-4000-8000-0000000000aa', org_id: org.id, name: 'casa', ipv4_cidr: '10.64.0.0/24',
  ipv6_cidr: 'fd00::/64', root_anchor: 'ab'.repeat(32), root_mode: 'managed', has_issuer_cert: true, issuer_cert_not_after: null, join_policy: 'cerrada' };

it('crear una red envía solo el nombre: el bloque es «Automático» y no hay campos de CIDR ni de raíz', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply(created) as never);
  const choose = vi.fn();
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={choose}/>);
  await user.click((await screen.findAllByRole('button', { name: 'Crear red' }))[0]);
  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByRole('radio', { name: /Automático/ })).toBeChecked();
  expect(within(dialog).getByRole('radio', { name: '10.147.17.*' })).toHaveAttribute('title', '10.147.17.0/24');
  expect(within(dialog).queryByText(/raíz|CIDR/i)).toBeNull();
  await user.type(within(dialog).getByLabelText('Nombre'), 'casa');
  await user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/networks', { params: { path: { org: org.id } }, body: { name: 'casa' } }));
  await waitFor(() => expect(choose).toHaveBeenCalledWith(created.id));
});

it('elegir un bloque lo envía y volver a «Automático» lo quita', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply(created) as never);
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={() => {}}/>);
  await user.click((await screen.findAllByRole('button', { name: 'Crear red' }))[0]);
  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('Nombre'), 'oficina');
  await user.click(within(dialog).getByRole('radio', { name: '172.25.*.*' }));
  expect(within(dialog).getByRole('radio', { name: '172.25.*.*' })).toBeChecked();
  expect(within(dialog).getByRole('radio', { name: /Automático/ })).not.toBeChecked();
  await user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/networks', { params: { path: { org: org.id } }, body: { name: 'oficina', ipv4_cidr: '172.25.0.0/16' } }));
  post.mockClear();
  await user.click(within(dialog).getByRole('radio', { name: /Automático/ }));
  await user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/networks', { params: { path: { org: org.id } }, body: { name: 'oficina' } }));
});

it('en Avanzado se da inicio y fin del rango; un rango mal escrito o al revés no sale del panel', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply(created) as never);
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={() => {}}/>);
  await user.click((await screen.findAllByRole('button', { name: 'Crear red' }))[0]);
  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('Nombre'), 'oficina');
  await user.click(within(dialog).getByRole('tab', { name: 'Avanzado' }));
  expect(within(dialog).queryByRole('radio')).toBeNull();
  const [start, end] = [within(dialog).getByLabelText('Inicio del rango'), within(dialog).getByLabelText('Fin del rango')];
  const send = () => user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  await user.type(start, '10.244.0.1'); await send();
  expect(await within(dialog).findByText(/inicio y el fin como direcciones IPv4/)).toBeVisible();
  await user.type(end, '10.243.0.9'); await send();
  expect(await within(dialog).findByText(/debe ser menor o igual que el fin/)).toBeVisible();
  expect(post).not.toHaveBeenCalled();
  await user.clear(end); await user.type(end, '10.244.0.254'); await send();
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/networks', { params: { path: { org: org.id } }, body: { name: 'oficina', ipv4_pool: { start: '10.244.0.1', end: '10.244.0.254' } } }));
});

it('desmarcar IPv6 crea una red solo IPv4, en cualquiera de los dos modos', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply(created) as never);
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={() => {}}/>);
  await user.click((await screen.findAllByRole('button', { name: 'Crear red' }))[0]);
  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('Nombre'), 'solo4');
  const ipv6 = within(dialog).getByRole('checkbox', { name: 'Asignar también direcciones IPv6' });
  expect(ipv6).toBeChecked();
  await user.click(ipv6);
  await user.click(within(dialog).getByRole('radio', { name: '10.144.*.*' }));
  await user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/networks', { params: { path: { org: org.id } }, body: { name: 'solo4', ipv6: false, ipv4_cidr: '10.144.0.0/16' } }));
});

it('un bloque ya usado se explica y no navega', async () => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  vi.spyOn(api, 'POST').mockRejectedValue(new ApiError(409, { error: { code: 'overlapping_network', message: 'x' } }));
  const choose = vi.fn();
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={choose}/>);
  await user.click((await screen.findAllByRole('button', { name: 'Crear red' }))[0]);
  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('Nombre'), 'oficina');
  await user.click(within(dialog).getByRole('radio', { name: '10.144.*.*' }));
  await user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  expect(await within(dialog).findByText(/Ese bloque ya lo usa otra red/)).toBeVisible();
  expect(choose).not.toHaveBeenCalled();
});

it.each([
  ['managed_unavailable', /no tiene activadas las redes gestionadas/],
  ['network_limit', /máximo de redes/],
])('un error %s se explica y no navega', async (code, message) => {
  vi.spyOn(api, 'GET').mockImplementation((async () => reply([])) as never);
  vi.spyOn(api, 'POST').mockRejectedValue(new ApiError(409, { error: { code, message: code } }));
  const choose = vi.fn();
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={choose}/>);
  await user.click((await screen.findAllByRole('button', { name: 'Crear red' }))[0]);
  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('Nombre'), 'casa');
  await user.click(within(dialog).getAllByRole('button', { name: 'Crear red' })[0]);
  expect(await within(dialog).findByText(message)).toBeVisible();
  expect(choose).not.toHaveBeenCalled();
});

const token = { token: 't', expires_at: '2999-01-01T00:00:00Z', code: 'ATENEA1-SINTETICO', root_anchor_fingerprint: 'f',
  connectivity: null, join: { url: 'https://panel.example.test', token: 't', network_id: created.id, root_anchor: 'ab'.repeat(32) } };

it('añadir un equipo pide un token de una hora con aprobación automática y muestra un solo comando', async () => {
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply(token) as never);
  const user = userEvent.setup();
  wrap(<QuickStart net="red" canEdit enroll={false} policies={1}/>);
  await user.click(screen.getByRole('button', { name: 'Generar comando' }));
  expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/enrollment-tokens', {
    params: { path: { net: 'red' } }, body: { ttl_secs: 3600, auto_approve: true, role: 'device' } });
  expect(await screen.findByText("sudo atenea network join 'ATENEA1-SINTETICO' --up")).toBeVisible();
  expect(screen.getByText(/No se vuelve a mostrar/)).toBeVisible();
  // Con políticas ya creadas no se ofrece «permitir todo».
  expect(screen.queryByRole('button', { name: 'Permitir todo entre equipos' })).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Ya lo copié: ocultarlo' }));
  expect(screen.queryByText(/ATENEA1-SINTETICO/)).not.toBeInTheDocument();
});

it('sin políticas se ofrece permitir todo, con confirmación, y un enroller no lo ve', async () => {
  const post = vi.spyOn(api, 'POST').mockResolvedValue(reply({ id: 'p' }) as never);
  const user = userEvent.setup();
  const { unmount } = wrap(<QuickStart net="red" canEdit enroll policies={0}/>);
  await user.click(screen.getByRole('button', { name: 'Permitir todo entre equipos' }));
  expect(post).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Crear la política' }));
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/policies', {
    params: { path: { net: 'red' } }, body: { src: { kind: 'any' }, dst: { kind: 'any' }, proto: 0, port_min: 0, port_max: 0 } }));
  unmount();
  wrap(<QuickStart net="red" canEdit={false} enroll policies={0}/>);
  expect(screen.getByRole('button', { name: 'Generar comando' })).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Permitir todo entre equipos' })).not.toBeInTheDocument();
});

it('quien solo ve no recibe la tarjeta', () => {
  const { container } = wrap(<QuickStart net="red" canEdit={false} enroll={false} policies={0}/>);
  expect(container).toBeEmptyDOMElement();
});

it('en una red gestionada no se ofrece subir el certificado: lo renueva el servidor', () => {
  const detail = { data: created, isPending: false, isError: false, error: null, dataUpdatedAt: 0, isFetching: false } as never;
  vi.spyOn(api, 'GET').mockImplementation((async () => reply({ servers: [], domains: [] })) as never);
  wrap(<NetworkSettings net={created.id} network={detail} devices={detail} routes={detail} role={'owner' as const} canEdit elapsed={0} refresh={async () => {}}/>);
  expect(screen.getByText(/lo firma y renueva el servidor solo/)).toBeVisible();
  expect(screen.queryByRole('button', { name: /certificado/i })).not.toBeInTheDocument();
});
