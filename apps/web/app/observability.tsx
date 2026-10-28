// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useId, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, result, type Model } from '@atenea/api-client';
import { LineChart, Line, XAxis, YAxis, Tooltip, Legend, ResponsiveContainer } from 'recharts';
import { Button, Empty, Field, ShortId, Status, StatusBadge } from './ui';
import { relays } from './domain';

const auditActions: Record<string, string> = {
  'token.create': 'Token de incorporación creado',
  'device.enroll': 'Dispositivo incorporado',
  'device.join_requested': 'Unión de dispositivo solicitada',
  'device.approve': 'Dispositivo aprobado',
  'device.revoke': 'Dispositivo revocado',
  'device.leave': 'Dispositivo desconectado de la red',
  'device.role': 'Tipo de dispositivo cambiado',
  'device.update': 'Dispositivo actualizado',
  'dns.set': 'DNS actualizado',
  'config.issue': 'Configuración emitida',
  'network.join_policy': 'Política de unión cambiada',
  'network.create': 'Red creada',
  'network.delete': 'Red eliminada',
  'network.issuer_cert': 'Certificado de emisor instalado',
  'org.create': 'Organización creada',
  'org.delete': 'Organización eliminada',
  'org.rename': 'Organización renombrada',
  'network.rename': 'Red renombrada',
  'member.set': 'Membresía actualizada',
  'policy.create': 'Política creada',
  'policy.delete': 'Política eliminada',
  'route.declare': 'Subredes declaradas',
  'route.approve': 'Subred aprobada',
  'route.reject': 'Subred rechazada o revocada',
};
export function auditAction(action: string) {
  return auditActions[action] ?? action;
}
export function AuditChange({ action, detail }: { action: string; detail: unknown }) {
  if (!detail || typeof detail !== 'object') return null;
  const data = detail as Record<string, unknown>;
  const previous = action === 'member.set' ? data.previous_role : data.previous_name;
  const next = action === 'member.set' ? data.role : data.name;
  return ['member.set', 'org.rename', 'network.rename'].includes(action) &&
    (typeof previous === 'string' || previous === null) && typeof next === 'string' ?
    <p>{previous ?? 'Sin rol'} → {next}</p> : null;
}
export function DeviceIdentity({ id, devices = [] }: { id: string; devices?: Model<'Device'>[] }) {
  return devices.find(device => device.id === id)?.name ?? <ShortId value={id}/>;
}

