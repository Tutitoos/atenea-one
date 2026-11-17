// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-026, fase 5: permisos de administración delegada de un dispositivo.
import { useState } from 'react';
import { api, type Model } from '@atenea/api-client';
import { Button, ConfirmDialog, Dialog, ShortId } from './ui';

type Permission = Model<'DeviceGrantPermission'>;
type ScopeKind = Model<'DeviceGrantScope'>['kind'];

export const permissionLabels: Record<Permission, string> = {
  'read:devices': 'Ver el inventario de la red',
  'read:policies': 'Ver las políticas de la red',
  'write:join-codes': 'Generar códigos de unión',
  'write:approve-devices': 'Aprobar dispositivos (por huella)',
  'write:policies': 'Crear y borrar políticas entre dispositivos',
  'write:revoke-devices': 'Revocar dispositivos',
};
const permissions = Object.keys(permissionLabels) as Permission[];
const scopeLabels: Record<ScopeKind, string> = {
  enrolled: 'Los dispositivos que él da de alta, y él mismo',
  devices: 'Una lista de dispositivos',
  network: 'Toda la red',
};

/** Nombre de un dispositivo de la red, o su identificador abreviado si ya no está en el inventario. */
function deviceName(devices: Model<'Device'>[], id: string) {
  return devices.find(item => item.id === id)?.name ?? <ShortId value={id}/>;
}

function GrantForm({ device, devices, current, save }: {
  device: Model<'Device'>; devices: Model<'Device'>[]; current?: Model<'DeviceGrant'>;
  save: (body: Model<'DeviceGrantInput'>) => Promise<unknown>;
}) {
  const [chosen, setChosen] = useState<Permission[]>(current?.scopes ?? ['read:devices']);
  const [kind, setKind] = useState<ScopeKind>(current?.scope.kind ?? 'enrolled');
  const [members, setMembers] = useState<string[]>(current?.scope.devices ?? []);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState('');
  const toggle = <T extends string>(list: T[], value: T) => list.includes(value) ? list.filter(item => item !== value) : [...list, value];
  const candidates = devices.filter(item => item.status !== 'revoked');
  const error = !chosen.length ? 'Elige al menos un permiso.'
    : kind === 'devices' && !members.length ? 'Elige al menos un dispositivo.'
    : kind === 'devices' && members.length > 64 ? 'Como máximo 64 dispositivos.' : '';
  return <form className="action-form" onSubmit={event => {
    event.preventDefault();
    if (error) { setMessage(error); return; }
    setPending(true); setMessage('');
    void save({ scopes: chosen, scope: kind === 'devices' ? { kind, devices: members } : { kind } })
      .then(() => setMessage('Permisos guardados.'), failure => setMessage(failure instanceof Error ? failure.message : 'No se pudo guardar.'))
      .finally(() => setPending(false));
  }}>
    <fieldset><legend>Permisos de {device.name}</legend>
      {permissions.map(permission => <label key={permission}>
        <input type="checkbox" checked={chosen.includes(permission)} onChange={() => setChosen(toggle(chosen, permission))}/>
        {' '}{permissionLabels[permission]} <code>{permission}</code></label>)}
    </fieldset>
    <fieldset><legend>Alcance de las escrituras</legend>
      {(Object.keys(scopeLabels) as ScopeKind[]).map(option => <label key={option}>
        <input type="radio" name="scope-kind" checked={kind === option} onChange={() => setKind(option)}/>
        {' '}{scopeLabels[option]}</label>)}
      <small>Las lecturas cubren siempre toda la red. Las políticas delegadas solo conectan dispositivos del alcance entre sí; nunca «cualquiera» ni prefijos.</small>
    </fieldset>
    {kind === 'devices' && <fieldset><legend>Dispositivos del alcance</legend>
      {candidates.map(item => <label key={item.id}>
        <input type="checkbox" checked={members.includes(item.id)} onChange={() => setMembers(toggle(members, item.id))}/>
        {' '}{item.name}</label>)}
    </fieldset>}
    {kind === 'network' && <p className="warning" role="note">Con alcance «Toda la red», este dispositivo podrá actuar sobre cualquier dispositivo de la red, también los de producción.</p>}
    <Button type="submit" variant="primary" loading={pending}>Guardar permisos</Button>
    <p role="status">{message}</p>
  </form>;
}

/** Concesión de administración delegada del dispositivo (ADR-026). `grants` ausente: el servidor no las ofrece. */
export function Delegation({ device, devices, net, grants, canEdit, refresh }: {
  device: Model<'Device'>; devices: Model<'Device'>[]; net: string;
  grants?: Model<'DeviceGrant'>[]; canEdit: boolean; refresh: () => Promise<unknown>;
}) {
  if (!grants || device.status !== 'approved') return null;
  const grant = grants.find(item => item.device_id === device.id);
  const params = { path: { net, dev: device.id } };
  return <>
    <h4>Administración delegada</h4>
    {grant ? <>
      <ul aria-label={`Permisos de ${device.name}`}>{grant.scopes.map(scope => <li key={scope}>{permissionLabels[scope]} <code>{scope}</code></li>)}</ul>
      <p>Alcance: {scopeLabels[grant.scope.kind]}
        {grant.scope.kind === 'devices' && <> ({grant.scope.devices.map((id, index) => <span key={id}>{index > 0 && ', '}{deviceName(devices, id)}</span>)})</>}.</p>
      <small>Concedido por un administrador (<ShortId value={grant.granted_by}/>) el <time dateTime={grant.granted_at}>{new Date(grant.granted_at).toLocaleString('es')}</time>.
        Se retira al revocar el dispositivo.</small>
    </> : <p>Sin permisos de administración: solo actúa sobre sí mismo.</p>}
    {canEdit ? <div className="actions">
      <Dialog trigger={grant ? 'Cambiar permisos' : 'Conceder permisos'} title={`Permisos de administración de ${device.name}`}
        description="Un administrador concede estos permisos; el dispositivo los ejerce desde la CLI. Nunca puede conceder ni ampliar permisos, cambiar DNS ni aprobar rutas.">
        <GrantForm device={device} devices={devices} current={grant} save={async body => {
          const response = await api.PUT('/api/v1/networks/{net}/devices/{dev}/grant', { params, body });
          if (response.error) throw new Error('El servidor rechazó la concesión.');
          await refresh();
        }}/>
      </Dialog>
      {grant && <ConfirmDialog trigger="Retirar permisos" title={`Retirar los permisos de ${device.name}`}
        description="Dejará de poder administrar la red. Lo que ya creó se conserva y lo administra un administrador."
        label="Confirmar retirada" confirm={async () => {
          await api.DELETE('/api/v1/networks/{net}/devices/{dev}/grant', { params });
          await refresh();
        }}/>}
    </div> : null}
  </>;
}

/** Quién dio de alta un dispositivo o creó una política: un dispositivo delegado, o «administrador». */
export function Author({ id, devices, none = 'administrador o solicitud' }: { id: string | null; devices: Model<'Device'>[]; none?: string }) {
  return id ? <>{deviceName(devices, id)} (delegado)</> : <>{none}</>;
}
