// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-025, fase 6: versiones instaladas en el detalle del dispositivo del panel.
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { axe } from 'vitest-axe';
import { expect, it } from 'vitest';
import type { Model } from '@atenea/api-client';
import { device } from './device.fixture';
import { Devices } from './devices';

const COMMIT = 'dfe72aef0123456789abcdef0123456789abcdef';

async function open(overrides: Partial<Model<'Device'>>) {
  const view = render(<MemoryRouter><Devices devices={[{ ...device, ...overrides }]} accessRole="member" net="red" elapsed={0} refresh={async () => {}} /></MemoryRouter>);
  await userEvent.setup().click(screen.getByRole('button', { name: `Ver detalle de ${device.name}` }));
  return view;
}
const value = (label: string) => screen.getByText(label).nextElementSibling?.textContent;

it('muestra la versión con el commit corto, «No instalada» para lo que falta y la fecha del informe, y axe sale limpio', async () => {
  const { container } = await open({ versions: [{ component: 'agent', version: '0.1.31', commit: COMMIT }, { component: 'cli', version: '0.1.31', commit: 'unknown' }], versions_at: '2026-11-16T10:00:00Z' });
  expect(value('Versión del agente')).toBe('0.1.31 (dfe72ae)');
  expect(value('Versión de la CLI')).toBe('0.1.31');
  expect(value('Versión de la aplicación')).toBe('No instalada');
  expect(screen.getByText(/no verificadas independientemente/)).toBeVisible();
  expect((await axe(container, { rules: { 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});

it('un dispositivo que nunca informó es «Sin informar», no «No instalada»', async () => {
  await open({ versions: [], versions_at: null });
  for (const label of ['Versión del agente', 'Versión de la CLI', 'Versión de la aplicación']) expect(value(label)).toBe('Sin informar');
  expect(screen.queryByText(/no verificadas independientemente/)).not.toBeInTheDocument();
});

it('informar la lista vacía con fecha significa que no hay nada instalado', async () => {
  await open({ versions: [], versions_at: '2026-11-16T10:00:00Z' });
  expect(value('Versión del agente')).toBe('No instalada');
});
