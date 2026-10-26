// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { expect, it } from 'vitest';
import { dnsDomain, dnsSchema, privatePrefix } from './domain';
it('solo acepta prefijos privados canónicos y acotados', () => {
  for (const prefix of ['10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16', '100.64.0.0/10', 'fc00::/32', 'fd77::/64']) expect(privatePrefix.safeParse(prefix).success, prefix).toBe(true);
  for (const prefix of ['0.0.0.0/0', '::/0', '10.0.0.0/7', '172.0.0.0/8', '100.0.0.0/8', '192.0.2.0/24', '127.0.0.0/8', '169.254.0.0/16', '224.0.0.0/8', '2001:db8::/32', 'fc00::/7', 'fd77::1/64', '192.168.1.1/24', 'no']) expect(privatePrefix.safeParse(prefix).success, prefix).toBe(false);
});
it('valida dominios LDH minúsculos, longitudes y exclusión de raíz', () => {
  for (const domain of ['lab.internal', 'a', 'a-b.example', `${'a'.repeat(63)}.internal`]) expect(dnsDomain.safeParse(domain).success, domain).toBe(true);
  for (const domain of ['', '.', 'Lab.internal', 'lab.internal.', 'a..b', '-lab.internal', 'lab-.internal', 'lab_internal', 'ñ.test', `${'a'.repeat(64)}.test`, Array(5).fill('a'.repeat(63)).join('.')]) expect(dnsDomain.safeParse(domain).success, domain).toBe(false);
});
it('limita DNS a tres IP y ocho dominios únicos sin decidir alcance localmente', () => {
  expect(dnsSchema.safeParse({ servers: ['10.77.0.1', 'fd77::1'], domains: ['lab.internal'] }).success).toBe(true);
  expect(dnsSchema.safeParse({ servers: [], domains: [] }).success).toBe(true);
  for (const servers of [['hostname'], ['10.77.0.1', '10.77.0.1'], ['fd77::1', 'fd77:0:0:0:0:0:0:1'], ['a:b:bad'], ['10.0.0.1', '10.0.0.2', '10.0.0.3', '10.0.0.4']]) expect(dnsSchema.safeParse({ servers, domains: [] }).success).toBe(false);
  for (const domains of [['lab.internal', 'lab.internal'], Array.from({ length: 9 }, (_, i) => `lab${i}.internal`)]) expect(dnsSchema.safeParse({ servers: [], domains }).success).toBe(false);
});
