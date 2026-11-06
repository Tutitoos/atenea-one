// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Outlet, Route, Routes } from 'react-router';
import { axe } from 'vitest-axe';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './lib/bridge';
import type { Agent } from './lib/live';
import { reply } from './lib/status';
import Diagnostics from './routes/diagnostics';
import { ToastProvider } from './toast';

vi.mock('./lib/bridge', () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.mocked(invoke).mockReset(); vi.mocked(invoke).mockImplementation(async (cmd: string) => (cmd === 'notifications_get' ? true : null) as never); });

const sample = (): Agent['sample'] => ({
  reply: reply.parse({ status: { public_key: 'c'.repeat(64), addrs: ['10.99.0.2'], hostname: 'equipo-a', config: { version: 1, lease_active: true }, counters: { path_lost: 2 }, peers: [] }, health: { level: 'ok', text: 'Conectado', reasons: [] } }),
  rates: {},
});
function view(agent: Partial<Agent>) {
  const full: Agent = { sample: sample(), stale: false, network: 'red-sintetica', error: null, empty: false, selectNetwork: () => undefined, ...agent };
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ToastProvider>
        <MemoryRouter initialEntries={['/diagnostics']}>
          <Routes><Route element={<Outlet context={full} />}><Route path="diagnostics" element={<Diagnostics />} /></Route></Routes>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

it('copia el informe anonimizado y avisa', async () => {
  const user = userEvent.setup();
  const write = vi.spyOn(navigator.clipboard, 'writeText');
  const { container } = view({});
  expect(screen.getByText(/Va anonimizado/)).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Copiar informe de diagnóstico' }));
  expect(write).toHaveBeenCalledOnce();
  const text = write.mock.calls[0][0];
  expect(text).toContain('Informe de diagnóstico de Atenea One');
  expect(text).toContain('path_lost=2');
  for (const secret of ['10.99.0.2', 'equipo-a', 'red-sintetica', 'c'.repeat(64)]) expect(text).not.toContain(secret);
  expect(await screen.findByText('Copiado: informe de diagnóstico')).toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});

it('con datos de otra red (stale) etiqueta «Actualizando…» y no ofrece copiar', async () => {
  const { container } = view({ stale: true });
  expect(screen.getByText('Actualizando…')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Copiar informe de diagnóstico' })).not.toBeInTheDocument();
  expect((await axe(container)).violations).toEqual([]);
});
