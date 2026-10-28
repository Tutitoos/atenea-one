// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useId, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, ConfirmDialog, Dialog, Empty, RunButton, ShortId, Status, StatusBadge } from './ui';

function Approval({ route, device, net, onApproved }: {
  route: Model<'Route'>; device: string; net: string; onApproved: () => Promise<unknown>;
}) {
  const snatLabel = useId();
  const [snat, setSnat] = useState('');
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState('');
  return <><p><strong>{device}</strong> · <code>{route.prefix}</code></p>
    <fieldset className="snat-options" role="radiogroup" aria-labelledby={snatLabel} disabled={pending}>
      <legend id={snatLabel}>Elige explícitamente el modo SNAT</legend>
      <label><input type="radio" name={`snat-${route.id}`} value="false" checked={snat === 'false'}
        onChange={event => setSnat(event.target.value)}/>
        <span>Sin SNAT: exige rutas de retorno en la LAN y conserva el origen real del cliente.</span>
      </label>
      <label><input type="radio" name={`snat-${route.id}`} value="true" checked={snat === 'true'}
        onChange={event => setSnat(event.target.value)}/>
        <span>Con SNAT: enmascara el tráfico sin tocar la LAN, pero se pierde el origen real.</span>
      </label>
    </fieldset>
    <Button variant="primary" disabled={!snat} loading={pending} onClick={() => {
      if (!snat || pending) return;
      setPending(true); setMessage('');
      void api.POST('/api/v1/networks/{net}/routes/{id}/approve', {
        params: { path: { net, id: route.id } }, body: { snat: snat === 'true' },
      }).then(onApproved, error => {
        setMessage(error instanceof ApiError && error.status === 409 && error.detail.error.code === 'route_overlap' ?
          'No se puede aprobar: el prefijo se solapa con la red del túnel u otra ruta aprobada. Revisa las subredes antes de volver a intentarlo.' :
          error instanceof Error ? error.message : 'No se pudo aprobar la ruta.');
      }).catch(error => setMessage(error instanceof Error ? error.message : 'No se pudo actualizar la lista.'))
        .finally(() => setPending(false));
    }}>Aprobar ruta</Button><p role="status">{message}</p>
  </>;
}

export function Subnets({ net, canEdit, devices }: {
  net: string; canEdit: boolean; devices: Model<'Device'>[];
}) {
  const cache = useQueryClient();
  const [message, setMessage] = useState('');
  const query = useQuery({ queryKey: ['network', net, 'routes'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/routes', { params: { path: { net } } })) });
  const refresh = () => cache.invalidateQueries({ queryKey: ['network', net] });
  return <section aria-label="Subredes"><h2>Subredes</h2>
    <p className="warning">Solo se sirven prefijos privados (RFC 1918, CGNAT y ULA). No hay salida a Internet ni ruta por defecto. Las rutas las declara el agente y requieren aprobación de owner o admin.</p>
    <Status query={query}/>{query.isError && <RunButton run={() => query.refetch()}>Reintentar rutas</RunButton>}
    <p role="status">{message}</p>
    {query.data && (query.data.length ? <div className="table-scroll"><table>
      <caption>Rutas declaradas por los dispositivos</caption>
      <thead><tr>{['Dispositivo', 'Prefijo', 'Estado', 'SNAT', 'Fecha de declaración', 'Fecha de decisión', 'Acciones']
        .map(label => <th scope="col" key={label}>{label}</th>)}</tr></thead>
      <tbody>{query.data.map(route => {
        const name = devices.find(device => device.id === route.device_id)?.name ?? 'Dispositivo desconocido';
        const approved = route.status === 'approved';
        return <tr key={route.id}>
          <td>{name}<br/><ShortId value={route.device_id}/></td><td><code>{route.prefix}</code></td>
          <td><StatusBadge tone={approved ? 'ok' : route.status === 'pending' ? 'warn' : 'bad'}>
            {approved ? 'Aprobada' : route.status === 'pending' ? 'Pendiente' : 'Rechazada'}
          </StatusBadge></td>
          <td>{route.snat === null ? 'Pendiente de decidir' : route.snat ? 'Aplicado' : 'No aplicado'}</td>
          <td><time dateTime={route.declared_at}>{new Date(route.declared_at).toLocaleString('es')}</time></td>
          <td>{route.decided_at ? <time dateTime={route.decided_at}>
            {new Date(route.decided_at).toLocaleString('es')}
          </time> : 'Sin decisión'}</td>
          <td>{!canEdit ? 'Solo lectura' : <div className="actions">
            {!approved && <Dialog trigger="Aprobar" title="Aprobar ruta de subred"
              description="La aprobación autoriza este prefijo; elige el tratamiento del origen antes de confirmar.">
              <Approval route={route} device={name} net={net} onApproved={async () => {
                await refresh(); setMessage('Ruta aprobada.');
              }}/>
            </Dialog>}
            {route.status !== 'rejected' && <ConfirmDialog trigger={approved ? 'Revocar' : 'Rechazar'}
              title={approved ? 'Revocar ruta' : 'Rechazar ruta'}
              description={`${name} · ${route.prefix}. El prefijo dejará de estar autorizado. Las políticas que dependan de él dejarán de seleccionar esta ruta.`}
              label={approved ? 'Confirmar revocación' : 'Confirmar rechazo'} confirm={async () => {
                await api.POST('/api/v1/networks/{net}/routes/{id}/reject', {
                  params: { path: { net, id: route.id } },
                }); await refresh();
              }}/>}
          </div>}</td>
        </tr>;
      })}</tbody>
    </table></div> : <Empty>No hay rutas declaradas por los dispositivos.</Empty>)}
  </section>;
}
