// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { expect, it } from 'vitest';
import { groupServers, serverHost } from './servers';
import { network } from './networks';

it('normaliza host exacto, esquema, mayúsculas y puertos estándar sin confundir subdominios', () => {
  expect(serverHost('HTTPS://PANEL.EXAMPLE.TEST:443/api')).toBe('panel.example.test');
  expect(serverHost('http://PANEL.EXAMPLE.TEST:80/api')).toBe('panel.example.test');
  expect(serverHost('panel.example.test/path')).toBe('panel.example.test');
  expect(serverHost('https://panel.example.test:8443/api')).toBe('panel.example.test:8443');
  expect(serverHost('https://otro.example.test')).toBe('otro.example.test');
  expect(serverHost('https://[2001:db8::1]:443/api')).toBe('[2001:db8::1]');
});

it('agrupa por url y conserva los estados de red; el peor estado agregado no convierte detenida en rojo', () => {
  const net = (url: string, state: string) => network.parse({ network_id: url + state, name: state, url, server: 'nombre visible', interface: 'atenea0', listen_port: 51820, join_status: 'aprobada', state });
  const groups = groupServers([net('https://PANEL.EXAMPLE.TEST:443/api', 'conectada'), net('https://panel.example.test', 'detenida'), net('https://otro.example.test', 'conflicto')]);
  expect(groups.map((s) => [s.host, s.networks.length, s.state])).toEqual([['panel.example.test', 2, 'detenida'], ['otro.example.test', 1, 'conflicto']]);
  for (const states of [['conectada', 'detenida'], ['detenida', 'iniciada_sin_configuracion'], ['iniciada_sin_configuracion', 'desconocida'], ['desconocida', 'conflicto']]) {
    expect(groupServers(states.map((s) => net('https://panel.example.test', s)))[0].state).toBe(states[1]);
  }
});
