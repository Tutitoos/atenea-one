// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, expect, it, vi } from 'vitest';
import { axe } from 'vitest-axe';
import { api, ApiError, type Model } from '@atenea/api-client';
import { Subnets } from './subnets';
import { Dns } from './dns';
import { Devices } from './devices';
import { device } from './device.fixture';
const route: Model<'Route'> = { id: '11111111-1111-4111-8111-111111111111', device_id: device.id, prefix: '192.168.50.0/24', status: 'pending', snat: null, declared_at: '2026-01-01T00:00:00Z', decided_at: null };
function mount(children: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(<QueryClientProvider client={client}>{children}</QueryClientProvider>);
}
afterEach(() => vi.restoreAllMocks());
it.each([false, true])('exige elegir SNAT explícitamente y envía %s sin valor predeterminado', async snat => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [route], response: new Response() });
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ response: new Response(null, { status: 204 }) });
  const user = userEvent.setup();
  mount(<Subnets net="red" canEdit devices={[device]}/>);
  await user.click(await screen.findByRole('button', { name: 'Aprobar' }));
  const dialog = screen.getByRole('dialog');
  expect(dialog).toHaveTextContent(device.name); expect(dialog).toHaveTextContent(route.prefix);
  await waitFor(() => expect(within(dialog).getByRole('radiogroup', { name: 'Elige explícitamente el modo SNAT' })).toBeVisible());
  expect(within(dialog).getByRole('button', { name: 'Aprobar ruta' })).toBeDisabled();
  for (const radio of within(dialog).getAllByRole('radio')) expect(radio).not.toBeChecked();
  await user.click(within(dialog).getByRole('radio', { name: snat ? /^Con SNAT/ : /^Sin SNAT/ }));
  expect(within(dialog).getByRole('radio', { name: snat ? /^Con SNAT/ : /^Sin SNAT/ })).toHaveFocus();
  await user.click(within(dialog).getByRole('button', { name: 'Aprobar ruta' }));
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/routes/{id}/approve', { params: { path: { net: 'red', id: route.id } }, body: { snat } }));
  expect(await screen.findByText('Ruta aprobada.')).toBeVisible();
});
it('explica el conflicto route_overlap sin ocultar el diálogo', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [route], response: new Response() });
  vi.spyOn(api, 'POST').mockRejectedValue(new ApiError(409, { error: { code: 'route_overlap', message: 'overlap' } }));
  const user = userEvent.setup(); mount(<Subnets net="red" canEdit devices={[device]}/>);
  await user.click(await screen.findByRole('button', { name: 'Aprobar' }));
  await user.click(screen.getByRole('radio', { name: /^Sin SNAT/ }));
  await user.click(screen.getByRole('button', { name: 'Aprobar ruta' }));
  expect(await screen.findByText(/No se puede aprobar: el prefijo se solapa/)).toBeVisible();
});
it('member lee rutas sin acciones y DNS sin formulario', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [route], response: new Response() });
  const { unmount } = mount(<Subnets net="red" canEdit={false} devices={[device]}/>);
  expect(await screen.findByText(route.prefix)).toBeVisible();
  expect(screen.queryByRole('button', { name: /Aprobar|Rechazar|Revocar/ })).not.toBeInTheDocument();
  unmount(); vi.spyOn(api, 'GET').mockResolvedValue({ data: { servers: ['10.77.0.1'], domains: ['lab.internal'] }, response: new Response() });
  mount(<Dns net="red" canEdit={false}/>);
  expect(await screen.findByText('lab.internal')).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Guardar DNS' })).not.toBeInTheDocument();
});
it.each(['pending', 'approved'] as const)('confirma antes de rechazar/revocar %s', async status => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [{ ...route, status, snat: status === 'approved' ? false : null }], response: new Response() });
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ response: new Response(null, { status: 204 }) });
  const user = userEvent.setup(); mount(<Subnets net="red" canEdit devices={[device]}/>);
  await user.click(await screen.findByRole('button', { name: status === 'approved' ? 'Revocar' : 'Rechazar' }));
  expect(post).not.toHaveBeenCalled();
  expect(screen.getByRole('dialog')).toHaveTextContent(route.prefix);
  await user.click(screen.getByRole('button', { name: status === 'approved' ? 'Confirmar revocación' : 'Confirmar rechazo' }));
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/routes/{id}/reject', { params: { path: { net: 'red', id: route.id } } }));
});
it('muestra el error autoritativo de alcance DNS', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: { servers: [], domains: [] }, response: new Response() });
  vi.spyOn(api, 'PUT').mockRejectedValue(new ApiError(400, { error: { code: 'invalid_dns', message: 'Servidor fuera de una ruta aprobada.' } }));
  const user = userEvent.setup(); mount(<Dns net="red" canEdit/>);
  await user.type(await screen.findByLabelText('Servidores DNS (separados por espacios)'), '10.77.0.1');
  await user.type(screen.getByLabelText('Dominios de enrutamiento (separados por espacios)'), 'lab.internal');
  await user.click(screen.getByRole('button', { name: 'Guardar DNS' }));
  expect(await screen.findByText('Servidor fuera de una ruta aprobada.')).toBeVisible();
});
it('distingue informe ausente, fallo de forwarding y conflicto declarado con antigüedad', async () => {
  const reported: Model<'Device'> = { ...device, report: { reported_at: '2026-01-01T00:00:00Z', age_secs: 20, nat_mapping: null, paths: [], forwarding_ok: false, dns: 'unsupported', routes: [{ prefix: route.prefix, state: 'conflict', reason: 'Se solapa con la LAN local' }] } };
  const { rerender } = render(<Devices devices={[reported]} accessRole="member" net="red" elapsed={5} refresh={async () => {}} routes={[{ ...route, status: 'approved', snat: false }]}/>);
  await userEvent.setup().click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  expect(screen.getByText(/Este router tiene rutas aprobadas/)).toBeVisible();
  expect(screen.getByText('DNS: unsupported')).toBeVisible();
  expect(screen.getByText(/conflicto · Se solapa con la LAN local/)).toBeVisible();
  expect(screen.getByText(/Declarado por el agente hace 25 s/)).toBeVisible();
  rerender(<Devices devices={[{ ...reported, report: { ...reported.report!, forwarding_ok: true } }]} accessRole="member" net="red" elapsed={5} refresh={async () => {}} routes={[{ ...route, status: 'approved' }]}/>);
  expect(screen.queryByText(/Este router tiene rutas aprobadas/)).not.toBeInTheDocument();
});
it('cubre carga, vacío y errores de rutas', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [], response: new Response() });
  const { unmount } = mount(<Subnets net="red" canEdit={false} devices={[]}/>);
  expect(screen.getByText('Cargando…')).toBeVisible();
  expect(await screen.findByText('No hay rutas declaradas por los dispositivos.')).toBeVisible(); unmount();
  vi.spyOn(api, 'GET').mockRejectedValue(new Error('Sin conexión REST'));
  mount(<Subnets net="red" canEdit={false} devices={[]}/>);
  expect(await screen.findByText('Error: Sin conexión REST')).toBeVisible();
  expect(screen.getByRole('button', { name: 'Reintentar rutas' })).toBeVisible();
});
it('subredes, aprobación y DNS sin infracciones axe básicas', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [route], response: new Response() });
  const user = userEvent.setup(); const { container, unmount } = mount(<Subnets net="red" canEdit devices={[device]}/>);
  await screen.findByText(route.prefix);
  const options = { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } };
  expect((await axe(container, options)).violations).toEqual([]);
  await user.click(screen.getByRole('button', { name: 'Aprobar' }));
  expect((await axe(screen.getByRole('dialog'), options)).violations).toEqual([]); unmount();
  vi.spyOn(api, 'GET').mockResolvedValue({ data: { servers: [], domains: [] }, response: new Response() });
  const dns = mount(<Dns net="red" canEdit/>); await screen.findByRole('button', { name: 'Guardar DNS' });
  expect((await axe(dns.container, options)).violations).toEqual([]);
});

it.each([
  ['pending', null, 'Pendiente', 'Pendiente de decidir'],
  ['approved', true, 'Aprobada', 'Aplicado'],
  ['approved', false, 'Aprobada', 'No aplicado'],
] as const)('presenta nombre, insignia, SNAT y fechas de la ruta %s (%s)', async (status, snat, label, mode) => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [{
    ...route, status, snat, decided_at: status === 'approved' ? route.declared_at : null,
  }], response: new Response() });
  mount(<Subnets net="red" canEdit={false} devices={[device]}/>);
  expect(await screen.findByText(device.name)).toBeVisible();
  expect(screen.getByTitle(device.id)).toHaveTextContent(`${device.id.slice(0, 8)}…`);
  expect(screen.getByText(label)).toHaveClass('badge');
  expect(screen.getByText(mode)).toBeVisible();
  expect(screen.getByRole('table').querySelector('time')).toHaveAttribute('dateTime', route.declared_at);
});
