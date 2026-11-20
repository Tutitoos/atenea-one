// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { axe } from 'vitest-axe';
import { afterEach, expect, it, vi } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { api, ApiError } from '@atenea/api-client';
import { Devices } from './devices';
import { device } from './device.fixture';
import { choose } from './test-setup';

const base = { accessRole: 'admin' as const, net: 'red', elapsed: 0, refresh: async () => {} };
const approved = { ...device, status: 'approved' as const, name_source: 'device' as const, hostname: 'equipo-sistema' };
afterEach(() => vi.restoreAllMocks());

it('presenta el contador, origen, fecha y huella copiable; aprobar requiere confirmación', async () => {
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ data: {}, response: new Response() });
  const user = userEvent.setup();
  render(<Devices {...base} devices={[device]}/>);
  expect(screen.getByRole('heading', { name: 'Solicitudes pendientes (1)' })).toBeVisible();
  expect(screen.getByText('Token de incorporación')).toBeVisible();
  expect(screen.getByText(new Date(device.enrolled_at).toLocaleString('es'))).toBeVisible();
  expect(screen.getByRole('button', { name: `Copiar huella de ${device.name}` })).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Aprobar' }));
  expect(screen.getByRole('dialog')).toHaveTextContent(device.fingerprint);
  expect(post).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar aprobación' }));
  await waitFor(() => expect(post).toHaveBeenCalledWith(
    '/api/v1/networks/{net}/devices/{dev}/approve',
    { params: { path: { net: 'red', dev: device.id } } },
  ));
});

it('rechaza una solicitud solo tras explicar y confirmar la revocación irreversible', async () => {
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ data: {}, response: new Response() });
  const user = userEvent.setup();
  render(<Devices {...base} devices={[device]}/>);
  await user.click(screen.getByRole('button', { name: 'Rechazar' }));
  expect(screen.getByRole('dialog')).toHaveTextContent('No se podrá volver a aprobar esta solicitud');
  expect(post).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar rechazo' }));
  await waitFor(() => expect(post).toHaveBeenCalledWith(
    '/api/v1/networks/{net}/devices/{dev}/revoke',
    { params: { path: { net: 'red', dev: device.id } } },
  ));
});

it('busca por nombre, IP y huella, filtra estado y tipo y ordena ambas direcciones', async () => {
  const user = userEvent.setup();
  const second = { ...approved, id: '22222222-2222-4222-8222-222222222222',
    name: 'Alfa', role: 'server' as const, ipv4: '10.0.0.2', fingerprint: 'huella-alfa' };
  render(<Devices {...base} devices={[approved, second]}/>);
  const search = screen.getByRole('searchbox', { name: 'Buscar dispositivos' });
  for (const query of ['Alfa', '10.0.0.2', 'huella-alfa']) {
    await user.clear(search);
    await user.type(search, query);
    expect(screen.getAllByRole('row')).toHaveLength(2);
    expect(screen.getByRole('button', { name: 'Ver detalle de Alfa' })).toBeVisible();
  }
  await user.clear(search);
  await choose(user, screen.getByRole('combobox', { name: 'Estado' }), 'revoked');
  expect(screen.getByText('Ningún resultado para el filtro.')).toBeVisible();
  await choose(user, screen.getByRole('combobox', { name: 'Estado' }), 'all');
  await choose(user, screen.getByRole('combobox', { name: 'Tipo' }), 'server');
  expect(screen.getAllByRole('row')).toHaveLength(2);
  await choose(user, screen.getByRole('combobox', { name: 'Tipo' }), 'all');
  expect(within(screen.getAllByRole('row')[1]).getByText('Alfa')).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Ordenar por Nombre' }));
  expect(within(screen.getAllByRole('row')[1]).getByText(device.name)).toBeVisible();
});

it('distingue inventario vacío, solicitudes pendientes y ausencia por filtros sin ofrecer tokens a member', () => {
  const { rerender } = render(<Devices {...base} devices={[]}/>);
  expect(screen.getByText('No hay dispositivos.')).toBeVisible();
  expect(screen.getByText(/Crea un token en Incorporar/)).toBeVisible();
  rerender(<Devices {...base} accessRole="member" devices={[]}/>);
  expect(screen.queryByText(/Crea un token/)).not.toBeInTheDocument();
  rerender(<Devices {...base} accessRole="member" devices={[device]}/>);
  expect(screen.getByText('Hay solicitudes pendientes de revisión.')).toBeVisible();
});

