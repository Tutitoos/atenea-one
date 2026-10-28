// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { expect, it } from 'vitest';
import { recordHistory } from './history';

const peers = (path = 'direct') => [{ public_key: 'sintetico', path }];
it('guarda tasas sin inventar la primera y detecta cambios sin duplicados', () => {
  let h = recordHistory(undefined, 'red-a', peers(), {}, 0);
  expect(h.devices.sintetico.points).toEqual([]);
  expect(h.devices.sintetico.events).toEqual([]);
  h = recordHistory(h, 'red-a', peers('relay-udp'), { sintetico: { down: 2, up: 1 } }, 2000);
  expect(h.devices.sintetico.events.map((e) => e.text)).toEqual(['Pasó a relay']);
  h = recordHistory(h, 'red-a', peers('relay-tcp'), {}, 4000);
  expect(h.devices.sintetico.events).toHaveLength(1);
  h = recordHistory(h, 'red-a', peers('none'), {}, 6000);
  h = recordHistory(h, 'red-a', peers(), {}, 8000);
  expect(h.devices.sintetico.events.map((e) => e.text)).toEqual(['Pasó a relay', 'Quedó Inactivo', 'Se conectó directamente']);
  expect(h.devices.sintetico.points).toEqual([{ at: 2000, down: 2, up: 1 }]);
});
it('acota muestras y eventos y reinicia al cambiar de red', () => {
  let h = recordHistory(undefined, 'red-a', peers(), {}, 0);
  for (let i = 1; i <= 400; i++) h = recordHistory(h, 'red-a', peers(i % 2 ? 'none' : 'direct'), { sintetico: { down: i, up: 0 } }, i * 2000);
  expect(h.devices.sintetico.points).toHaveLength(300);
  expect(h.devices.sintetico.events).toHaveLength(20);
  expect(recordHistory(h, 'red-b', peers(), {}, 800001).devices.sintetico.events).toEqual([]);
});
it('registra desaparición y reconexión una sola vez', () => {
  let h = recordHistory(undefined, 'red-a', peers(), {}, 0);
  h = recordHistory(h, 'red-a', [], {}, 2000);
  h = recordHistory(h, 'red-a', [], {}, 4000);
  h = recordHistory(h, 'red-a', peers(), {}, 6000);
  expect(h.devices.sintetico.events.map((e) => e.text)).toEqual(['Se desconectó', 'Se conectó directamente']);
});
