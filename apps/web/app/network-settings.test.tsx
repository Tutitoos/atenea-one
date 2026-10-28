// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { afterEach, expect, it, vi } from 'vitest';
import { axe } from 'vitest-axe';
import { api, type Model } from '@atenea/api-client';
import { NetworkSettings } from './network-settings';
import { Dns } from './dns';

const network: Model<'Network'> = {
  id: '00000000-0000-4000-8000-000000000002', org_id: '00000000-0000-4000-8000-000000000001',
  name: 'Red sintética', ipv4_cidr: '10.42.0.0/24', ipv6_cidr: 'fd42::/64',
  root_anchor: 'ab'.repeat(32), has_issuer_cert: false, join_policy: 'cerrada',
  issuer_cert_not_after: null,
};
function Settings({ canEdit = true, installed = false, refresh = async () => {} }: {
  canEdit?: boolean; installed?: boolean; refresh?: () => Promise<unknown>;
}) {
  const detail = useQuery({ queryKey: ['detail'], queryFn: async () => ({ ...network, has_issuer_cert: installed }),
    initialData: { ...network, has_issuer_cert: installed } });
  const devices = useQuery({ queryKey: ['devices'], queryFn: async (): Promise<Model<'Device'>[]> => [], initialData: [] });
  const routes = useQuery({ queryKey: ['routes'], queryFn: async (): Promise<Model<'Route'>[]> => [], initialData: [] });
  return <NetworkSettings net={network.id} network={detail} devices={devices} routes={routes}
    role={canEdit ? 'admin' : 'member'} canEdit={canEdit} elapsed={0} refresh={refresh}/>;
}
function wrap(node: React.ReactNode) {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
    {node}
  </QueryClientProvider>);
}
afterEach(() => vi.restoreAllMocks());
function mockGet() {
  vi.spyOn(api, 'GET').mockImplementation((async (path: string) => ({ data: path.endsWith('/dns') ?
    { servers: ['10.42.0.53'], domains: ['servicios.example.test'] } :
    { code: 'ATENEA1-SINTETICO', root_anchor_fingerprint: 'aaaa-bbbb', join_policy: 'cerrada' }, response: new Response() })) as never);
}
it('asocia los errores DNS con sus campos y guarda valores válidos', async () => {
  mockGet(); const put = vi.spyOn(api, 'PUT').mockResolvedValue({ response: new Response(null, { status: 204 }) });
  const user = userEvent.setup(); wrap(<Dns net="red" canEdit/>);
  const servers = await screen.findByLabelText('Servidores DNS (separados por espacios)');
  const domains = screen.getByLabelText('Dominios de enrutamiento (separados por espacios)');
  await user.clear(servers); await user.type(servers, 'no-es-ip');
  await user.clear(domains); await user.type(domains, 'INVALIDO.');
  await user.click(screen.getByRole('button', { name: 'Guardar DNS' }));
  expect(servers).toHaveAttribute('aria-invalid', 'true');
  expect(servers).toHaveAccessibleDescription(/hasta 3 direcciones IP válidas/);
  expect(domains).toHaveAttribute('aria-invalid', 'true');
  expect(domains).toHaveAccessibleDescription(/hasta 8 dominios LDH/);
  expect(put).not.toHaveBeenCalled();
  await user.clear(servers); await user.type(servers, '10.42.0.53');
  await user.clear(domains); await user.type(domains, 'servicios.example.test');
  await user.click(screen.getByRole('button', { name: 'Guardar DNS' }));
  expect(await screen.findByText('DNS actualizado.')).toBeVisible();
  expect(put).toHaveBeenCalledWith('/api/v1/networks/{net}/dns', {
    params: { path: { net: 'red' } }, body: { servers: ['10.42.0.53'], domains: ['servicios.example.test'] },
  });
});
it('muestra errores del servidor DNS sin anunciar éxito', async () => {
  mockGet(); vi.spyOn(api, 'PUT').mockRejectedValue(new Error('Servidor DNS fuera de alcance.'));
  const user = userEvent.setup(); wrap(<Dns net="red" canEdit/>);
  await screen.findByLabelText('Servidores DNS (separados por espacios)');
  await user.click(screen.getByRole('button', { name: 'Guardar DNS' }));
  expect(await screen.findByText('Servidor DNS fuera de alcance.')).toBeVisible();
  expect(screen.queryByText('DNS actualizado.')).not.toBeInTheDocument();
});
it.each([false, true])('sube o reemplaza el certificado público (instalado: %s)', async installed => {
  mockGet(); const put = vi.spyOn(api, 'PUT').mockResolvedValue({ response: new Response(null, { status: 204 }) });
  const refresh = vi.fn(async () => {}); const user = userEvent.setup(); wrap(<Settings installed={installed} refresh={refresh}/>);
  await user.click(screen.getByRole('button', { name: installed ? 'Reemplazar certificado' : 'Subir certificado' }));
  await user.click(screen.getByRole('button', { name: 'Subir issuer.cert' }));
  const input = screen.getByLabelText('Certificado público');
  expect(input).toHaveAttribute('aria-invalid', 'true'); expect(put).not.toHaveBeenCalled();
  const file = new File([new Uint8Array(178)], 'issuer.cert', { type: 'application/octet-stream' });
  await user.upload(input, file);
  await user.click(screen.getByRole('button', { name: 'Subir issuer.cert' }));
  await waitFor(() => expect(refresh).toHaveBeenCalledOnce());
  expect(await screen.findByText('Certificado instalado.')).toBeVisible();
  expect(put).toHaveBeenCalledWith('/api/v1/networks/{net}/issuer-cert', expect.objectContaining({
    headers: { 'Content-Type': 'application/octet-stream' }, bodySerializer: expect.any(Function),
  }));
});
it('exige escribir el nombre para borrar y conserva errores del servidor', async () => {
  mockGet(); const remove = vi.spyOn(api, 'DELETE').mockRejectedValue(new Error('No se pudo eliminar.'));
  const user = userEvent.setup(); wrap(<Settings/>);
  await user.click(screen.getByRole('button', { name: 'Eliminar red' }));
  await user.click(screen.getByRole('button', { name: 'Eliminar definitivamente' }));
  expect(remove).not.toHaveBeenCalled();
  await user.type(screen.getByLabelText(`Escribe «${network.name}» para confirmar`), network.name);
  await user.click(screen.getByRole('button', { name: 'Eliminar definitivamente' }));
  expect(await screen.findByText('No se pudo eliminar.')).toBeVisible();
  expect(remove).toHaveBeenCalledWith('/api/v1/networks/{net}', { params: { path: { net: network.id } } });
});
it('member tiene solo lectura, sin acciones administrativas, y la página es accesible', async () => {
  mockGet(); const { container } = wrap(<Settings canEdit={false}/>);
  expect(await screen.findByText('servicios.example.test')).toBeVisible();
  expect(screen.getByText(network.root_anchor)).toBeVisible();
  expect(screen.getByText(/Pide la huella de la raíz/)).toBeVisible();
  expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /Guardar DNS|Subir certificado|Reemplazar certificado|Eliminar red/ })).not.toBeInTheDocument();
  expect(api.GET).toHaveBeenCalledTimes(1);
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
