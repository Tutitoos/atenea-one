import { z } from 'zod';
import type { Model } from '@atenea/api-client';
// La CSP no permite eval; incluso el sondeo JIT de Zod genera una infracción.
z.config({ ...z.locales.es(), jitless: true });
export const nameSchema = z.string().trim().min(1, 'Indica un nombre.').refine(v => new TextEncoder().encode(v).length <= 255, 'Máximo 255 bytes UTF-8.');
export const rootAnchor = z.string().regex(/^[a-fA-F0-9]{64}$/, 'El ancla debe contener 64 caracteres hexadecimales.');
function networkAddress(value: string) {
  if (!z.union([z.cidrv4(), z.cidrv6()]).safeParse(value).success) return false;
  const [address, prefix] = value.split('/');
  const ipv6 = address.includes(':');
  let parts: string[];
  if (ipv6) {
    const expanded = address.replace(/(?:\d+\.){3}\d+$/, v => {
      const octets = v.split('.').map(Number);
      return `${((octets[0] << 8) | octets[1]).toString(16)}:${((octets[2] << 8) | octets[3]).toString(16)}`;
    });
    const [left, right] = expanded.split('::');
    const first = left ? left.split(':') : [];
    const last = right ? right.split(':') : [];
    parts = right === undefined ? first : [...first, ...Array<string>(8-first.length-last.length).fill('0'), ...last];
  } else parts = address.split('.');
  const bits = ipv6 ? 128 : 32;
  const number = parts.reduce((n,p) => (n << BigInt(ipv6 ? 16 : 8)) | BigInt(ipv6 ? `0x${p}` : p), 0n);
  return (number & ((1n << BigInt(bits-Number(prefix))) - 1n)) === 0n;
}
const ipv4Cidr = z.cidrv4().refine(networkAddress, 'El CIDR debe ser una dirección de red, sin bits de host.');
const ipv6Cidr = z.cidrv6().refine(networkAddress, 'El CIDR debe ser una dirección de red, sin bits de host.');
export const networkSchema = z.object({ name: nameSchema, ipv4_cidr: ipv4Cidr, ipv6_cidr: ipv6Cidr, root_anchor: rootAnchor });
export const privatePrefix = z.union([ipv4Cidr, ipv6Cidr]).refine(value => {
  const [address, length] = value.split('/');
  if (address.includes(':')) return Number(length) >= 32 && (parseInt(address.split(':')[0], 16) & 0xfe00) === 0xfc00;
  const [a, b] = address.split('.').map(Number);
  const prefix = Number(length);
  return prefix >= 8 && (a === 10 || (a === 172 && b >= 16 && b <= 31 && prefix >= 12) || (a === 192 && b === 168 && prefix >= 16) || (a === 100 && b >= 64 && b <= 127 && prefix >= 10));
}, 'Solo prefijos privados canónicos: RFC 1918, CGNAT o ULA; mínimo /8 IPv4 o /32 IPv6. No hay salida a Internet.');
export const dnsDomain = z.string().max(253).regex(/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*$/, 'Usa dominios LDH en minúsculas, sin etiquetas vacías ni punto final; no se admite el dominio raíz.');
export const dnsSchema = z.object({
  servers: z.array(z.union([z.ipv4(), z.ipv6()])).max(3).refine(values => new Set(values.map(value => z.ipv6().safeParse(value).success ? new URL(`http://[${value}]`).hostname : value)).size === values.length, 'No repitas servidores.'),
  domains: z.array(dnsDomain).max(8).refine(values => new Set(values).size === values.length, 'No repitas dominios.'),
});
export const endpoint = z.string().refine(value => {
  const match = /^(?:\[([^\]]+)\]|([^:]+)):(\d+)$/.exec(value);
  if (!match || Number(match[3]) < 1 || Number(match[3]) > 65535) return false;
  return match[1] ? z.ipv6().safeParse(match[1]).success : z.ipv4().safeParse(match[2]).success;
}, 'Usa IP:puerto o [IPv6]:puerto, con puerto 1–65535.');
export const endpointsSchema = z.array(endpoint).max(4);
export const selector = z.discriminatedUnion('kind', [z.object({ kind: z.literal('any') }), z.object({ kind: z.literal('device'), device_id: z.guid() }), z.object({ kind: z.literal('prefix'), prefix: z.union([ipv4Cidr, ipv6Cidr]) })]);
export const policySchema = z.object({ src: selector, dst: selector, proto: z.union([z.literal(0), z.literal(1), z.literal(6), z.literal(17), z.literal(58)]), port_min: z.number().int().min(0).max(65535), port_max: z.number().int().min(0).max(65535) }).refine(v => v.port_min <= v.port_max && ([6, 17].includes(v.proto) || (v.port_min === 0 && v.port_max === 0)), 'Puertos ordenados y solo para TCP/UDP.');
export const tokenSchema = z.object({ ttl_secs: z.number().int().min(60).max(86400), auto_approve: z.boolean() });
export function connection(report: Model<'DeviceReport'> | null, elapsed = 0): Model<'Connection'> { const age = report ? report.age_secs + elapsed : null; return age === null ? 'unknown' : age < 90 ? 'online' : age < 600 ? 'stale' : 'offline'; }
export function relays(devices: Model<'Device'>[], elapsed = 0) {
  const observed = new Map<string, { relay: string; age: number; devices: Set<string> }>();
  for (const device of devices) for (const path of device.report?.paths ?? []) {
    if (!path.relay || !device.report) continue;
    const age = device.report.age_secs + elapsed;
    const row = observed.get(path.relay) ?? { relay: path.relay, age, devices: new Set<string>() };
    row.age = Math.min(row.age, age); row.devices.add(device.name); observed.set(path.relay, row);
  }
  return [...observed.values()];
}
export function secondsSince(updatedAt: number, now = Date.now()) { return updatedAt ? Math.max(0, Math.floor((now - updatedAt) / 1000)) : 0; }
export function shellQuote(value: string) { return `'${value.replaceAll("'", "'\\''")}'`; }
