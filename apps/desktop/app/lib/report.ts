// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { int, num, pct, text } from './format';
import type { Reply } from './status';

type Group = { title: string; match: RegExp };
// Orden = prioridad: la primera familia que coincide se queda con el contador; lo no reconocido va a «Otros».
const FAMILIES: Group[] = [
  { title: 'Configuración y lease', match: /^(config_|denied_no_lease|lease)/ },
  { title: 'Caminos', match: /^(path_|probe_)/ },
  { title: 'Relay', match: /^relay_/ },
  { title: 'Handshakes', match: /^(tx_init|rx_init|init_|cookie_|tx_cookie|rx_cookie|handshake)/ },
  { title: 'Tráfico denegado o descartado', match: /(_acl_denied|^rx_bad_|^rx_malformed|^rx_oversize|^tx_malformed|^tx_bad_source|^tx_no_route|^tx_encrypt)/ },
];

/** Agrupa por familia y, salvo `all`, oculta los que valen 0. Devuelve solo las familias con algo que enseñar. */
export function groupCounters(counters: Record<string, number>, all: boolean) {
  const groups = [...FAMILIES, { title: 'Otros', match: /./ }].map((g) => ({ title: g.title, items: [] as [string, number][] }));
  for (const [k, v] of Object.entries(counters).sort(([a], [b]) => (a < b ? -1 : 1))) {
    if (!all && v === 0) continue;
    const i = FAMILIES.findIndex((f) => f.match.test(k));
    groups[i < 0 ? FAMILIES.length : i].items.push([k, v]);
  }
  return groups.filter((g) => g.items.length);
}

// Pasada de redacción para texto libre: cualquier cosa con forma de dirección, URL o identificador largo.
// shortcut: puede redactar de más (p. ej. una hora «12:30:45»); es preferible a filtrar una dirección.
const PATTERNS: [RegExp, string][] = [
  [/\b[a-z][a-z0-9+.-]*:\/\/\S+/gi, '[url]'],
  [/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, '[id]'],
  [/\b\d{1,3}(?:\.\d{1,3}){3}(?:\/\d+)?(?::\d+)?/g, '[ip]'],
  [/(?=[0-9a-f:]*[0-9a-f])[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7}(?:\/\d+)?/gi, '[ip]'],
  [/[A-Za-z0-9+/_-]{24,}={0,2}/g, '[clave]'],
  [/\b[a-z0-9-]+(?:\.[a-z0-9-]+)+\b/gi, '[host]'],
];

export type ReportInput = { reply: Reply; network?: string };

/** Informe en texto plano para pegar en un correo o una incidencia. Sin IP, claves, huellas, nombres, redes ni endpoints. */
export function buildReport({ reply, network }: ReportInput): string {
  const { status, health } = reply;
  const aliases = new Map<string, string>();
  const counts: Record<string, number> = {};
  /** Etiqueta estable (`equipo-1`…) por valor y tipo, en orden de primera aparición. */
  const tag = (kind: string, ...values: (string | null | undefined)[]) => {
    const known = values.find((v) => v && aliases.has(v));
    const label = (known && aliases.get(known)) ?? `${kind}-${(counts[kind] = (counts[kind] ?? 0) + 1)}`;
    for (const v of values) if (v) aliases.set(v, label);
    return label;
  };
  const self = tag('este-equipo', status.public_key, status.hostname, ...status.addrs);
  if (network) tag('red', network);
  tag('servidor', status.sync?.url, status.coordinator?.url);
  const peers = status.peers.map((p) => ({ p, label: tag('equipo', p.public_key, p.device_id, p.name, p.endpoint, p.session, ...p.addrs) }));
  const entries = status.relay?.entries ?? [];
  // La entrada identifica al par por su clave (o un prefijo); si no es de ningún par conocido, es un servidor.
  const owners = entries.map((e) => {
    const id = text(e.peer);
    if (!id) return 'desconocido';
    const owner = peers.find(({ p }) => p.public_key.startsWith(id))?.label;
    if (owner) aliases.set(id, owner);
    return owner ?? tag('servidor', id);
  });
  const literals = [...aliases].filter(([v]) => v.length >= 3).sort((a, b) => b[0].length - a[0].length);
  const clean = (value: string | null | undefined) => {
    let out = value ?? '';
    for (const [literal, label] of literals) out = out.split(literal).join(label);
    for (const [re, to] of PATTERNS) out = out.replace(re, to);
    return out;
  };
  const server = status.server;
  const versions = Object.entries({ 'control plane': server?.control_plane, coordinator: server?.coordinator, relay: server?.relay }).flatMap(([name, v]) => (v ? [`${name} ${v.version}`] : []));
  const lines: string[] = [
    'Informe de diagnóstico de Atenea One',
    'Anonimizado: no incluye IP, claves, nombres de equipo, redes ni direcciones; las etiquetas (equipo-1…) son estables dentro del informe.',
    `Versiones: ${versions.join(' · ') || 'no informadas'}`,
    `Estado general: ${{ ok: 'correcto', degraded: 'degradado', down: 'caído' }[health.level]} — ${clean(health.text)}`,
    `Avisos: ${health.reasons.length ? health.reasons.map(clean).join('; ') : 'ninguno'}`,
    `Lease: ${status.config.lease_active ? 'activo' : 'inactivo'} · configuración v${status.config.version}${status.config.valid_until_unix ? ` · vigente hasta ${new Date(status.config.valid_until_unix * 1000).toISOString()}` : ''}`,
    `NAT: ${clean(status.nat?.mapping) || 'sin datos'}${status.nat ? ` · dirección pública ${status.nat.reflexive ? 'vista' : 'no vista'}` : ''}`,
  ];
  const byPath: Record<string, number> = {};
  for (const { p } of peers) { const k = p.path.startsWith('relay') ? 'relay' : p.path; byPath[k] = (byPath[k] ?? 0) + 1; }
  lines.push(`Pares (${peers.length}): ${Object.entries(byPath).map(([k, n]) => `${clean(k)} ${n}`).join(' · ') || 'ninguno'}`);
  for (const { p, label } of peers) lines.push(`  ${label}${p.role === 'server' ? ' (servidor)' : ''}: ${clean(p.path)}`);
  const groups = groupCounters(status.counters, false);
  lines.push(`Contadores distintos de cero:${groups.length ? '' : ' ninguno'}`);
  for (const g of groups) lines.push(`  ${g.title}: ${g.items.map(([k, v]) => `${k}=${int(v)}`).join(', ')}`);
  lines.push(`Relays (${entries.length}):${entries.length ? '' : ' ninguno'}`);
  for (const [i, e] of entries.entries()) {
    const sent = num(e.pings_sent), lost = num(e.pings_lost);
    lines.push(`  ${owners[i]} · ${clean(text(e.transport)) || '?'} · ${clean(text(e.state)) || '?'}${sent > 0 ? ` · pings perdidos ${pct(lost, sent)} (${int(lost)}/${int(sent)})` : ''}${text(e.last_error) ? ` · error: ${clean(text(e.last_error))}` : ''}`);
  }
  lines.push('Errores de sincronización:');
  for (const [name, error] of [['Configuración', status.config.last_error], ['Control plane', status.sync?.last_error], ['Coordinator', status.coordinator?.last_error], ['Servidor', server?.last_error]] as const) lines.push(`  ${name}: ${clean(error) || 'ninguno'}`);
  lines.push(`Equipo local: ${self}`);
  return lines.join('\n');
}