it('guarda solo endpoints sin convertir el nombre del sistema en administrativo', async () => {
  const patch = vi.spyOn(api, 'PATCH').mockResolvedValue({ data: { device_id: device.id }, response: new Response() });
  const user = userEvent.setup();
  render(<Devices {...base} devices={[approved]}/>);
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  await user.click(screen.getByRole('button', { name: 'Editar' }));
  await user.type(screen.getByRole('textbox', { name: 'Endpoints separados por coma' }), '192.0.2.1:8080');
  await user.click(screen.getByRole('button', { name: 'Guardar dispositivo' }));
  await waitFor(() => expect(patch).toHaveBeenCalledWith(
    '/api/v1/networks/{net}/devices/{dev}',
    { params: { path: { net: 'red', dev: device.id } }, body: { endpoints: ['192.0.2.1:8080'] } },
  ));
  expect(screen.getByRole('dialog')).toHaveTextContent('dejará de seguir el nombre del sistema');
});

it('restaura el nombre del sistema y revoca aprobados con confirmación', async () => {
  const patch = vi.spyOn(api, 'PATCH').mockResolvedValue({ data: { device_id: device.id }, response: new Response() });
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ data: {}, response: new Response() });
  const user = userEvent.setup();
  render(<Devices {...base} devices={[{ ...approved, name_source: 'admin' }]}/>);
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  await user.click(screen.getByRole('button', { name: 'Volver al nombre del sistema' }));
  await user.click(screen.getByRole('button', { name: 'Confirmar nombre del sistema' }));
  await waitFor(() => expect(patch).toHaveBeenCalledWith(
    '/api/v1/networks/{net}/devices/{dev}',
    { params: { path: { net: 'red', dev: device.id } }, body: { use_system_name: true } },
  ));
  await user.keyboard('{Escape}');
  await user.click(screen.getByRole('button', { name: 'Revocar' }));
  expect(post).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar revocación' }));
  await waitFor(() => expect(post).toHaveBeenCalledOnce());
});

it('member consulta detalles sin acciones y la pantalla no tiene infracciones axe en jsdom', async () => {
  const user = userEvent.setup();
  const { container } = render(<Devices {...base} accessRole="member" devices={[device, { ...approved, id: 'otro' }]}/>);
  await user.click(screen.getAllByRole('button', { name: `Ver detalle de ${device.name}` })[1]);
  expect(screen.getByText('Origen del nombre')).toBeVisible();
  for (const name of ['Aprobar', 'Rechazar', 'Editar', 'Cambiar tipo', 'Revocar', 'Volver al nombre del sistema']) {
    expect(screen.queryByRole('button', { name })).not.toBeInTheDocument();
  }
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('aplica el filtro inicial de pendientes y permite limpiarlo sin perder el inventario', async () => {
  const user = userEvent.setup();
  render(<Devices {...base} status="pending" devices={[device, { ...approved, id: 'otro', name: 'Aprobado sintético' }]}/>);
  expect(screen.getByRole('combobox', { name: 'Estado' })).toHaveTextContent('Pendiente');
  expect(screen.queryByRole('button', { name: 'Ver detalle de Aprobado sintético' })).not.toBeInTheDocument();
  await choose(user, screen.getByRole('combobox', { name: 'Estado' }), 'all');
  expect(screen.getByRole('button', { name: 'Ver detalle de Aprobado sintético' })).toBeVisible();
});

it('guarda solo el nombre cuando cambia y no envía un PATCH si no cambia nada', async () => {
  const patch = vi.spyOn(api, 'PATCH').mockResolvedValue({ data: { device_id: device.id }, response: new Response() });
  const user = userEvent.setup();
  render(<Devices {...base} devices={[approved]}/>);
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  await user.click(screen.getByRole('button', { name: 'Editar' }));
  await user.click(screen.getByRole('button', { name: 'Guardar dispositivo' }));
  await screen.findByText('Acción completada.');
  expect(patch).not.toHaveBeenCalled();
  await user.clear(screen.getByRole('textbox', { name: 'Nombre' }));
  await user.type(screen.getByRole('textbox', { name: 'Nombre' }), 'Nombre administrativo sintético');
  await user.click(screen.getByRole('button', { name: 'Guardar dispositivo' }));
  await waitFor(() => expect(patch).toHaveBeenCalledWith(
    '/api/v1/networks/{net}/devices/{dev}',
    { params: { path: { net: 'red', dev: device.id } }, body: { name: 'Nombre administrativo sintético' } },
  ));
});

it.each([
  { ipv4: '192.0.2.5', ipv6: null, label: '192.0.2.5' },
  { ipv4: null, ipv6: '2001:db8::5', label: '2001:db8::5' },
  { ipv4: null, ipv6: null, label: '—' },
])('muestra solo las direcciones existentes: $label', ({ ipv4, ipv6, label }) => {
  render(<Devices {...base} devices={[{ ...approved, ipv4, ipv6 }]}/>);
  const row = screen.getAllByRole('row')[1];
  expect(within(row).getAllByRole('cell')[3]).toHaveTextContent(new RegExp(`^${label}$`));
  expect(within(row).getAllByRole('cell')[2]).toHaveTextContent(/^Desconocida$/);
});

const peer = { ...approved, id: '33333333-3333-4333-8333-333333333333', name: 'Equipo par' };
const note = 'No modifica las políticas manuales; otra política puede seguir permitiendo el acceso.';
/** ADR-031: el detalle consulta las conexiones automáticas cuando el panel las activa (`autoConnections`). */
function renderConnections(props: Partial<React.ComponentProps<typeof Devices>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const refresh = vi.fn(() => client.invalidateQueries({ queryKey: ['network', 'red'] }));
  const view = render(<QueryClientProvider client={client}>
    <Devices {...base} autoConnections devices={[approved, peer]} refresh={refresh} {...props}/>
  </QueryClientProvider>);
  return { refresh, container: view.container };
}

it('lista solo las conexiones automáticas del dispositivo, con su nombre, y no tiene infracciones axe', async () => {
  const get = vi.spyOn(api, 'GET').mockResolvedValue({ data: [peer.id], response: new Response() });
  const user = userEvent.setup();
  const { container } = renderConnections();
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${approved.name}` }));
  expect(await screen.findByRole('list', { name: `Conexiones automáticas de ${approved.name}` })).toHaveTextContent(peer.name);
  expect(get).toHaveBeenCalledWith('/api/v1/networks/{net}/devices/{dev}/auto-peers', { params: { path: { net: 'red', dev: approved.id } } });
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('quitar una conexión automática pide confirmar, avisa de que no toca las políticas manuales y recarga', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [peer.id], response: new Response() });
  const remove = vi.spyOn(api, 'DELETE').mockResolvedValue({ response: new Response(null, { status: 204 }) } as never);
  const user = userEvent.setup();
  const { refresh } = renderConnections();
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${approved.name}` }));
  expect(await screen.findByText(note)).toBeVisible();
  await user.click(screen.getByRole('button', { name: `Quitar conexión con ${peer.name}` }));
  expect(screen.getByRole('dialog')).toHaveTextContent(note);
  expect(remove).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Quitar conexión' }));
  await waitFor(() => expect(remove).toHaveBeenCalledWith('/api/v1/networks/{net}/devices/{dev}/auto-peers/{peer}', {
    params: { path: { net: 'red', dev: approved.id, peer: peer.id } },
  }));
  await waitFor(() => expect(refresh).toHaveBeenCalledOnce());
});

