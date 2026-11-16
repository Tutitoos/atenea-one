// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-024, fase 6: servicios publicados en el detalle del dispositivo del panel.
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { axe } from 'vitest-axe';
import { expect, it } from 'vitest';
import type { Model } from '@atenea/api-client';
import { device } from './device.fixture';
import { Devices } from './devices';

async function open(overrides: Partial<Model<'Device'>>) {
  const view = render(<MemoryRouter><Devices devices={[{ ...device, ...overrides }]} accessRole="member" net="red" elapsed={0} refresh={async () => {}} /></MemoryRouter>);
  await userEvent.setup().click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  return view;
}
const published = [{ port: 8080, proto: 'tcp' as const, scheme: 'http' as const, label: 'Storybook' }, { port: 8443, proto: 'tcp' as const, scheme: 'https' as const }];

it('lista puerto, esquema y etiqueta de lo publicado, avisa de que no abre nada y axe sale limpio', async () => {
  const { container } = await open({ services: published, services_at: new Date().toISOString() });
  const list = within(screen.getByRole('list', { name: `Servicios publicados por ${device.name}` }));
  expect(list.getAllByRole('listitem').map(item => item.textContent)).toEqual(['Puerto 8080 · http · Storybook', 'Puerto 8443 · https']);
  expect(screen.getByText(/no lo hace alcanzable ni crea políticas/)).toBeVisible();
  expect(screen.queryByText(/ya no ven esta lista/)).not.toBeInTheDocument();
  expect((await axe(container, { rules: { 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('un informe de hace más de 120 s avisa de que los pares ya no lo ven, pero se sigue mostrando', async () => {
  await open({ services: published, services_at: '2026-01-01T00:00:00Z' });
  expect(screen.getByText(/ya no ven esta lista/)).toBeVisible();
  expect(within(screen.getByRole('list', { name: `Servicios publicados por ${device.name}` })).getAllByRole('listitem')).toHaveLength(2);
});

it('sin fecha de informe es «sin informar»; con fecha y lista vacía, «no publica»', async () => {
  const first = await open({ services: [], services_at: null });
  expect(screen.getByText(/Sin servicios informados/)).toBeVisible();
  expect(screen.queryByText('No publica servicios.')).not.toBeInTheDocument();
  first.unmount();
  await open({ services: [], services_at: new Date().toISOString() });
  expect(screen.getByText('No publica servicios.')).toBeVisible();
  expect(screen.queryByText(/Sin servicios informados/)).not.toBeInTheDocument();
});
