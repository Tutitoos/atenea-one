// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { expect, it } from 'vitest';
import type { Network } from './networks';
import { installCommand, phaseOf, trayHint, type Joined } from './wizard';

const net = (over: Partial<Network> = {}): Network => ({
  network_id: '0199ab12-3456-7def-8abc-0123456789ab', name: 'casa', server: 'control.example.test', url: 'https://control.example.test',
  interface: 'atenea0', listen_port: 51821, join_status: 'aprobada', state: 'conectada', ...over,
});
const ok = { os: 'linux', supported: true, installed: true };
const joined = (over: Partial<Joined> = {}): Joined => ({ networkId: net().network_id, name: 'casa', server: 'control.example.test', hasToken: false, ...over });

it.each([
  ['sin saber nada todavía', {}, 'cargando'],
  ['fallo al comprobar', { client: ok, failed: true }, 'error'],
  ['sistema no soportado, aunque haya CLI y redes', { client: { os: 'windows', supported: false, installed: true }, networks: [net()] }, 'sistema'],
  ['cliente ausente', { client: { ...ok, installed: false } }, 'cliente'],
  ['cliente presente, aún sin la lista de redes', { client: ok }, 'cargando'],
  ['sin redes', { client: ok, networks: [] }, 'unirse'],
  ['sin redes y con instalación heredada', { client: ok, networks: [], legacy: true }, 'heredada'],
  ['heredada descartada por el usuario', { client: ok, networks: [], legacy: true, skippedLegacy: true }, 'unirse'],
  ['con redes ya unidas', { client: ok, networks: [net()] }, 'listo'],
  ['con redes y heredada: no se ofrece importar', { client: ok, networks: [net()], legacy: true }, 'listo'],
] as const)('fase: %s', (_, input, expected) => {
  expect(phaseOf(input as never)).toBe(expected);
});

it('una solicitud sin token espera la aprobación hasta que la red deja de estar pendiente', () => {
  const base = { client: ok, joined: joined() };
  expect(phaseOf({ ...base, networks: [] })).toBe('aprobacion'); // la lista aún no refleja la red
  expect(phaseOf({ ...base, networks: [net({ state: 'iniciada_sin_configuracion' })] })).toBe('aprobacion');
  expect(phaseOf({ ...base, networks: [net({ state: 'conectada' })] })).toBe('listo');
  expect(phaseOf({ ...base, networks: [net({ state: 'detenida' })] })).toBe('listo');
  expect(phaseOf({ ...base, networks: [net({ state: 'iniciada_sin_configuracion' })], skippedApproval: true })).toBe('listo');
});

it('un código con token entra aprobado y no espera', () => {
  expect(phaseOf({ client: ok, joined: joined({ hasToken: true }), networks: [net({ state: 'iniciada_sin_configuracion' })] })).toBe('listo');
});

it('la unión recién hecha se busca por el id de la red, no por el nombre', () => {
  const other = net({ network_id: '0199ab12-3456-7def-8abc-0123456789cd', name: 'casa', state: 'conectada' });
  expect(phaseOf({ client: ok, joined: joined(), networks: [other] })).toBe('aprobacion');
});

it('las órdenes de instalación son texto fijo por sistema y no hay para los no soportados', () => {
  expect(installCommand('darwin')).toBe('sudo installer -pkg atenea-one-*-macos-*.pkg -target /');
  expect(installCommand('linux')).toBe('sudo apt install ./atenea-cli-*-linux-*.deb ./atenea-desktop-*-linux-*.deb');
  expect(installCommand('windows')).toBeUndefined();
  expect(installCommand('freebsd')).toBeUndefined();
  expect(installCommand('')).toBeUndefined();
});

it('la pista de la bandeja distingue la barra de menús de macOS', () => {
  expect(trayHint('darwin')).toContain('barra de menús');
  expect(trayHint('linux')).toContain('bandeja');
});
