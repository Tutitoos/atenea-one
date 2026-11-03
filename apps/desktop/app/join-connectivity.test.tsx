// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-018: la confirmación de unión enseña lo que anuncia el código (STUN y CA del relay), y sigue funcionando
// con un CLI anterior que no devuelve ese campo.
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import Networks from './routes/networks';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const NET_ID = '0199ab12-3456-7def-8abc-0123456789ab';
const base = { name: 'casa', server: 'control.example.test', network_id: NET_ID, root_fingerprint: '6c4b-fd0b-2585-9aae', has_token: false };
const HASH = 'cd'.repeat(32);

async function review(info: object) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === 'network_list') return '[]' as never;
    if (cmd === 'network_legacy') return '{"present":false}' as never;
    if (cmd === 'autostart_get') return null as never;
    if (cmd === 'network_inspect') return JSON.stringify(info) as never;
    throw new Error(`orden inesperada: ${cmd}`);
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <MemoryRouter><Networks /></MemoryRouter>
    </QueryClientProvider>,
  );
  await userEvent.type(await screen.findByRole('textbox', { name: 'Código de red' }), 'ATENEA1-ABCDEF-GH23');
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  return within(await screen.findByRole('group', { name: 'Confirmar unión' }));
}

beforeEach(() => { vi.mocked(invoke).mockReset(); });

it.each([
  ['con CA', { stun_host: 'relay.example.test', relay_ca_sha256: HASH }, /relay\.example\.test.*cdcdcdcdcdcdcdcd/],
  ['sin hash', { stun_host: 'relay.example.test', relay_ca_sha256: null }, /relay\.example\.test.*pública/],
])('la confirmación enseña la conectividad del código (%s)', async (_, connectivity, text) => {
  const confirm = await review({ ...base, connectivity });
  expect(confirm.getByText(text)).toBeInTheDocument();
});

it.each([
  ['sin campo (CLI 0.1.8)', {}],
  ['null', { connectivity: null }],
])('sin conectividad en el código no se enseña ninguna fila y la unión sigue disponible (%s)', async (_, extra) => {
  const confirm = await review({ ...base, ...extra });
  expect(confirm.queryByText('Conectividad')).not.toBeInTheDocument();
  expect(confirm.getByRole('button', { name: 'Unirme a esta red' })).toBeEnabled();
});
