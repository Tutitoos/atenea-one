// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Fragment, useState } from 'react';
import { api, type Model } from '@atenea/api-client';
import {
  Button, Card, ConfirmDialog, CopyButton, Dialog, EmptyState, Field, RunButton, StatusBadge,
} from './ui';
import { connection } from './domain';
import { DeviceDetail } from './device-detail';

const states = { pending: 'Pendiente', approved: 'Aprobado', revoked: 'Revocado' };
const connectionLabels = { online: 'En línea', stale: 'Informe antiguo', offline: 'Sin conexión', unknown: 'Desconocida' };
const tones = { online: 'ok', stale: 'warn', offline: 'bad', unknown: 'neutral' } as const;
const columns = [
  ['name', 'Nombre'], ['status', 'Estado'], ['connection', 'Conexión'], ['ip', 'IP'], ['activity', 'Última actividad'],
] as const;
type SortColumn = typeof columns[number][0];

export function Devices({ devices, accessRole, net, elapsed, refresh, routes = [], status = 'all' }: {
  devices: Model<'Device'>[]; accessRole: Model<'Role'>; net: string; elapsed: number;
  refresh: () => Promise<unknown>; routes?: Model<'Route'>[]; status?: string;
}) {
  const [search, setSearch] = useState('');
  const [stateFilter, setStateFilter] = useState(status);
  const [typeFilter, setTypeFilter] = useState('all');
  const [sort, setSort] = useState<{ column: SortColumn; descending: boolean }>({ column: 'name', descending: false });
  const [expanded, setExpanded] = useState<string[]>([]);
  const canEdit = accessRole !== 'member';
  const pending = devices.filter(device => device.status === 'pending');
  const query = search.trim().toLocaleLowerCase('es');
  const rows = devices.filter(device =>
    (stateFilter === 'all' || device.status === stateFilter) &&
    (typeFilter === 'all' || device.role === typeFilter) &&
    [device.name, device.ipv4, device.ipv6, device.fingerprint].some(value => value?.toLocaleLowerCase('es').includes(query)),
  );
  const sortValue = (device: Model<'Device'>) => {
    switch (sort.column) {
      case 'name': return device.name;
      case 'status': return states[device.status];
      case 'connection': return connectionLabels[connection(device.report, elapsed)];
      case 'ip': return device.ipv4 ?? device.ipv6 ?? '';
      case 'activity': return device.report?.reported_at ?? '';
    }
  };
  rows.sort((a, b) => sortValue(a).localeCompare(sortValue(b), 'es', { numeric: true }) * (sort.descending ? -1 : 1));
  return <div className="devices-page">
    {pending.length > 0 && <Card title={`Solicitudes pendientes (${pending.length})`} className="devices-pending">
      <ul>{pending.map(device => {
        const params = { path: { net, dev: device.id } };
        return <li key={device.id}>
          <div><h3>{device.name}</h3><p><span>{device.join_origin === 'request' ? 'Solicitud de unión' : 'Token de incorporación'}</span>
            {' · '}<time dateTime={device.enrolled_at}>{new Date(device.enrolled_at).toLocaleString('es')}</time></p>
            <p className="devices-fingerprint"><code>{device.fingerprint}</code>
              <CopyButton value={device.fingerprint} label={`Copiar huella de ${device.name}`}/></p></div>
          {canEdit ? <div className="actions">
            <Dialog trigger="Aprobar" title={`Aprobar ${device.name}`}
              description="Compara esta huella por otro canal antes de confirmar.">
              <code className="devices-fingerprint">{device.fingerprint}</code>
              <RunButton variant="primary" run={async () => {
                await api.POST('/api/v1/networks/{net}/devices/{dev}/approve', { params });
                await refresh();
              }}>Confirmar aprobación</RunButton>
            </Dialog>
            <ConfirmDialog trigger="Rechazar" title={`Rechazar ${device.name}`}
              description="No se podrá volver a aprobar esta solicitud. El dispositivo deberá iniciar una nueva incorporación."
              label="Confirmar rechazo" confirm={async () => {
                await api.POST('/api/v1/networks/{net}/devices/{dev}/revoke', { params });
                await refresh();
              }}/>
          </div> : <p>Solo lectura</p>}
        </li>;
      })}</ul>
    </Card>}
    <Card title="Inventario">
      <div className="devices-filters">
        <Field label="Buscar dispositivos" help="Por nombre, IP o huella completa.">{props =>
          <input {...props} type="search" value={search} onChange={event => setSearch(event.target.value)}/>}</Field>
        <Field label="Estado">{props => <select {...props} value={stateFilter} onChange={event => setStateFilter(event.target.value)}>
          <option value="all">Todos los estados</option>
          {Object.entries(states).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select>}</Field>
        <Field label="Tipo">{props => <select {...props} value={typeFilter} onChange={event => setTypeFilter(event.target.value)}>
          <option value="all">Todos los tipos</option><option value="device">Dispositivo</option><option value="server">Servidor</option>
        </select>}</Field>
      </div>
      {!devices.length ? <EmptyState message="No hay dispositivos."
        action={canEdit ? 'Crea un token en Incorporar para añadir el primero.' : 'Pide a un administrador que incorpore un dispositivo.'}/>
        : !rows.length ? <EmptyState message="Ningún resultado para el filtro." action={
          <Button onClick={() => { setSearch(''); setStateFilter('all'); setTypeFilter('all'); }}>Limpiar filtros</Button>
        }/> : <>
          {devices.every(device => device.status === 'pending') &&
            <p>Hay solicitudes pendientes de revisión.</p>}
          <div className="table-scroll"><table className="devices-table">
            <caption>Inventario · los caminos son declaraciones de los agentes</caption>
            <thead><tr>{columns.map(([column, label]) =>
              <th scope="col" key={column} aria-sort={sort.column === column ? sort.descending ? 'descending' : 'ascending' : 'none'}>
                <Button variant="link" aria-label={`Ordenar por ${label}`} onClick={() => setSort({
                  column, descending: sort.column === column && !sort.descending,
                })}>{label}{sort.column === column ? sort.descending ? ' ↓' : ' ↑' : ''}</Button>
              </th>)}</tr></thead>
            <tbody>{rows.map(device => {
              const isExpanded = expanded.includes(device.id);
              const state = connection(device.report, elapsed);
              return <Fragment key={device.id}><tr>
                <td><Button variant="link" aria-label={`Ver detalle de ${device.name}`}
                  aria-expanded={isExpanded} aria-controls={`device-${device.id}`} onClick={() =>
                    setExpanded(current => isExpanded ? current.filter(id => id !== device.id) : [...current, device.id])}>
                  {device.name}</Button><br/><StatusBadge>{device.role === 'server' ? 'Servidor' : 'Dispositivo'}</StatusBadge></td>
                <td><StatusBadge tone={device.status === 'approved' ? 'ok' : device.status === 'pending' ? 'warn' : 'bad'}>
                  {states[device.status]}</StatusBadge></td>
                <td><StatusBadge tone={tones[state]}>{connectionLabels[state]}</StatusBadge></td>
                <td>{device.ipv4 && <div>{device.ipv4}</div>}{device.ipv6 && <div>{device.ipv6}</div>}
                  {!device.ipv4 && !device.ipv6 && '—'}</td>
                <td>{device.report ? `Hace ${device.report.age_secs + elapsed} s` : 'Sin informe'}</td>
              </tr>
              {isExpanded && <tr id={`device-${device.id}`}><td colSpan={5}>
                <DeviceDetail device={device} devices={devices} net={net} elapsed={elapsed} routes={routes}
                  canEdit={canEdit} refresh={refresh}/>
              </td></tr>}
              </Fragment>;
            })}</tbody>
          </table></div>
        </>}
    </Card>
  </div>;
}
