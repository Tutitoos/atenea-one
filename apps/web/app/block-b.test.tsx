// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router';
import { afterEach, expect, it, vi } from 'vitest';
import { axe } from 'vitest-axe';
import { api, ApiError, type Model } from '@atenea/api-client';
import { Organizations } from './organizations';
import { Overview } from './overview';
import { NetworkSettings } from './network-settings';
import { certificateExpiry, CertificateStatus, ServerInfo } from './server-status';
import { Audit } from './observability';
import { QueryAnnouncements } from './ui';
const org = { id: '00000000-0000-4000-8000-000000000001', name: 'Organización sintética', role: 'owner' as const };
const member = { user_id: '00000000-0000-4000-8000-000000000002', display_name: 'Persona sintética', email: 'persona@example.test', role: 'owner' };
const network: Model<'Network'> = { id: '00000000-0000-4000-8000-000000000003', org_id: org.id, name: 'Red sintética', ipv4_cidr: '192.0.2.0/24', ipv6_cidr: '2001:db8::/64', root_anchor: 'ab'.repeat(32), has_issuer_cert: true, issuer_cert_not_after: null, join_policy: 'cerrada' };
function mount(node: React.ReactNode) {
 const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
 return { cache, ...render(<MemoryRouter><QueryClientProvider client={cache}>{node}</QueryClientProvider></MemoryRouter>) };
}
function mockGet() {
 vi.spyOn(api, 'GET').mockImplementation((async (path: string) => ({ data: path.endsWith('/members') ? [member] : path.endsWith('/dns') ? { servers: [], domains: [] } : [], response: new Response() })) as never);
}
afterEach(() => vi.restoreAllMocks());
it.each(['owner', 'admin', 'member'] as const)('muestra identidad y protege el correo para %s', async role => {
 mockGet(); mount(<Organizations org={{ ...org, role }} choose={() => {}}/>);
 await userEvent.click(screen.getByRole('tab', { name: 'Miembros' }));
 expect(await screen.findByText(member.display_name)).toBeVisible();
 expect(screen.getByTitle(member.user_id)).toBeVisible();
 expect(screen.queryByText(member.email)?.textContent ?? null).toBe(role === 'member' ? null : member.email);
 expect(screen.queryByLabelText('Correo electrónico') !== null).toBe(role === 'owner');
});
it('añade por correo y refresca miembros con un formulario accesible', async () => {
 mockGet(); const post = vi.spyOn(api, 'POST').mockResolvedValue({ response: new Response() });
 mount(<Organizations org={org} choose={() => {}}/>); const user = userEvent.setup();
 await user.click(screen.getByRole('tab', { name: 'Miembros' }));
 await user.type(screen.getByLabelText('Correo electrónico'), 'nuevo@example.test');
 await user.click(screen.getByRole('button', { name: 'Añadir por correo' }));
 expect(await screen.findByText('Acción completada.')).toBeVisible();
 expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/members', { params: { path: { org: org.id } }, body: { email: 'nuevo@example.test', role: 'member' } });
 expect(api.GET).toHaveBeenCalledTimes(3);
 expect((await axe(screen.getByRole('tabpanel'), { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
it.each([
 [404, 'not_found', 'No hay ningún usuario con ese correo que haya iniciado sesión antes. Pide que entre una vez en el panel.'],
 [409, 'last_owner', 'Siempre debe quedar al menos un owner.'],
 [409, 'conflict', 'Varias identidades comparten ese correo; resuélvelo en el proveedor de identidad.'],
 [429, 'rate_limited', 'Demasiados intentos; espera un minuto.'],
])('explica el rechazo al añadir: %s %s', async (status, code, message) => {
 mockGet(); vi.spyOn(api, 'POST').mockRejectedValue(new ApiError(Number(status), { error: { code: String(code), message: 'detalle privado' } }));
 mount(<Organizations org={org} choose={() => {}}/>); const user = userEvent.setup();
 await user.click(screen.getByRole('tab', { name: 'Miembros' }));
 await user.type(screen.getByLabelText('Correo electrónico'), 'nuevo@example.test');
 await user.click(screen.getByRole('button', { name: 'Añadir por correo' }));
 expect(await screen.findByText(String(message))).toBeVisible();
 expect(screen.queryByText('detalle privado')).not.toBeInTheDocument();
});
it('impide retirar al último owner y exige confirmar el cambio de rol', async () => {
 mockGet(); const remove = vi.spyOn(api, 'DELETE'); const put = vi.spyOn(api, 'PUT');
 mount(<Organizations org={org} choose={() => {}}/>); const user = userEvent.setup();
 await user.click(screen.getByRole('tab', { name: 'Miembros' }));
 await user.click(await screen.findByRole('button', { name: 'Retirar miembro' }));
 await user.click(screen.getByRole('button', { name: 'Confirmar retirada' }));
 expect(await screen.findByText('Siempre debe quedar al menos un owner.')).toBeVisible();
 expect(remove).not.toHaveBeenCalled(); await user.keyboard('{Escape}');
 await user.click(screen.getByRole('button', { name: 'Cambiar rol' }));
 await user.selectOptions(within(screen.getByRole('dialog')).getByLabelText('Rol'), 'admin');
 expect(put).not.toHaveBeenCalled();
 await user.click(screen.getByRole('button', { name: 'Confirmar cambio de rol' }));
 expect(await screen.findByText('Siempre debe quedar al menos un owner.')).toBeVisible(); expect(put).not.toHaveBeenCalled();
});
it.each([15, 14, 3, 0, -1])('clasifica caducidad a %s días', days => {
 const now = Date.UTC(2026, 0, 1); const expires = new Date(now + days * 86400000).toISOString();
 expect(certificateExpiry(expires, now)).toEqual({ days, tone: days > 14 ? 'ok' : days > 3 ? 'warn' : 'bad' });
});
it('no calcula una caducidad ausente', () => expect(certificateExpiry(null, 0)).toBeNull());
it.each([[20, 'ok'], [10, 'warn'], [2, 'bad'], [-1, 'bad']] as const)('muestra fecha y tono a %s días', (days, tone) => {
 const expiry = new Date(Date.now() + days * 86400000).toISOString();
 const { container } = mount(<CertificateStatus installed expires={expiry}/>);
 expect(container.querySelector(`.badge-${tone}`)).toBeInTheDocument(); expect(screen.getByText(/Caduca|Caducó/)).toBeVisible();
});
function NetworkView({ settings = false, canEdit = true, expires = new Date(Date.now() + 10 * 86400000).toISOString(), refresh = async () => {} }: { settings?: boolean; canEdit?: boolean; expires?: string | null; refresh?: () => Promise<unknown> }) {
 const detail = useQuery({ queryKey: ['network', network.id, 'detail'], queryFn: async () => ({ ...network, issuer_cert_not_after: expires }), initialData: { ...network, issuer_cert_not_after: expires } });
 const devices = useQuery({ queryKey: ['devices'], queryFn: async (): Promise<Model<'Device'>[]> => [], initialData: [] });
 const routes = useQuery({ queryKey: ['routes'], queryFn: async (): Promise<Model<'Route'>[]> => [], initialData: [] });
 const Component = settings ? NetworkSettings : Overview;
 return <Component net={network.id} network={detail} devices={devices} routes={routes} canEdit={canEdit} role={canEdit ? 'admin' : 'member'} elapsed={0} refresh={refresh}/>;
}
it('avisa en Resumen de caducidad próxima con siguiente paso y sin infracciones axe', async () => {
 mockGet(); const { container } = mount(<NetworkView/>);
 expect(screen.getByRole('note', { name: 'Caducidad del certificado' })).toHaveTextContent('Renueva el certificado con la raíz offline y súbelo en DNS y certificado');
 expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
it.each([null, new Date(Date.now() + 20 * 86400000).toISOString()])('no avisa si no hay caducidad próxima: %s', expires => {
 mockGet(); mount(<NetworkView expires={expires}/>); expect(screen.queryByRole('note', { name: 'Caducidad del certificado' })).not.toBeInTheDocument();
});
it.each(['owner', 'admin', 'member'] as const)('renombrar organización solo para owner: %s', async role => {
 mockGet(); mount(<Organizations org={{ ...org, role }} choose={() => {}}/>); await userEvent.click(screen.getByRole('tab', { name: 'Ajustes' }));
 expect(screen.queryByRole('button', { name: 'Renombrar organización' }) !== null).toBe(role === 'owner');
});
it('valida y renombra organización refrescando su lista', async () => {
 mockGet(); const patch = vi.spyOn(api, 'PATCH').mockResolvedValue({ data: org, response: new Response() });
 const { cache } = mount(<Organizations org={org} choose={() => {}}/>); const invalidate = vi.spyOn(cache, 'invalidateQueries'); const user = userEvent.setup();
 await user.click(screen.getByRole('tab', { name: 'Ajustes' })); await user.click(screen.getByRole('button', { name: 'Renombrar organización' }));
 await user.clear(screen.getByLabelText('Nuevo nombre')); await user.type(screen.getByLabelText('Nuevo nombre'), '   '); await user.click(screen.getByRole('button', { name: 'Guardar nombre' })); expect(patch).not.toHaveBeenCalled();
 await user.clear(screen.getByLabelText('Nuevo nombre')); await user.type(screen.getByLabelText('Nuevo nombre'), 'Nombre nuevo'); await user.click(screen.getByRole('button', { name: 'Guardar nombre' }));
 expect(await screen.findByText('Acción completada.')).toBeVisible(); expect(invalidate).toHaveBeenCalledWith({ queryKey: ['organizations'] });
});
it.each([true, false])('renombrar red depende del permiso de edición: %s', canEdit => { mockGet(); mount(<NetworkView settings canEdit={canEdit}/>); expect(screen.queryByRole('button', { name: 'Renombrar red' }) !== null).toBe(canEdit); });
it.each([
 ['conflict', 'Ya existe una red con ese nombre en la organización.'],
 ['invalid_network_name_for_code', 'El nombre no cabe en el código de unión. Prueba un nombre más corto.'],
 ['invalid_public_url_for_code', 'El nombre no cabe en el código de unión con la URL pública actual. Prueba un nombre más corto.'],
])('explica conflicto de renombrado %s', async (code, message) => {
 mockGet(); vi.spyOn(api, 'PATCH').mockRejectedValue(new ApiError(409, { error: { code, message: code } })); mount(<NetworkView settings/>); const user = userEvent.setup();
 await user.click(screen.getByRole('button', { name: 'Renombrar red' })); await user.click(screen.getByRole('button', { name: 'Guardar nombre' })); await waitFor(() => expect(screen.getByText(message)).toBeVisible());
});
it('renombra red y refresca detalle y selectores', async () => {
 mockGet(); const patch = vi.spyOn(api, 'PATCH').mockResolvedValue({ data: network, response: new Response() }); const refresh = vi.fn(async () => {});
 const { cache } = mount(<NetworkView settings refresh={refresh}/>); const invalidate = vi.spyOn(cache, 'invalidateQueries'); const user = userEvent.setup();
 await user.click(screen.getByRole('button', { name: 'Renombrar red' })); await user.click(screen.getByRole('button', { name: 'Guardar nombre' }));
 await waitFor(() => expect(refresh).toHaveBeenCalledOnce()); expect(patch).toHaveBeenCalled(); expect(invalidate).toHaveBeenCalledWith({ queryKey: ['org', org.id, 'networks'] });
});
it.each([-60, -30, 0, 30, 60])('muestra servidor y desfase de %s segundos', async offset => {
 vi.spyOn(api, 'GET').mockResolvedValue({ data: { control_plane: { version: '0.1.5', commit: 'abcdef1234567890' }, server_time: new Date(Date.now() + offset * 1000).toISOString() }, response: new Response() });
 mount(<ServerInfo/>); expect(await screen.findByText(/0.1.5/)).toBeVisible(); expect(screen.getByText(/abcdef12/)).toBeVisible(); expect(screen.getByText(/Las versiones de coordinator y relay no están disponibles desde el panel/)).toBeVisible();
 expect(screen.queryByText(/los leases dependen del reloj/) !== null).toBe(Math.abs(offset) > 30);
});
it('404 del servidor es silencioso', async () => { vi.spyOn(api, 'GET').mockRejectedValue(new ApiError(404, { error: { code: 'not_found', message: 'no' } })); mount(<ServerInfo/>); expect(await screen.findByText('Este servidor aún no informa de su versión')).toBeVisible(); expect(screen.queryByRole('alert')).not.toBeInTheDocument(); });
it.each([['org.rename', 'Organización renombrada', { previous_name: 'Antes', name: 'Después' }], ['network.rename', 'Red renombrada', { previous_name: 'Antes', name: 'Después' }], ['member.set', 'Membresía actualizada', { previous_role: 'member', role: 'admin' }]])('traduce auditoría %s y su cambio', async (action, title, detail) => {
 vi.spyOn(api, 'GET').mockResolvedValue({ data: [{ id: 1, at: '2026-01-01T00:00:00Z', actor_kind: 'user', actor_id: member.user_id, target: org.id, action, detail }], response: new Response() }); mount(<Audit net={network.id}/>);
 expect(await screen.findByText(String(title))).toBeVisible(); expect(screen.getByText(action === 'member.set' ? 'member → admin' : 'Antes → Después')).toBeVisible();
});

it('usa el UUID corto si no hay nombre y no inventa un correo ausente', async () => {
 vi.spyOn(api, 'GET').mockImplementation((async (path: string) => ({
   data: path.endsWith('/members') ? [{ ...member, display_name: null, email: null }] : [], response: new Response(),
 })) as never);
 mount(<Organizations org={org} choose={() => {}}/>);
 await userEvent.click(screen.getByRole('tab', { name: 'Miembros' }));
 expect(await screen.findByRole('heading', { name: `${member.user_id.slice(0, 8)}…` })).toBeVisible();
 expect(screen.queryByText(member.email)).not.toBeInTheDocument();
});

it('valida el nombre de red antes de enviar y el diálogo es accesible', async () => {
 mockGet(); const patch = vi.spyOn(api, 'PATCH');
 mount(<NetworkView settings/>); const user = userEvent.setup();
 await user.click(screen.getByRole('button', { name: 'Renombrar red' }));
 const input = screen.getByLabelText('Nuevo nombre');
 await user.clear(input); await user.type(input, ' '.repeat(3));
 await user.click(screen.getByRole('button', { name: 'Guardar nombre' }));
 await waitFor(() => expect(screen.getByText('Indica un nombre.')).toBeVisible());
 expect(patch).not.toHaveBeenCalled();
 expect((await axe(screen.getByRole('dialog'), { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it.each([2, 0, -2])('mantiene el aviso de certificado urgente o caducado a %s días', days => {
 mockGet(); mount(<NetworkView expires={new Date(Date.now() + days * 86400000).toISOString()}/>);
 expect(screen.getByRole('note', { name: 'Caducidad del certificado' })).toHaveClass('badge-bad');
});

it('conserva la alternativa UUID plegada y pide confirmar antes del PUT', async () => {
 mockGet(); const put = vi.spyOn(api, 'PUT').mockResolvedValue({ response: new Response() });
 mount(<Organizations org={org} choose={() => {}}/>); const user = userEvent.setup();
 await user.click(screen.getByRole('tab', { name: 'Miembros' }));
 expect(screen.getByText('Alternativa: añadir o cambiar por UUID').parentElement).not.toHaveAttribute('open');
 await user.click(screen.getByText('Alternativa: añadir o cambiar por UUID'));
 await user.type(screen.getByLabelText('UUID del usuario'), member.user_id);
 const form = screen.getByLabelText('UUID del usuario').closest('form')!;
 await user.selectOptions(within(form).getByLabelText('Rol'), 'owner');
 await user.click(screen.getByRole('button', { name: 'Revisar cambio por UUID' }));
 expect(put).not.toHaveBeenCalled();
 await user.click(await screen.findByRole('button', { name: 'Confirmar operación por UUID' }));
 await user.click(screen.getByRole('button', { name: 'Confirmar asignación' }));
 await waitFor(() => expect(put).toHaveBeenCalledWith('/api/v1/orgs/{org}/members/{user}', {
   params: { path: { org: org.id, user: member.user_id } }, body: { role: 'owner' },
 }));
});

it('la consulta periódica de versión no genera anuncios de cambios', () => {
 const { cache, container } = mount(<QueryAnnouncements/>);
 act(() => { cache.setQueryData(['server'], { server_time: '2026-01-01T00:00:00Z' }); });
 act(() => { cache.setQueryData(['server'], { server_time: '2026-01-01T00:05:00Z' }); });
 expect(container.querySelector('[aria-live]')).toHaveTextContent('');
});