it('si la conexión ya no es automática (404), lo dice y recarga la lista sin anunciar éxito', async () => {
  const get = vi.spyOn(api, 'GET').mockResolvedValue({ data: [peer.id], response: new Response() });
  vi.spyOn(api, 'DELETE').mockRejectedValue(new ApiError(404, { error: { code: 'not_found', message: 'not_found' } }));
  const user = userEvent.setup();
  const { refresh } = renderConnections();
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${approved.name}` }));
  await user.click(await screen.findByRole('button', { name: `Quitar conexión con ${peer.name}` }));
  await user.click(screen.getByRole('button', { name: 'Quitar conexión' }));
  expect(await screen.findByText('Esa conexión ya no es automática. La lista se ha actualizado.')).toBeVisible();
  await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
  expect(refresh).not.toHaveBeenCalled();
  expect(screen.queryByText('Acción completada.')).not.toBeInTheDocument();
});

it('un miembro ve las conexiones automáticas sin poder quitarlas', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [peer.id], response: new Response() });
  const user = userEvent.setup();
  renderConnections({ accessRole: 'member' });
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${approved.name}` }));
  expect(await screen.findByRole('list', { name: `Conexiones automáticas de ${approved.name}` })).toHaveTextContent(peer.name);
  expect(screen.queryByText(note)).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: `Quitar conexión con ${peer.name}` })).not.toBeInTheDocument();
});

it('sin conexiones automáticas lo dice', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [], response: new Response() });
  const user = userEvent.setup();
  renderConnections();
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${approved.name}` }));
  expect(await screen.findByText('Ningún otro dispositivo se conecta automáticamente con este.')).toBeVisible();
});

it('un dispositivo pendiente no consulta conexiones automáticas', async () => {
  const get = vi.spyOn(api, 'GET').mockResolvedValue({ data: [], response: new Response() });
  const user = userEvent.setup();
  renderConnections({ devices: [device, peer] });
  await user.click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  expect(get).not.toHaveBeenCalled();
});
