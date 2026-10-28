// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router';
import { axe } from 'vitest-axe';
import { afterEach, expect, it, vi } from 'vitest';
import { api } from '@atenea/api-client';
import { Field } from './ui';
import { Organizations } from './organizations';
import Panel from './panel';
import { device } from './device.fixture';
import { readFileSync } from 'node:fs';

const org = { id: '00000000-0000-4000-8000-000000000001', name: 'Organización sintética', role: 'owner' as const };
const network = { id: '00000000-0000-4000-8000-000000000002', org_id: org.id, name: 'Red sintética', ipv4_cidr: '10.0.0.0/24', ipv6_cidr: 'fd00::/64', has_issuer_cert: true, join_policy: 'cerrada' };
function wrap(node: React.ReactNode, entry = '/') {
  return render(<MemoryRouter initialEntries={[entry]}><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>{node}</QueryClientProvider></MemoryRouter>);
}
function mockData(pending = true, partialError = false, networkList = true, policyCount = 0) {
  vi.spyOn(api, 'GET').mockImplementation((async (path: string) => {
    if (partialError && path.endsWith('/policies')) throw new Error('Políticas no disponibles');
    if (path === '/api/v1/session') return { data: { user: { display_name: 'Prueba' }, memberships: [{ org_id: org.id, role: 'owner' }] } };
    if (path === '/api/v1/orgs') return { data: [org] };
    if (path === '/api/v1/networks/{net}') return { data: network };
    if (path.endsWith('/networks')) return { data: networkList ? [network] : [] };
    if (path.endsWith('/devices')) return { data: pending ? [device] : [] };
    if (path.endsWith('/members')) return { data: [{ user_id: device.id, role: 'owner' }] };
    if (path.endsWith('/policies')) return { data: Array.from({ length: policyCount }, (_, id) => ({ id })) };
    return { data: [] };
  }) as never);
}
afterEach(() => vi.restoreAllMocks());
it('asocia ayuda y error al control de Field', () => {
  render(<Field label="Nombre" help="Nombre visible" error="Indica un nombre">{props => <input {...props}/>}</Field>);
  const input = screen.getByRole('textbox', { name: 'Nombre' });
  expect(input).toHaveAccessibleDescription('Nombre visible Indica un nombre');
  expect(input).toHaveAttribute('aria-invalid', 'true');
});
it('muestra migas, selector de red e insignia de pendientes', async () => {
  mockData();
  const { container } = wrap(<Panel/>, `/?org=${org.id}&net=${network.id}`);
  await screen.findByRole('navigation', { name: 'Migas de pan' });
  expect(screen.getByRole('combobox', { name: 'Red' })).toHaveValue(network.id);
  expect(await screen.findByRole('link', { name: /Dispositivos.*1 solicitud pendiente/ })).toBeVisible();
  expect(screen.getByRole('navigation', { name: 'Migas de pan' })).toHaveTextContent('Resumen');
  expect((await axe(container, { rules: { 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
it.each([true, false])('resumen con solicitudes pendientes: %s', async pending => {
  mockData(pending);
  wrap(<Panel/>, `/?org=${org.id}&net=${network.id}`);
  await screen.findByRole('heading', { name: 'Solicitudes pendientes' });
  if (pending) expect(await screen.findByRole('link', { name: 'Revisar pendientes' })).toHaveAttribute('href', expect.stringContaining('status=pending'));
  else expect(await screen.findByText('No hay solicitudes pendientes.')).toBeVisible();
});
it('conserva las tarjetas disponibles cuando falla una consulta', async () => {
  mockData(true, true);
  wrap(<Panel/>, `/?org=${org.id}&net=${network.id}`);
  expect(await screen.findByText(/Políticas no disponibles/)).toBeVisible();
  expect(screen.getByText('Certificado instalado')).toBeVisible();
});
it('las redes son enlaces y los miembros son visibles para member', async () => {
  mockData();
  const user = userEvent.setup();
  const { container } = wrap(<Organizations org={{ ...org, role: 'member' }} choose={() => {}}/>);
  expect(await screen.findByRole('link', { name: network.name })).toHaveAttribute('href', expect.stringContaining(network.id));
  await user.click(screen.getByRole('tab', { name: 'Miembros' }));
  expect(await screen.findByText('owner')).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Añadir o cambiar rol' })).not.toBeInTheDocument();
  await user.click(screen.getByRole('tab', { name: 'Ajustes' }));
  expect(screen.queryByRole('button', { name: 'Borrar organización' })).not.toBeInTheDocument();
  expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
it('exige el nombre antes de borrar la organización', async () => {
  mockData();
  const remove = vi.spyOn(api, 'DELETE').mockResolvedValue({ response: new Response() } as never);
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={() => {}}/>);
  await user.click(screen.getByRole('tab', { name: 'Ajustes' }));
  await user.click(screen.getByRole('button', { name: 'Borrar organización' }));
  await user.type(screen.getByRole('textbox', { name: /Escribe/ }), 'incorrecto');
  await user.click(screen.getByRole('button', { name: 'Confirmar borrado' }));
  expect(remove).not.toHaveBeenCalled();
  await user.clear(screen.getByRole('textbox', { name: /Escribe/ }));
  await user.type(screen.getByRole('textbox', { name: /Escribe/ }), org.name);
  await user.click(screen.getByRole('button', { name: 'Confirmar borrado' }));
  await waitFor(() => expect(remove).toHaveBeenCalledWith('/api/v1/orgs/{org}', { params: { path: { org: org.id } } }));
  expect(await screen.findByRole('heading', { name: 'Organizaciones' })).toBeVisible();
});
it('el selector de red abre el resumen y permite volver a la organización', async () => {
  mockData();
  const user = userEvent.setup();
  wrap(<Panel/>, `/?org=${org.id}`);
  await screen.findByRole('link', { name: network.name });
  await user.selectOptions(screen.getByRole('combobox', { name: 'Red' }), network.id);
  expect(await screen.findByRole('heading', { name: `${network.name} · Resumen` })).toBeVisible();
  await user.selectOptions(screen.getByRole('combobox', { name: 'Red' }), '');
  expect(await screen.findByRole('heading', { name: org.name })).toBeVisible();
});
it('impide retirar al último owner y mantiene la confirmación accesible', async () => {
  mockData();
  const remove = vi.spyOn(api, 'DELETE');
  const user = userEvent.setup();
  wrap(<Organizations org={org} choose={() => {}}/>);
  await user.click(screen.getByRole('tab', { name: 'Miembros' }));
  await user.click(await screen.findByRole('button', { name: 'Retirar miembro' }));
  const dialog = screen.getByRole('dialog', { name: 'Retirar miembro' });
  expect((await axe(dialog, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
  await user.click(screen.getByRole('button', { name: 'Confirmar retirada' }));
  expect(await screen.findByText('Siempre debe quedar al menos un owner.')).toBeVisible();
  expect(remove).not.toHaveBeenCalled();
});
it('la acción de pendientes abre únicamente las solicitudes pendientes', async () => {
  mockData();
  const user = userEvent.setup();
  wrap(<Panel/>, `/?org=${org.id}&net=${network.id}`);
  await user.click(await screen.findByRole('link', { name: 'Revisar pendientes' }));
  expect(await screen.findByRole('link', { name: 'Ver todos los dispositivos' })).toBeVisible();
  expect(screen.getByRole('button', { name: `Ver detalle de ${device.name}` })).toBeVisible();
});
it('usa el nombre de detalle cuando la red no aparece en la lista', async () => {
  mockData(true, false, false);
  wrap(<Panel/>, `/?org=${org.id}&net=${network.id}`);
  expect(await screen.findByRole('option', { name: network.name })).toHaveValue(network.id);
  expect(screen.queryByText('Red seleccionada')).not.toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Vista conjunta' })).toBeVisible();
  expect(screen.getByRole('link', { name: 'Auditoría' })).toBeVisible();
});
it.each([0, 1, 2])('concordancia de políticas con %s', async count => {
  mockData(true, false, true, count);
  wrap(<Panel/>, `/?org=${org.id}&net=${network.id}`);
  expect(await screen.findByText(`${count} ${count === 1 ? 'política' : 'políticas'}`)).toBeVisible();
  expect(await screen.findByText('1 solicitud requiere revisión.')).toBeVisible();
  expect(screen.getByText('0 relays observados por los agentes.')).toBeVisible();
});

it('una organización sin redes ofrece un único botón «Crear red», y con redes también', async () => {
  mockData(true, false, false);
  const empty = wrap(<Organizations org={org} choose={() => {}} />);
  await screen.findByText('No hay redes.');
  expect(screen.getAllByRole('button', { name: 'Crear red' })).toHaveLength(1);
  empty.unmount();

  vi.restoreAllMocks();
  mockData(true, false, true);
  wrap(<Organizations org={org} choose={() => {}} />);
  await screen.findByRole('link', { name: network.name });
  expect(screen.getAllByRole('button', { name: 'Crear red' })).toHaveLength(1);
});

it('actualiza las membresías al crear una organización para poder administrar su red inmediatamente', async () => {
  let created = false;
  vi.spyOn(api, 'POST').mockImplementation((async () => {
    created = true;
    return { data: org };
  }) as never);
  vi.spyOn(api, 'GET').mockImplementation((async (path: string) => {
    if (path === '/api/v1/session') return { data: {
      user: { display_name: 'Prueba' }, memberships: created ? [{ org_id: org.id, role: 'owner' }] : [],
    } };
    if (path === '/api/v1/orgs') return { data: created ? [org] : [] };
    if (path === '/api/v1/networks/{net}') return { data: { ...network, has_issuer_cert: false } };
    if (path.endsWith('/networks')) return { data: [network] };
    if (path.endsWith('/dns')) return { data: { servers: [], domains: [] } };
    return { data: [] };
  }) as never);
  const user = userEvent.setup();
  wrap(<Panel/>);
  await user.click(await screen.findByRole('button', { name: 'Crear organización' }));
  await user.type(screen.getByRole('textbox', { name: 'Nombre' }), org.name);
  await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Crear organización' }));
  await screen.findByText('Acción completada.');
  await user.keyboard('{Escape}');
  await user.selectOptions(screen.getByRole('combobox', { name: 'Organización' }), org.id);
  await user.click(await screen.findByRole('link', { name: network.name }));
  await user.click(screen.getByRole('link', { name: 'DNS y certificado' }));
  expect(await screen.findByRole('button', { name: 'Subir certificado' })).toBeVisible();
});

it('mantiene el contraste AA del texto secundario en los fondos claros del panel', () => {
  const styles = readFileSync('app/styles.css', 'utf8');
  const tokens = [...styles.matchAll(/:root \{([^}]+)\}/g)].at(-1)![1];
  const luminance = (token: string) => {
    const hex = new RegExp(`--${token}: #([0-9a-f]{6});`).exec(tokens)![1];
    const channels = hex.match(/../g)!.map(channel => {
      const value = parseInt(channel, 16) / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    });
    return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  };
  for (const background of ['bg', 'side', 'card']) {
    const ratio = (luminance(background) + 0.05) / (luminance('muted') + 0.05);
    expect(ratio, `Contraste de --muted sobre --${background}`).toBeGreaterThanOrEqual(4.5);
  }
});
