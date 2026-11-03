// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router';
import { afterEach, expect, it, vi } from 'vitest';
import { api } from '@atenea/api-client';
import { Organizations } from './organizations';

const org = { id: '00000000-0000-4000-8000-000000000001', name: 'Organización sintética', role: 'owner' as const };
const secret = `atsvc_${'A'.repeat(43)}`;
const item = { id: '00000000-0000-4000-8000-000000000009', name: 'asistente de ejemplo', created_at: '2026-11-03T10:00:00Z',
  expires_at: '2099-01-01T00:00:00Z', last_used_at: null, revoked_at: null };
function mount(role: 'owner' | 'admin' | 'member') {
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<MemoryRouter><QueryClientProvider client={cache}><Organizations org={{ ...org, role }} choose={() => {}}/></QueryClientProvider></MemoryRouter>);
  return cache;
}
function mockGet(items: unknown[]) {
  vi.spyOn(api, 'GET').mockImplementation((async (path: string) => ({ data: path.endsWith('/service-tokens') ? items : [], response: new Response() })) as never);
}
afterEach(() => { vi.restoreAllMocks(); localStorage.clear(); sessionStorage.clear(); });

it('muestra el token una sola vez y no lo conserva en caché ni almacenamiento', async () => {
  mockGet([item]);
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ data: { id: item.id, token: secret, expires_at: item.expires_at }, response: new Response() } as never);
  const cache = mount('owner'); const user = userEvent.setup();
  await user.click(screen.getByRole('tab', { name: 'Tokens de IA' }));
  await user.type(await screen.findByLabelText('Nombre del token'), 'asistente de ejemplo');
  await user.click(screen.getByRole('button', { name: 'Crear token' }));
  expect(await screen.findByText(secret)).toBeVisible();
  expect(post).toHaveBeenCalledWith('/api/v1/orgs/{org}/service-tokens', { params: { path: { org: org.id } }, body: { name: 'asistente de ejemplo', ttl_secs: 30 * 86400 } });
  expect(JSON.stringify(cache.getQueryCache().getAll().map(query => query.state.data))).not.toContain(secret);
  expect(JSON.stringify({ ...localStorage })).not.toContain(secret);
  expect(JSON.stringify({ ...sessionStorage })).not.toContain(secret);
  await user.click(screen.getByRole('button', { name: 'Ocultar token' }));
  expect(screen.queryByText(secret)).not.toBeInTheDocument();
});

it('lista tokens y pide confirmar antes de revocar', async () => {
  mockGet([item]);
  const del = vi.spyOn(api, 'DELETE').mockResolvedValue({ response: new Response() } as never);
  mount('owner'); const user = userEvent.setup();
  await user.click(screen.getByRole('tab', { name: 'Tokens de IA' }));
  expect(await screen.findByText('Activo')).toBeVisible();
  await user.click(screen.getByRole('button', { name: `Revocar ${item.name}` }));
  expect(del).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar revocación' }));
  await screen.findByText('Acción completada.');
  expect(del).toHaveBeenCalledWith('/api/v1/orgs/{org}/service-tokens/{id}', { params: { path: { org: org.id, id: item.id } } });
});

it.each(['admin', 'member'] as const)('un %s no ve la sección', role => {
  mockGet([]); mount(role);
  expect(screen.queryByRole('tab', { name: 'Tokens de IA' })).not.toBeInTheDocument();
});
