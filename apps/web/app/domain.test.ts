// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { describe, expect, it, vi } from 'vitest';
import { device } from './device.fixture';
import { connection, endpoint, endpointsSchema, policySchema, relays } from './domain';
import { z } from 'zod';
it('valida sin sondeos eval ni compilación JIT bajo la CSP',()=>{
 expect(z.config().jitless).toBe(true);
 const constructor=vi.spyOn(globalThis,'Function').mockImplementation(()=>{throw new Error('La CSP prohíbe eval.');});
 try {
  expect(policySchema.safeParse({src:{kind:'any'},dst:{kind:'any'},proto:0,port_min:0,port_max:0}).success).toBe(true);
  expect(constructor).not.toHaveBeenCalled();
 } finally { constructor.mockRestore(); }
});
describe('validadores del contrato',()=>{
 it('rechaza endpoints inválidos y más de cuatro',()=>{for(const s of ['192.0.2.1:443','[2001:db8::1]:4242'])expect(endpoint.safeParse(s).success).toBe(true);for(const s of ['host:443','192.0.2.1:0','192.0.2.1:65536','2001:db8::1:80'])expect(endpoint.safeParse(s).success).toBe(false);expect(endpointsSchema.safeParse(Array(5).fill('192.0.2.1:80')).success).toBe(false);});
 it('valida selectores, protocolos y puertos',()=>{const p={src:{kind:'any'},dst:{kind:'prefix',prefix:'192.0.2.0/24'},proto:6,port_min:80,port_max:443};expect(policySchema.safeParse(p).success).toBe(true);for(const change of [{proto:1},{port_min:444},{port_max:65536},{src:{kind:'device',device_id:'no'}}])expect(policySchema.safeParse({...p,...change}).success).toBe(false);});
});
it('rechaza bits de host en prefijos sin normalizarlos',()=>{
 expect(policySchema.safeParse({src:{kind:'prefix',prefix:'192.0.2.1/24'},dst:{kind:'any'},proto:0,port_min:0,port_max:0}).success).toBe(false);
});
it('clasifica conexión usando edad del servidor más tiempo local transcurrido',()=>{expect(connection(null)).toBe('unknown');for(const [age,status] of [[0,'online'],[89,'online'],[90,'stale'],[599,'stale'],[600,'offline']] as const)expect(connection({reported_at:'2026-01-01T00:00:00Z',age_secs:age,nat_mapping:null,paths:[],forwarding_ok:null,dns:null,routes:null})).toBe(status);expect(connection({reported_at:'2026-01-01T00:00:00Z',age_secs:80,nat_mapping:null,paths:[],forwarding_ok:null,dns:null,routes:null},11)).toBe('stale');});
it('deduplica relays sin inventar disponibilidad',()=>{const d={...device,report:{reported_at:'2026-01-01T00:00:00Z',age_secs:20,nat_mapping:null,paths:[{peer:device.id,path:'relay-udp' as const,relay:'relay-sintetico'}],forwarding_ok:null,dns:null,routes:null}};expect(relays([d,d],5)).toEqual([{relay:'relay-sintetico',age:25,devices:new Set([device.name])}]);expect(relays([device])).toEqual([]);});
