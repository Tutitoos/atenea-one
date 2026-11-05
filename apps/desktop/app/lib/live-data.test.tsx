// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from './bridge';
import { readAgent } from './agent';
import { useAgent } from './live';
import { Join } from '../join';
import { CopyButton } from '../ui';
import Shell from '../routes/shell';

vi.mock('./bridge', () => ({ invoke: vi.fn() }));

const NET_A = '0199ab12-3456-7def-8abc-0123456789ab';
const NET_B = '0199ab12-3456-7def-8abc-0123456789cd';
const KEY = 'a'.repeat(64);
const statusFor = (hostname: string, rx = 0) => JSON.stringify({
  status: { public_key: 'c'.repeat(64), hostname, config: { version: 1, lease_active: true }, peers: [{ public_key: KEY, path: 'direct', rx_bytes: rx, tx_bytes: 0 }] },
  health: { level: 'ok', text: 'Conectado', reasons: [] },
});
const net = (id: string, name: string) => ({ network_id: id, name, server: 'panel.example.org', url: 'https://panel.example.org', interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada' });
const deferred = <T,>() => { let resolve!: (v: T) => void; const promise = new Promise<T>((r) => { resolve = r; }); return { promise, resolve }; };

beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });
afterEach(() => { vi.restoreAllMocks(); });

it('el historial es por red: una lectura de otra red no lo pisa ni lo reinicia', async () => {
  let now = 1_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  const read = async (network: string, rx: number) => { vi.mocked(invoke).mockResolvedValue(statusFor('equipo-a', rx) as never); return readAgent(network); };
  await read('red-historial-a', 0);
  now += 2000;
  await read('red-historial-a', 2000);
  now += 2000;
  await read('red-historial-b', 5);
  now += 2000;
  const back = await read('red-historial-a', 6000);
  expect(back.history?.network).toBe('red-historial-a');
  // Dos lecturas con tasa en A; la de B en medio no las borró.
  expect(back.history?.devices[KEY]?.points).toHaveLength(2);
});

it('al volver a una red tras minutos no se dibuja una tasa media falsa', async () => {
  let now = 5_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  vi.mocked(invoke).mockResolvedValue(statusFor('equipo-a', 0) as never);
  await readAgent('red-vuelta');
  now += 5 * 60_000;
  vi.mocked(invoke).mockResolvedValue(statusFor('equipo-a', 9_000_000) as never);
  expect((await readAgent('red-vuelta')).rates).toEqual({});
});

function Probe() {
  const { sample, stale, network, selectNetwork } = useAgent();
  return (
    <div>
      <p data-testid="estado">{`${network === NET_B ? 'B' : 'A'}|${sample?.reply.status.hostname ?? 'sin datos'}|${stale ? 'obsoleto' : 'actual'}`}</p>
      <button type="button" onClick={() => selectNetwork(NET_B)}>ir a B</button>
    </div>
  );
}

it('al cambiar de red se conservan los datos previos marcados como obsoletos y luego se sustituyen', async () => {
  const slow = deferred<string>();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === 'network_list') return JSON.stringify([net(NET_A, 'casa'), net(NET_B, 'trabajo')]) as never;
    return ((args as { network: string }).network === NET_B ? await slow.promise : statusFor('equipo-a')) as never;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/x']}>
        <Routes><Route element={<Shell />}><Route path="x" element={<Probe />} /></Route></Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  await waitFor(() => expect(screen.getByTestId('estado').textContent).toBe('A|equipo-a|actual'));
  await userEvent.click(screen.getByRole('button', { name: 'ir a B' }));
  // La red es la nueva, los datos son los viejos y así se declara; no hay parpadeo a «sin datos».
  await waitFor(() => expect(screen.getByTestId('estado').textContent).toBe('B|equipo-a|obsoleto'));
  await act(async () => { slow.resolve(statusFor('equipo-b')); });
  await waitFor(() => expect(screen.getByTestId('estado').textContent).toBe('B|equipo-b|actual'));
});

const info = { name: 'casa', server: 'panel.example.org', network_id: NET_A, root_fingerprint: '6c4b-fd0b-2585-9aae', has_token: false };

it('join: el código editado mientras se inspecciona no muestra la huella del viejo ni se usa para unir', async () => {
  const first = deferred<string>();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === 'network_inspect') return ((args as { code: string }).code === 'ATENEA1-VIEJO' ? await first.promise : JSON.stringify({ ...info, name: 'nueva' })) as never;
    return '' as never;
  });
  const client = new QueryClient();
  render(<QueryClientProvider client={client}><Join onDone={() => {}} /></QueryClientProvider>);
  const box = screen.getByLabelText('Código de red');
  await userEvent.type(box, 'ATENEA1-VIEJO');
  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  await userEvent.clear(box);
  await userEvent.type(box, 'ATENEA1-NUEVO');
  await act(async () => { first.resolve(JSON.stringify(info)); });
  expect(screen.queryByRole('group', { name: 'Confirmar unión' })).not.toBeInTheDocument();

  await userEvent.click(screen.getByRole('button', { name: 'Revisar código' }));
  await userEvent.click(await screen.findByRole('button', { name: 'Unirme a esta red' }));
  await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith('network_apply', { op: 'join', code: 'ATENEA1-NUEVO' }));
});

it('CopyButton avisa si el portapapeles falla o no existe', async () => {
  render(<CopyButton value="x" label="huella" />);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: () => Promise.reject(new Error('denegado')) } });
  await userEvent.click(screen.getByRole('button', { name: 'Copiar huella' }));
  expect(await screen.findByText('No se pudo copiar')).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Copiar huella' })).toHaveAttribute('title', 'No se pudo copiar');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined });
  await userEvent.click(screen.getByRole('button', { name: 'Copiar huella' }));
  expect(await screen.findByRole('status')).toHaveTextContent('No se pudo copiar');
});
