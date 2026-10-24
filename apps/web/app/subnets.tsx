import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { flexRender, tableFeatures, useTable, type ColumnDef } from '@tanstack/react-table';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, Dialog, Empty, RunButton, Status } from './ui';
const features = tableFeatures({});

function Approval({ route, device, net, refresh }: { route: Model<'Route'>; device: string; net: string; refresh: () => Promise<unknown> }) {
  const [snat, setSnat] = useState('');
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState('');
  return <><p><strong>{device}</strong> · <code>{route.prefix}</code></p><fieldset disabled={pending}><legend>Elige explícitamente el modo SNAT</legend><label><input type="radio" name={`snat-${route.id}`} value="false" checked={snat === 'false'} onChange={e => setSnat(e.target.value)}/>Sin SNAT: exige rutas de retorno en la LAN y conserva el origen real del cliente.</label><label><input type="radio" name={`snat-${route.id}`} value="true" checked={snat === 'true'} onChange={e => setSnat(e.target.value)}/>Con SNAT: enmascara el tráfico sin tocar la LAN, pero se pierde el origen real.</label></fieldset><Button disabled={!snat || pending} onClick={() => {
    if (!snat || pending) return;
    setPending(true); setMessage('');
    void api.POST('/api/v1/networks/{net}/routes/{id}/approve', { params: { path: { net, id: route.id } }, body: { snat: snat === 'true' } }).then(async () => { await refresh(); setMessage('Ruta aprobada.'); }, error => {
      setMessage(error instanceof ApiError && error.status === 409 && error.detail.error.code === 'route_overlap' ? 'No se puede aprobar: el prefijo se solapa con la red del túnel u otra ruta aprobada. Revisa las subredes antes de volver a intentarlo.' : error instanceof Error ? error.message : 'No se pudo aprobar la ruta.');
    }).catch(error => setMessage(error instanceof Error ? error.message : 'No se pudo actualizar la lista.')).finally(() => setPending(false));
  }}>Aprobar ruta</Button><p role="status">{message}</p></>;
}

export function Subnets({ net, canEdit, devices }: { net: string; canEdit: boolean; devices: Model<'Device'>[] }) {
  const cache = useQueryClient();
  const query = useQuery({ queryKey: ['network', net, 'routes'], queryFn: () => result(api.GET('/api/v1/networks/{net}/routes', { params: { path: { net } } })) });
  const refresh = () => cache.invalidateQueries({ queryKey: ['network', net] });
  const deviceName = (route: Model<'Route'>) => devices.find(device => device.id === route.device_id)?.name ?? route.device_id;
  const columns: ColumnDef<typeof features, Model<'Route'>>[] = [
    { id: 'device', header: 'Dispositivo', cell: ({ row }) => <>{deviceName(row.original)}<br/><small>{row.original.device_id}</small></> },
    { accessorKey: 'prefix', header: 'Prefijo' },
    { accessorKey: 'status', header: 'Estado' },
    { id: 'snat', header: 'SNAT', cell: ({ row }) => row.original.snat === null ? 'Sin decidir' : row.original.snat ? 'Con SNAT' : 'Sin SNAT' },
    { accessorKey: 'declared_at', header: 'Fecha de declaración' },
    { id: 'decided', header: 'Fecha de decisión', cell: ({ row }) => row.original.decided_at ?? 'Sin decisión' },
    { id: 'actions', header: 'Acciones', cell: ({ row }) => {
      const route = row.original;
      return !canEdit ? 'Solo lectura' : <div className="actions">{route.status !== 'approved' && <Dialog trigger="Aprobar" title="Aprobar ruta de subred" description="La aprobación autoriza este prefijo; elige el tratamiento del origen antes de confirmar."><Approval route={route} device={deviceName(route)} net={net} refresh={refresh}/></Dialog>}{route.status !== 'rejected' && <Dialog trigger={route.status === 'approved' ? 'Revocar' : 'Rechazar'} title={route.status === 'approved' ? 'Revocar ruta' : 'Rechazar ruta'} description="El prefijo dejará de estar autorizado. Las políticas que dependan de él dejarán de seleccionar esta ruta."><p>{deviceName(route)} · <code>{route.prefix}</code></p><RunButton run={async () => { await api.POST('/api/v1/networks/{net}/routes/{id}/reject', { params: { path: { net, id: route.id } } }); await refresh(); }}>Confirmar {route.status === 'approved' ? 'revocación' : 'rechazo'}</RunButton></Dialog>}</div>;
    } },
  ];
  const table = useTable({ features, data: query.data ?? [], columns, getRowId: route => route.id });
  return <section aria-label="Subredes"><h2>Subredes</h2><p className="warning">Solo se sirven prefijos privados (RFC 1918, CGNAT y ULA). No hay salida a Internet ni ruta por defecto. Las rutas las declara el agente y requieren aprobación de owner o admin.</p><Status query={query}/>{query.isError && <RunButton run={() => query.refetch()}>Reintentar rutas</RunButton>}{query.data && (query.data.length ? <div className="table-scroll"><table><caption>Rutas declaradas por los dispositivos</caption><thead>{table.getHeaderGroups().map(group => <tr key={group.id}>{group.headers.map(header => <th scope="col" key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</th>)}</tr>)}</thead><tbody>{table.getRowModel().rows.map(row => <tr key={row.id}>{row.getAllCells().map(cell => { const template = cell.column.columnDef.cell; return <td key={cell.id}>{typeof template === 'function' ? template(cell.getContext()) : template}</td>; })}</tr>)}</tbody></table></div> : <Empty>No hay rutas declaradas por los dispositivos.</Empty>)}</section>;
}
