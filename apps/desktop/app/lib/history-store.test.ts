// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushHistory, getEvents, MAX_AGE_MS, recordEvents, resetHistoryStore } from './history-store';

const KEY = 'atenea.history.v1';
const ev = (key: string, at: number, to = 'relay') => ({ key, at, from: 'direct', to });
beforeEach(() => { localStorage.clear(); resetHistoryStore(); vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

it('persiste con debounce y recupera tras reiniciar', () => {
  recordEvents('red-a', [ev('equipo-a', Date.now())]);
  recordEvents('red-a', [ev('equipo-a', Date.now() + 1, 'none')]);
  expect(localStorage.getItem(KEY)).toBeNull();
  vi.advanceTimersByTime(2000);
  expect(JSON.parse(localStorage.getItem(KEY) ?? '{}').v).toBe(1);
  resetHistoryStore();
  expect(getEvents('red-a', 'equipo-a').map((e) => e.to)).toEqual(['relay', 'none']);
  expect(getEvents('red-b', 'equipo-a')).toEqual([]);
});
it('retiene como máximo 200 eventos por red y 30 días', () => {
  const now = Date.now();
  recordEvents('red-a', Array.from({ length: 250 }, (_, i) => ev('equipo-a', now - 1000 + i)));
  recordEvents('red-b', [ev('equipo-a', now)]);
  const kept = getEvents('red-a', 'equipo-a');
  expect(kept).toHaveLength(200);
  expect(kept[0].at).toBe(now - 1000 + 50);
  expect(getEvents('red-b', 'equipo-a')).toHaveLength(1);
  recordEvents('red-c', [ev('equipo-a', now - MAX_AGE_MS - 1), ev('equipo-a', now - 1000)]);
  expect(getEvents('red-c', 'equipo-a')).toHaveLength(1);
});
it('JSON corrupto o versión desconocida empiezan vacíos sin lanzar', () => {
  for (const raw of ['{no es json', JSON.stringify({ v: 2, data: { a: { b: [{ at: Date.now(), from: 'x', to: 'y' }] } } }), JSON.stringify({ v: 1, data: { a: { b: 'mal' } } })]) {
    localStorage.setItem(KEY, raw);
    resetHistoryStore();
    expect(getEvents('a', 'b')).toEqual([]);
  }
});
it('una cuota llena no lanza y la sesión sigue viendo los eventos', () => {
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new DOMException('lleno', 'QuotaExceededError'); });
  expect(() => { recordEvents('red-a', [ev('equipo-a', Date.now())]); vi.advanceTimersByTime(2000); flushHistory(); }).not.toThrow();
  expect(getEvents('red-a', 'equipo-a')).toHaveLength(1);
});
it('el almacén solo guarda transiciones: red, par, instante y caminos', () => {
  const at = Date.now();
  recordEvents('red-a', [ev('equipo-a', at)]);
  vi.advanceTimersByTime(2000);
  expect(JSON.parse(localStorage.getItem(KEY) ?? '{}')).toEqual({ v: 1, data: { 'red-a': { 'equipo-a': [{ at, from: 'direct', to: 'relay' }] } } });
});
