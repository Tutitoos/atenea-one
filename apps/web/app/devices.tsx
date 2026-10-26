// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useTable, tableFeatures, flexRender, type ColumnDef } from '@tanstack/react-table';
import { api, type Model } from '@atenea/api-client';
import { ActionForm, Dialog, Empty, RunButton } from './ui';
import { connection, endpointsSchema, nameSchema } from './domain';
const features = tableFeatures({});
export function Devices({ devices, accessRole: role, net, elapsed, refresh, routes = [] }: { devices: Model<'Device'>[]; accessRole: Model<'Role'>; net: string; elapsed: number; refresh: () => Promise<unknown>; routes?: Model<'Route'>[] }) {
  const columns: ColumnDef<typeof features, Model<'Device'>>[] = [
    { accessorKey: 'name', header: 'Dispositivo' },
    { accessorKey: 'status', header: 'Estado' },
    { id: 'connection', header: 'Conexión declarada', cell: ({ row }) => connection(row.original.report, elapsed) },
    { id: 'paths', header: 'Caminos declarados / antigüedad', cell: ({ row }) => row.original.report ? <>{row.original.report.paths.map(p => <div key={p.peer}>{p.peer}: {p.path} · último RX {p.last_rx_secs === null || p.last_rx_secs === undefined ? 'desconocido' : `${p.last_rx_secs + (row.original.report?.age_secs ?? 0) + elapsed} s`} {p.rtt_ms != null && `· RTT ${p.rtt_ms} ms`}</div>)}<small>Informe hace {row.original.report.age_secs + elapsed} s</small></> : 'Sin informe' },
    { id: 'routing', header: 'Rutas / DNS / forwarding declarados', cell: ({ row }) => {
      const device = row.original;
      const report = device.report;
      if (!report) return 'Sin informe del agente';
      return <><p>Forwarding: {report.forwarding_ok == null ? 'desconocido' : report.forwarding_ok ? 'correcto' : 'desactivado'}</p><p>DNS: {report.dns ?? 'desconocido'}</p>{report.forwarding_ok === false && routes.some(route => route.device_id === device.id && route.status === 'approved') && <p className="warning" role="status">Este router tiene rutas aprobadas pero declara forwarding desactivado; no puede reenviar tráfico de la LAN.</p>}{report.routes?.map(route => <div key={route.prefix}><code>{route.prefix}</code>: {route.state === 'installed' ? 'instalada' : route.state === 'conflict' ? 'conflicto' : 'pendiente'}{route.reason && ` · ${route.reason}`}</div>)}{!report.routes?.length && <p>Sin rutas informadas</p>}<small>Declarado por el agente hace {report.age_secs + elapsed} s; no verificado independientemente.</small></>;
    } },
    { id: 'ips', header: 'IPs', cell: ({ row }) => <>{row.original.ipv4 ?? '—'}<br/>{row.original.ipv6 ?? '—'}</> },
    { accessorKey: 'fingerprint', header: 'Huella', cell: ({ row }) => <code>{row.original.fingerprint}</code> },
    { id: 'actions', header: 'Acciones', cell: ({ row }) => { const d = row.original; const params = { path: { net, dev: d.id } }; return role === 'member' ? 'Solo lectura' : <div className="actions">{d.status !== 'revoked' && <Dialog trigger={d.status === 'pending' ? 'Aprobar' : 'Revocar'} title={`${d.status === 'pending' ? 'Aprobar' : 'Revocar'} ${d.name}`} description="Compara esta huella por otro canal antes de confirmar."><code>{d.fingerprint}</code><RunButton run={async () => { if (d.status === 'pending') await api.POST('/api/v1/networks/{net}/devices/{dev}/approve', { params }); else await api.POST('/api/v1/networks/{net}/devices/{dev}/revoke', { params }); await refresh(); }}>Confirmar {d.status === 'pending' ? 'aprobación' : 'revocación'}</RunButton></Dialog>}<Dialog trigger="Editar" title={`Editar ${d.name}`} description="Los endpoints son direcciones públicas declaradas por el administrador; máximo cuatro."><ActionForm label="Guardar dispositivo" fields={[{ name: 'name', label: 'Nombre', value: d.name }, { name: 'endpoints', label: 'Endpoints separados por coma', value: d.endpoints.join(', '), required: false }]} submit={async v => { const endpoints = endpointsSchema.parse(v.endpoints.trim() ? v.endpoints.split(',').map(s => s.trim()) : []); await api.PATCH('/api/v1/networks/{net}/devices/{dev}', { params, body: { name: nameSchema.parse(v.name), endpoints } }); await refresh(); }}/></Dialog></div>; } },
  ];
  const table = useTable({ features, data: devices, columns, getRowId: device => device.id });
  if (!devices.length) return <Empty>No hay dispositivos. Crea un token en Incorporar.</Empty>;
  // Las plantillas son callbacks puros, no tipos de componente: el reloj no debe desmontar los diálogos.
  return <div className="table-scroll">
    <table>
      <caption>Inventario · los caminos son declaraciones de los agentes</caption>
      <thead>{table.getHeaderGroups().map(group => <tr key={group.id}>{group.headers.map(header => <th scope="col" key={header.id}>{header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}</th>)}</tr>)}</thead>
      <tbody>{table.getRowModel().rows.map(row => <tr key={row.id}>{row.getAllCells().map(cell => {
        const template = cell.column.columnDef.cell;
        return <td key={cell.id}>{typeof template === 'function' ? template(cell.getContext()) : template}</td>;
      })}</tr>)}</tbody>
    </table>
  </div>;
}
