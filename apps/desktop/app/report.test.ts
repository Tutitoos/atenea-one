// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { expect, it } from 'vitest';
import { buildReport } from './lib/report';
import { reply } from './lib/status';

const KEY_SELF = 'c'.repeat(43) + '=';
const KEY_A = 'A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8S9t0U1v=';
const KEY_B = 'f'.repeat(64);
const DEVICE_ID = '0123abcd-4567-89ab-cdef-0123456789ab';
const NETWORK = 'red-interna-prueba';
const LITERALS = [
  KEY_SELF, KEY_A, KEY_B, DEVICE_ID, NETWORK, 'portatil-de-ana', 'MacBook de prueba', 'panel.example.org', 'control.example.org', 'relay-uno.example.org',
  '10.99.0.2', '10.99.0.3', '10.99.0.4', '198.51.100.7', '203.0.113.9', '2001:db8::7', 'fe80::1', 'aabbccdd11', 'sesion-secreta',
];
const synthetic = () => reply.parse({
  status: {
    server: { control_plane: { version: '1.2.3', commit: 'abc' }, coordinator: { version: '1.2.4', commit: 'abc' }, server_time_unix: 0, clock_skew_secs: 0, issuer_cert_not_after_unix: null, fetched_unix: 0, last_error: 'fallo hablando con 10.99.0.4:443' },
    public_key: KEY_SELF, addrs: ['10.99.0.2'], hostname: 'MacBook de prueba',
    config: { version: 7, lease_active: true, valid_until_unix: 1900000000, last_error: `rechazada por ${KEY_B}` },
    nat: { mapping: 'endpoint-independent', reflexive: '203.0.113.9:51820' },
    sync: { url: 'https://control.example.org/sync', last_error: 'dial tcp 198.51.100.7:443: timeout en https://control.example.org/sync' },
    coordinator: { url: 'https://panel.example.org', last_error: 'no resuelve panel.example.org ([2001:db8::7]) ni fe80::1' },
    relay: { allocations: 2, drops: {}, entries: [
      { peer: 'aabbccdd11', transport: 'Udp', state: 'ready', pings_sent: 540, pings_lost: 1, last_error: 'rechazo de relay-uno.example.org' },
      { peer: KEY_A.slice(0, 12), transport: 'Tcp', state: 'degraded', pings_sent: 10, pings_lost: 5 },
    ] },
    counters: { path_lost: 2, tx_init: 12345, relay_renewal_failed: 0 },
    peers: [
      { public_key: KEY_A, device_id: DEVICE_ID, name: 'portatil-de-ana', addrs: ['10.99.0.3'], endpoint: '198.51.100.7:51820', session: 'sesion-secreta', tx_bytes: 1, rx_bytes: 2, path: 'direct' },
      { public_key: 'aabbccdd11' + KEY_B, addrs: ['10.99.0.4'], endpoint: '[2001:db8::7]:51820', tx_bytes: 1, rx_bytes: 2, path: 'relay-udp', role: 'server' },
    ],
  },
  health: { level: 'degraded', text: 'Degradado: portatil-de-ana por relay', reasons: ['portatil-de-ana sin respuesta desde 10.99.0.3'] },
});

it('el informe no contiene ninguna IP, clave, nombre, host ni endpoint de los datos', () => {
  const out = buildReport({ reply: synthetic(), network: NETWORK });
  for (const literal of LITERALS) expect(out, literal).not.toContain(literal);
  expect(out).not.toMatch(/\b\d{1,3}(\.\d{1,3}){3}\b/);
  expect(out).not.toMatch(/https?:\/\//);
  expect(out).not.toMatch(/\.example\.org/);
});

it('conserva lo útil: estado, lease, NAT, pares por camino, contadores, relays y errores con etiquetas estables', () => {
  const out = buildReport({ reply: synthetic(), network: NETWORK });
  expect(out).toContain('Versiones: control plane 1.2.3 · coordinator 1.2.4');
  expect(out).toContain('Estado general: degradado');
  expect(out).toContain('Lease: activo · configuración v7');
  expect(out).toContain('NAT: endpoint-independent · dirección pública vista');
  expect(out).toContain('Pares (2): direct 1 · relay 1');
  expect(out).toMatch(/equipo-1: direct/);
  expect(out).toMatch(/equipo-2 \(servidor\): relay-udp/);
  expect(out).toContain('Caminos: path_lost=2');
  expect(out).not.toContain('relay_renewal_failed');
  expect(out).toMatch(/equipo-2 · Udp · ready · pings perdidos 0,2 % \(1\/540\)/);
  expect(out).toMatch(/equipo-1 · Tcp · degraded · pings perdidos 50,0 % \(5\/10\)/);
  // El mismo equipo recibe la misma etiqueta en el aviso, en la lista de pares y en los relays.
  expect(out).toContain('Avisos: equipo-1 sin respuesta');
  expect(out).toMatch(/Control plane: .*servidor-1.*/);
});

it('es estable: la misma entrada da el mismo informe', () => {
  expect(buildReport({ reply: synthetic(), network: NETWORK })).toBe(buildReport({ reply: synthetic(), network: NETWORK }));
});

it('sanea direcciones en un texto libre desconocido', () => {
  const r = synthetic();
  r.status.config.last_error = 'fallo con 192.0.2.55:8080, 2001:db8:abcd::1, https://extra.example.net/x y 00112233445566778899aabbccddeeff';
  const out = buildReport({ reply: r });
  for (const literal of ['192.0.2.55', '2001:db8:abcd', 'extra.example.net', '00112233445566778899aabbccddeeff']) expect(out).not.toContain(literal);
});
