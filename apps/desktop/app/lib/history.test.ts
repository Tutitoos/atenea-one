// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { renderHook, act } from '@testing-library/react';
import { beforeEach, expect, it } from 'vitest';
import { eventText, recordHistory, useDeviceEvents } from './history';
import { recordEvents, resetHistoryStore } from './history-store';

const peers = (path = 'direct') => [{ public_key: 'sintetico', path }];
const texts = (h: ReturnType<typeof recordHistory>[]) => h.flatMap((x) => x.transitions.map((t) => eventText(t.to)));
beforeEach(() => { localStorage.clear(); resetHistoryStore(); });

it('guarda tasas sin inventar la primera y detecta cambios sin duplicados', () => {
  const h0 = recordHistory(undefined, 'red-a', peers(), {}, 0);
  expect(h0.devices.sintetico.points).toEqual([]);
  expect(h0.transitions).toEqual([]);
  const h1 = recordHistory(h0, 'red-a', peers('relay-udp'), { sintetico: { down: 2, up: 1 } }, 2000);
  expect(h1.transitions).toEqual([{ key: 'sintetico', at: 2000, from: 'direct', to: 'relay' }]);
  const h2 = recordHistory(h1, 'red-a', peers('relay-tcp'), {}, 4000);
  expect(h2.transitions).toEqual([]);
  const h3 = recordHistory(h2, 'red-a', peers('none'), {}, 6000);
  const h4 = recordHistory(h3, 'red-a', peers(), {}, 8000);
  expect(texts([h1, h3, h4])).toEqual(['Pasó a relay', 'Quedó Inactivo', 'Se conectó directamente']);
  expect(h4.devices.sintetico.points).toEqual([{ at: 2000, down: 2, up: 1 }]);
});
it('ver un par por primera vez no es una transición, ni al arrancar ni cuando aparece después', () => {
  const first = recordHistory(undefined, 'red-a', peers('none'), {}, 0);
  expect(first.transitions).toEqual([]);
  const later = recordHistory(first, 'red-a', [...peers('none'), { public_key: 'nuevo', path: 'direct' }], {}, 2000);
  expect(later.transitions).toEqual([]);
});
it('acota las muestras y reinicia al cambiar de red', () => {
  let h = recordHistory(undefined, 'red-a', peers(), {}, 0);
  for (let i = 1; i <= 400; i++) h = recordHistory(h, 'red-a', peers(), { sintetico: { down: i, up: 0 } }, i * 2000);
  expect(h.devices.sintetico.points).toHaveLength(300);
  expect(recordHistory(h, 'red-b', peers('none'), {}, 800001).transitions).toEqual([]);
});
it('registra desaparición y reconexión una sola vez', () => {
  const steps = [peers(), [], [], peers()].map((p, i) => ({ p, now: i * 2000 }));
  let h: ReturnType<typeof recordHistory> | undefined;
  const all = steps.map(({ p, now }) => (h = recordHistory(h, 'red-a', p, {}, now)));
  expect(texts(all)).toEqual(['Se desconectó', 'Se conectó directamente']);
});
it('useDeviceEvents lee del almacén y se actualiza al registrarse un evento', () => {
  const { result } = renderHook(() => useDeviceEvents('red-a', 'sintetico'));
  expect(result.current).toEqual([]);
  act(() => recordEvents('red-a', [{ key: 'sintetico', at: Date.now(), from: 'direct', to: 'none' }]));
  expect(result.current.map((e) => eventText(e.to))).toEqual(['Quedó Inactivo']);
  expect(renderHook(() => useDeviceEvents('red-b', 'sintetico')).result.current).toEqual([]);
});