export function Relays({ devices, elapsed }: { devices: Model<'Device'>[]; elapsed: number }) {
  const rows = relays(devices, elapsed);
  return <section><h2>Relays observados por los agentes</h2>
    <p>No es un inventario de infraestructura ni una comprobación independiente de disponibilidad.</p>
    {rows.length ? <ul className="cards">{rows.map(row => <li className="card" key={row.relay}>
      <h3>{devices.some(device => device.id === row.relay) ?
        <DeviceIdentity id={row.relay} devices={devices}/> :
        /^[0-9a-f-]{36}$/i.test(row.relay) ? <ShortId value={row.relay}/> : row.relay}</h3>
      <StatusBadge tone={row.age < 90 ? 'ok' : row.age < 600 ? 'warn' : 'neutral'}>
        {row.age < 90 ? 'Observación reciente' : row.age < 600 ? 'Observación antigua' : 'Sin observación reciente'}
      </StatusBadge>
      <p>Observación más reciente hace {row.age} s</p><p>{[...row.devices].join(', ')}</p>
    </li>)}</ul> : <Empty>Ningún agente ha declarado un relay.</Empty>}
  </section>;
}
export function Topology({ devices, elapsed }: { devices: Model<'Device'>[]; elapsed: number }) {
  const title = useId();
  const nodes = devices.map((device, index) => ({ ...device,
    x: 300 + 220 * Math.cos(2 * Math.PI * index / Math.max(1, devices.length)),
    y: 260 + 190 * Math.sin(2 * Math.PI * index / Math.max(1, devices.length)),
  }));
  const paths = nodes.flatMap(device => (device.report?.paths ?? []).map(path => ({
    from: device, to: nodes.find(node => node.id === path.peer), ...path,
  })));
  return <section className="obs-topology"><h2>Topología declarada</h2>
    <p>Camino por par declarado por cada agente; no se infieren conexiones. Línea continua: directo; discontinua: relay; punteada: sondeo o sin camino.</p>
    {!devices.length ? <Empty>No hay dispositivos.</Empty> : <>
      <svg viewBox="0 0 600 520" role="img" aria-labelledby={title}>
        <title id={title}>Dispositivos y caminos declarados. Detalle equivalente en la tabla siguiente.</title>
        {paths.map((path, index) => path.to && <line key={index}
          x1={path.from.x} y1={path.from.y} x2={path.to.x} y2={path.to.y}
          stroke={path.path === 'direct' ? 'var(--ok)' : path.path.startsWith('relay') ? 'var(--sel)' : 'var(--off)'}
          strokeWidth="2" strokeDasharray={path.path === 'direct' ? undefined : path.path.startsWith('relay') ? '8 4' : '2 5'}/>)}
        {nodes.map(device => <g key={device.id}><circle cx={device.x} cy={device.y} r="7" fill="currentColor"/>
          <text x={device.x} y={device.y - 15} textAnchor="middle" fill="currentColor">{device.name}</text>
        </g>)}
      </svg>
      <div className="table-scroll"><table><caption>Caminos declarados por origen</caption>
        <thead><tr><th>Origen</th><th>Destino</th><th>Camino</th><th>Antigüedad del informe</th></tr></thead>
        <tbody>{paths.map((path, index) => <tr key={index}><td>{path.from.name}</td>
          <td><DeviceIdentity id={path.peer} devices={devices}/></td>
          <td>{path.path === 'direct' ? 'Directo' : path.path.startsWith('relay') ? 'Relay' : path.path}</td>
          <td>{(path.from.report?.age_secs ?? 0) + elapsed} s</td>
        </tr>)}</tbody>
      </table></div>{!paths.length && <Empty>No hay caminos declarados.</Empty>}
    </>}
  </section>;
}
const series = [
  { key: 'total', label: 'Total', color: '--fg' },
  { key: 'direct', label: 'Directo', color: '--ok' },
  { key: 'relay', label: 'Relay', color: '--sel' },
  { key: 'probing', label: 'Sondeo', color: '--warn' },
  { key: 'unknown', label: 'Desconocido', color: '--off' },
  { key: 'offline', label: 'Sin conexión', color: '--bad' },
];
export function Metrics({ net }: { net: string }) {
  const query = useQuery({ queryKey: ['network', net, 'metrics'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/metrics', {
      params: { path: { net }, query: { limit: 360 } },
    })) });
  return <section className="obs-metrics"><h2>Métricas reales</h2>
    <p>Muestras del servidor cada 60 s. Solo se dibujan los puntos recibidos; no se interpolan intervalos ni se rellenan huecos.</p>
    <Status query={query}/>{query.data?.length ? <>
      <div className="chart"><ResponsiveContainer width="100%" height={320}>
        <LineChart data={query.data}><XAxis dataKey="at" stroke="var(--muted)"
          tickFormatter={value => new Date(String(value)).toLocaleTimeString('es')}/>
          <YAxis allowDecimals={false} stroke="var(--muted)"/>
          <Tooltip contentStyle={{ background: 'var(--surface)', color: 'var(--fg)', borderColor: 'var(--line)' }}/>
          <Legend formatter={label => <span className="metrics-legend-label">{label}</span>}/>
          {series.map(item => <Line key={item.key} dataKey={item.key} name={item.label}
            stroke={`var(${item.color})`} strokeWidth={0} dot={{ r: 3, fill: `var(${item.color})` }} activeDot={{ r: 5 }}
            connectNulls={false} isAnimationActive={false}/>)}</LineChart>
      </ResponsiveContainer></div>
      <div className="table-scroll"><table><caption>Muestras recibidas</caption>
        <thead><tr>{['Fecha', ...series.map(item => item.label)].map(label => <th key={label}>{label}</th>)}</tr></thead>
        <tbody>{query.data.map(sample => <tr key={sample.at}>
          <td>{new Date(sample.at).toLocaleString('es')}</td><td>{sample.total}</td><td>{sample.direct}</td>
          <td>{sample.relay}</td><td>{sample.probing}</td><td>{sample.unknown}</td><td>{sample.offline}</td>
        </tr>)}</tbody>
      </table></div>
    </> : !query.isPending && !query.isError && <Empty>Todavía no hay muestras.</Empty>}
  </section>;
}
export function Audit({ net, devices = [] }: { net: string; devices?: Model<'Device'>[] }) {
  const [cursors, setCursors] = useState<number[]>([]);
  const [filter, setFilter] = useState('');
  const before = cursors.at(-1);
  const query = useQuery({ queryKey: ['network', net, 'audit', before],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/audit', {
      params: { path: { net }, query: { limit: 50, ...(before === undefined ? {} : { before }) } },
    })) });
  const needle = filter.trim().toLocaleLowerCase('es');
  const rows = query.data?.filter(event => [auditAction(event.action), event.action, event.actor_kind,
    event.actor_id, event.target, devices.find(device => device.id === event.actor_id)?.name,
    devices.find(device => device.id === event.target)?.name, JSON.stringify(event.detail),
  ].join(' ').toLocaleLowerCase('es').includes(needle));
  return <section className="obs-audit"><h2>Auditoría</h2><Status query={query}/>
    <Field label="Filtrar eventos de esta página" help="Busca por acción, actor, objetivo o detalle entre los 50 eventos de esta página.">
      {props => <input {...props} type="search" value={filter} onChange={event => setFilter(event.target.value)}/>}
    </Field>
    {rows?.length ? <div className="table-scroll"><table><caption>Eventos, del más reciente al más antiguo</caption>
      <thead><tr><th>Fecha</th><th>Actor</th><th>Acción</th><th>Destino</th><th>Detalle</th></tr></thead>
      <tbody>{rows.map(event => <tr key={event.id}>
        <td><time dateTime={event.at}>{new Date(event.at).toLocaleString('es')}</time></td>
        <td>{event.actor_kind === 'user' ? 'Usuario' : event.actor_kind === 'device' ? 'Dispositivo' : event.actor_kind}: <DeviceIdentity id={event.actor_id} devices={devices}/></td>
        <td>{auditAction(event.action)}</td><td><DeviceIdentity id={event.target} devices={devices}/></td>
        <td><AuditChange action={event.action} detail={event.detail}/><details><summary>Ver detalle JSON</summary><pre>{JSON.stringify(event.detail, null, 2)}</pre></details></td>
      </tr>)}</tbody>
    </table></div> : !query.isPending && !query.isError && <Empty>
      {query.data?.length ? 'No hay eventos que coincidan con el filtro.' : 'No hay eventos en esta página.'}
    </Empty>}
    <div className="actions"><Button disabled={!cursors.length || query.isFetching}
      onClick={() => setCursors(value => value.slice(0, -1))}>Más recientes</Button>
      <Button disabled={query.data?.length !== 50 || query.isFetching} onClick={() => {
        const id = query.data?.at(-1)?.id; if (id !== undefined) setCursors(value => [...value, id]);
      }}>Más antiguos</Button></div>
  </section>;
}
