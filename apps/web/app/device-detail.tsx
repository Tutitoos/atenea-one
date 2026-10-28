// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { api, type Model } from '@atenea/api-client';
import { ActionForm, ConfirmDialog, CopyButton, Dialog, RunButton, ShortId } from './ui';
import { endpointsSchema, nameSchema } from './domain';

export function DeviceDetail({ device, devices, net, elapsed, routes, canEdit, refresh }: {
  device: Model<'Device'>; devices: Model<'Device'>[]; net: string; elapsed: number;
  routes: Model<'Route'>[]; canEdit: boolean; refresh: () => Promise<unknown>;
}) {
  const report = device.report;
  const params = { path: { net, dev: device.id } };
  return <section className="devices-detail" aria-label={`Detalle de ${device.name}`}>
    <h3>{device.name}</h3>
    <dl className="devices-metadata">
      <div><dt>ID</dt><dd><ShortId value={device.id}/></dd></div>
      <div><dt>Hostname</dt><dd>{device.hostname ?? 'Sin nombre del sistema comunicado'}</dd></div>
      <div><dt>Origen del nombre</dt><dd>{device.name_source === 'admin' ? 'administrativo' : 'del sistema'}</dd></div>
      <div><dt>Endpoints</dt><dd>{device.endpoints.join(', ') || 'Sin endpoints'}</dd></div>
    </dl>
    <p className="devices-fingerprint"><strong>Huella completa: </strong><code>{device.fingerprint}</code>
      <CopyButton value={device.fingerprint} label={`Copiar huella del detalle de ${device.name}`}/></p>
    {report ? <>
      <h4>Rutas, DNS y forwarding declarados</h4>
      <p>Forwarding: {report.forwarding_ok == null ? 'desconocido' : report.forwarding_ok ? 'correcto' : 'desactivado'}</p>
      <p>DNS: {report.dns ?? 'desconocido'}</p>
      {report.forwarding_ok === false && routes.some(route => route.device_id === device.id && route.status === 'approved') &&
        <p className="warning" role="status">Este router tiene rutas aprobadas pero declara forwarding desactivado; no puede reenviar tráfico de la LAN.</p>}
      {report.routes?.map(route => <p key={route.prefix}><code>{route.prefix}</code>: {' '}
        {route.state === 'installed' ? 'instalada' : route.state === 'conflict' ? 'conflicto' : 'pendiente'}
        {route.reason && ` · ${route.reason}`}</p>)}
      {!report.routes?.length && <p>Sin rutas informadas</p>}
      <small>Declarado por el agente hace {report.age_secs + elapsed} s; no verificado independientemente.</small>
      <h4>Caminos declarados / antigüedad</h4>
      {report.paths.map(path => <div key={path.peer}>
        {devices.find(peer => peer.id === path.peer)?.name ?? 'Par'} <ShortId value={path.peer}/>: {path.path}
        {' · último RX '}{path.last_rx_secs == null ? 'desconocido' : `${path.last_rx_secs + report.age_secs + elapsed} s`}
        {path.rtt_ms != null && ` · RTT ${path.rtt_ms} ms`}
      </div>)}
      {!report.paths.length && <p>Sin caminos informados</p>}
      <small>Informe hace {report.age_secs + elapsed} s</small>
    </> : <p>Sin informe del agente</p>}
    {canEdit ? <div className="actions devices-actions">
      <Dialog trigger="Editar" title={`Editar ${device.name}`}
        description="Los endpoints son direcciones públicas declaradas por el administrador; máximo cuatro.">
        <ActionForm label="Guardar dispositivo" fields={[
          { name: 'name', label: 'Nombre', value: device.name,
            help: 'Si cambias el nombre, será administrativo y dejará de seguir el nombre del sistema.' },
          { name: 'endpoints', label: 'Endpoints separados por coma', value: device.endpoints.join(', '), required: false },
        ]} submit={async values => {
          const body: Model<'DeviceUpdate'> = {};
          if (values.name !== device.name) body.name = nameSchema.parse(values.name);
          const endpoints = endpointsSchema.parse(values.endpoints.trim()
            ? values.endpoints.split(',').map(value => value.trim()) : []);
          if (JSON.stringify(endpoints) !== JSON.stringify(device.endpoints)) body.endpoints = endpoints;
          if (!Object.keys(body).length) return;
          await api.PATCH('/api/v1/networks/{net}/devices/{dev}', { params, body });
          await refresh();
        }}/>
      </Dialog>
      <Dialog trigger="Cambiar tipo" title={`Cambiar tipo de ${device.name}`}
        description="El tipo solo afecta a la presentación, no concede permisos ni forma parte de la configuración firmada.">
        <p>{device.role === 'server' ? 'Servidor → Dispositivo' : 'Dispositivo → Servidor'}</p>
        <RunButton run={async () => {
          await api.PATCH('/api/v1/networks/{net}/devices/{dev}', {
            params, body: { role: device.role === 'server' ? 'device' : 'server' },
          });
          await refresh();
        }}>Confirmar cambio de tipo</RunButton>
      </Dialog>
      {device.name_source === 'admin' && (device.hostname ?
        <ConfirmDialog trigger="Volver al nombre del sistema" title={`Usar el nombre del sistema de ${device.name}`}
          description={`El nombre volverá a ser «${device.hostname}» y seguirá los cambios comunicados por el sistema.`}
          label="Confirmar nombre del sistema" confirm={async () => {
            await api.PATCH('/api/v1/networks/{net}/devices/{dev}', { params, body: { use_system_name: true } });
            await refresh();
          }}/>
        : <p>Para volver al nombre del sistema, el agente debe comunicar primero su hostname.</p>)}
      {device.status === 'approved' && <ConfirmDialog trigger="Revocar" title={`Revocar ${device.name}`}
        description="El dispositivo dejará de recibir configuración y se liberarán sus direcciones. No se puede deshacer."
        label="Confirmar revocación" confirm={async () => {
          await api.POST('/api/v1/networks/{net}/devices/{dev}/revoke', { params });
          await refresh();
        }}/>}
    </div> : <p>Solo lectura</p>}
  </section>;
}
