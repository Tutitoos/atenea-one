import { describe, expect, it, vi } from 'vitest';
import { device } from './device.fixture';
import { connection, endpoint, endpointsSchema, networkSchema, policySchema, relays, rootAnchor } from './domain';
import { z } from 'zod';
it('valida sin sondeos eval ni compilación JIT bajo la CSP',()=>{
 expect(z.config().jitless).toBe(true);
 const constructor=vi.spyOn(globalThis,'Function').mockImplementation(()=>{throw new Error('La CSP prohíbe eval.');});
 try {
  expect(networkSchema.safeParse({name:'Red',ipv4_cidr:'192.0.2.0/24',ipv6_cidr:'2001:db8::/64',root_anchor:'ab'.repeat(32)}).success).toBe(true);
  expect(constructor).not.toHaveBeenCalled();
 } finally { constructor.mockRestore(); }
});
describe('validadores del contrato',()=>{
 it('valida CIDR IPv4/IPv6 y ancla hex sin crear claves',()=>{const v={name:'Red',ipv4_cidr:'192.0.2.0/24',ipv6_cidr:'2001:db8::/64',root_anchor:'ab'.repeat(32)};expect(networkSchema.safeParse(v).success).toBe(true);expect(networkSchema.safeParse({...v,ipv4_cidr:'999.0.0.0/24'}).success).toBe(false);expect(networkSchema.safeParse({...v,ipv6_cidr:'2001:db8::/129'}).success).toBe(false);expect(rootAnchor.safeParse('z'.repeat(64)).success).toBe(false);});
 it('rechaza endpoints inválidos y más de cuatro',()=>{for(const s of ['192.0.2.1:443','[2001:db8::1]:4242'])expect(endpoint.safeParse(s).success).toBe(true);for(const s of ['host:443','192.0.2.1:0','192.0.2.1:65536','2001:db8::1:80'])expect(endpoint.safeParse(s).success).toBe(false);expect(endpointsSchema.safeParse(Array(5).fill('192.0.2.1:80')).success).toBe(false);});
 it('valida selectores, protocolos y puertos',()=>{const p={src:{kind:'any'},dst:{kind:'prefix',prefix:'192.0.2.0/24'},proto:6,port_min:80,port_max:443};expect(policySchema.safeParse(p).success).toBe(true);for(const change of [{proto:1},{port_min:444},{port_max:65536},{src:{kind:'device',device_id:'no'}}])expect(policySchema.safeParse({...p,...change}).success).toBe(false);});
});
it('rechaza bits de host en redes y prefijos sin normalizarlos',()=>{
 const base={name:'Red',ipv4_cidr:'192.0.2.0/24',ipv6_cidr:'2001:db8::/64',root_anchor:'ab'.repeat(32)};
 for(const value of ['192.0.2.1/24','192.0.2.0/0','192.0.2.0/33'])expect(networkSchema.safeParse({...base,ipv4_cidr:value}).success).toBe(false);
 for(const value of ['2001:db8::1/64','2001:db8::/0','2001:db8::/129'])expect(networkSchema.safeParse({...base,ipv6_cidr:value}).success).toBe(false);
 for(const value of ['0.0.0.0/0','192.0.2.1/32'])expect(networkSchema.safeParse({...base,ipv4_cidr:value}).success).toBe(true);
 for(const value of ['::/0','::/128','2001:db8::1/128','2001:db8::/32','2001:db8:0:0:0:0:0:0/64'])expect(networkSchema.safeParse({...base,ipv6_cidr:value}).success).toBe(true);
 expect(policySchema.safeParse({src:{kind:'prefix',prefix:'192.0.2.1/24'},dst:{kind:'any'},proto:0,port_min:0,port_max:0}).success).toBe(false);
});
it('clasifica conexión usando edad del servidor más tiempo local transcurrido',()=>{expect(connection(null)).toBe('unknown');for(const [age,status] of [[0,'online'],[89,'online'],[90,'stale'],[599,'stale'],[600,'offline']] as const)expect(connection({reported_at:'2026-01-01T00:00:00Z',age_secs:age,nat_mapping:null,paths:[],forwarding_ok:null,dns:null,routes:null})).toBe(status);expect(connection({reported_at:'2026-01-01T00:00:00Z',age_secs:80,nat_mapping:null,paths:[],forwarding_ok:null,dns:null,routes:null},11)).toBe('stale');});
it('deduplica relays sin inventar disponibilidad',()=>{const d={...device,report:{reported_at:'2026-01-01T00:00:00Z',age_secs:20,nat_mapping:null,paths:[{peer:device.id,path:'relay-udp' as const,relay:'relay-sintetico'}],forwarding_ok:null,dns:null,routes:null}};expect(relays([d,d],5)).toEqual([{relay:'relay-sintetico',age:25,devices:new Set([device.name])}]);expect(relays([device])).toEqual([]);});
