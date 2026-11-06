// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { expect, it, vi } from 'vitest';
import { createSampler } from './agent';
import { reply } from './status';

// Como el resto de tests: sin esto se carga el runtime de Wails, cuyo sondeo de 50 ms (drag.js) puede disparar tras desmontar jsdom y lanzar «window is not defined».
vi.mock('./bridge', () => ({ invoke: vi.fn() }));

const peer = (rx: number, tx: number) => ({ public_key: 'aa', path: 'direct', rx_bytes: rx, tx_bytes: tx });
const make = (rx: number, tx: number, extra: object = {}) => reply.parse({
  status: { public_key: 'bb', config: { version: 1, lease_active: true }, peers: [peer(rx, tx)], ...extra },
  health: { level: 'ok', text: 'Conectado', reasons: [] },
});

it('calcula bytes por segundo entre lecturas y no inventa la primera tasa', () => {
  const sample = createSampler();
  expect(sample(make(1000, 0), 0).rates).toEqual({});
  expect(sample(make(3000, 500), 2000).rates).toEqual({ aa: { down: 1000, up: 250 } });
});

it('un contador que retrocede (agente reiniciado) da tasa 0 y no negativa', () => {
  const sample = createSampler();
  sample(make(5000, 5000), 0);
  expect(sample(make(10, 10), 1000).rates.aa).toEqual({ down: 0, up: 0 });
});

it('descarta la tasa si las lecturas están demasiado juntas o demasiado separadas y reanuda después', () => {
  const sample = createSampler();
  sample(make(0, 0), 0);
  expect(sample(make(500, 0), 300).rates).toEqual({});
  expect(sample(make(9000, 0), 60000).rates).toEqual({});
  expect(sample(make(11000, 0), 62000).rates).toEqual({ aa: { down: 1000, up: 0 } });
});

it('acepta un agente anterior sin direcciones y rechaza una respuesta con otra forma', () => {
  const old = make(0, 0);
  expect(old.status.addrs).toEqual([]);
  expect(old.status.peers[0]?.addrs).toEqual([]);
  expect(() => reply.parse({ status: { peers: 'no' }, health: {} })).toThrow();
});
