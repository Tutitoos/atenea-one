// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-027: pestaña «Permisos» del detalle de cada dispositivo. Muestra lo que consta y, si este equipo tiene `write:grants`,
// permite conceder, cambiar y retirar los permisos de un dispositivo de su alcance. La lectura sale del estado del agente;
// cada escritura es una llamada al CLI con la autorización de administrador del sistema y la decisión es del servidor.
import { useState } from 'react';
import { AdminSummary, Card, nameOf, scopeText, useWrite, type Admin, type Dev } from './admin';
import type { Status } from './lib/status';
import { button, Heading } from './ui';

const labels: Record<string, string> = {
  'read:devices': 'Ver el inventario de la red',
  'read:policies': 'Ver las políticas de la red',
  'write:join-codes': 'Generar códigos de unión',
  'write:approve-devices': 'Aprobar dispositivos (por huella)',
  'write:policies': 'Crear y borrar políticas entre dispositivos',
  'write:revoke-devices': 'Revocar dispositivos',
  'write:grants': 'Administrar los permisos de otros dispositivos',
};
const kindLabels = { devices: 'Una lista de dispositivos', enrolled: 'Los que el destinatario dé de alta, y él mismo', network: 'Toda la red' } as const;
type Kind = keyof typeof kindLabels;
type Entry = NonNullable<Admin['grants']>[number];

const Perms = ({ scopes }: { scopes: string[] }) => <ul className="mt-1 text-sm">{scopes.map((s) => <li key={s}>{labels[s] ?? s} <code className="text-xs text-gray-400">{s}</code></li>)}</ul>;

/** Propio equipo: su concesión y, con `write:grants`, lo que ha concedido a otros. */
export function OwnPermissions({ status, canWrite }: { status: Status; canWrite: boolean }) {
  const admin = status.admin;
  if (!admin?.grant) {
    return <section aria-label="Permisos">
      <Heading>Permisos de este equipo</Heading>
      <p className="text-sm text-gray-400">Este equipo no tiene permisos de administración en esta red. Los concede un administrador de la red desde el panel.</p>
    </section>;
  }
  const devices = admin.devices ?? [];
  const given = (admin.grants ?? []).filter((g) => g.granted_by_device && g.granted_by_device === admin.grant?.device_id);
  return <>
    <AdminSummary status={status} canWrite={canWrite} />
    {admin.grant.scopes.includes('write:grants') && <section aria-label="Permisos concedidos a otros">
      <Heading>Concedidos por este equipo</Heading>
      {given.length === 0 ? <p className="text-sm text-gray-400">Este equipo no ha concedido permisos a ningún otro. Se conceden desde la pestaña «Permisos» del dispositivo.</p> : <Card>{given.map((g) => <div key={g.device_id} className="border-b border-rule py-2.5 last:border-b-0">
        <span>{nameOf(devices, g.device_id)}</span> <span className="text-xs text-gray-400">· alcance: {scopeText[g.scope.kind]}</span>
        <Perms scopes={g.scopes} />
      </div>)}</Card>}
    </section>}
  </>;
}

/** Otro dispositivo: su concesión, si este equipo puede verla, y el formulario si puede cambiarla. */
export function PeerPermissions({ deviceId, status, network, canWrite, stale }: { deviceId?: string | null; status: Status; network: string; canWrite: boolean; stale: boolean }) {
  const admin = status.admin;
  const own = admin?.grant;
  if (!own?.scopes.includes('write:grants') || !deviceId) {
    return <section aria-label="Permisos">
      <Heading>Permisos</Heading>
      <p className="text-sm text-gray-400">Solo un administrador de la red, o un equipo con permiso para administrar permisos, puede ver y cambiar los de este dispositivo.</p>
    </section>;
  }
  const devices = admin?.devices ?? [];
  const entry = admin?.grants?.find((g) => g.device_id === deviceId);
  const known = devices.find((d) => d.id === deviceId);
  const mine = !!entry?.granted_by_device && entry.granted_by_device === own.device_id;
  const editable = canWrite && !stale && (!entry || mine) && known?.in_scope !== false && known?.status !== 'revoked';
  return <section aria-label="Permisos">
    <Heading>Permisos</Heading>
    {entry ? <>
      <p className="text-sm">Alcance de las escrituras: {scopeText[entry.scope.kind]}. {entry.granted_by_device ? `Concedidos por ${nameOf(devices, entry.granted_by_device)}.` : 'Concedidos por un administrador.'}</p>
      <Perms scopes={entry.scopes} />
    </> : <p className="text-sm text-gray-400">Este dispositivo no tiene permisos de administración en esta red.</p>}
    {known?.in_scope === false && <p className="mt-2 text-sm text-gray-400">Este dispositivo está fuera de tu alcance: no puedes cambiar sus permisos.</p>}
    {entry && !mine && <p className="mt-2 text-sm text-gray-400">Solo puedes cambiar o retirar los permisos que diste tú, no los de un administrador ni los de otro equipo.</p>}
    {!canWrite && <p className="mt-2 text-sm text-gray-400">En este sistema la aplicación no puede hacer cambios: usa <code>atenea network grant</code> en una terminal con privilegios.</p>}
    {editable && <GrantForm key={entry?.granted_at ?? 'nuevo'} network={network} target={deviceId} own={own} members={devices.filter((d) => d.in_scope && d.status === 'approved')} current={mine ? entry : undefined} />}
  </section>;
}

function GrantForm({ network, target, own, members, current }: { network: string; target: string; own: NonNullable<Admin['grant']>; members: Dev[]; current?: Entry }) {
  const write = useWrite(network);
  const offered = own.scopes.filter((p) => p !== 'write:grants');
  const kinds: Kind[] = own.scope.kind === 'network' ? ['devices', 'enrolled', 'network'] : ['devices'];
  const [perms, setPerms] = useState<string[]>(current?.scopes.filter((p) => offered.includes(p)) ?? []);
  const [kind, setKind] = useState<Kind>(current && kinds.includes(current.scope.kind) ? current.scope.kind : 'devices');
  const [picked, setPicked] = useState<string[]>(current?.scope.devices ?? []);
  const [confirm, setConfirm] = useState(false);
  const toggle = (list: string[], set: (l: string[]) => void, v: string) => set(list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
  const ready = perms.length > 0 && (kind !== 'devices' || picked.length > 0);
  return <div className="mt-3">
    <form onSubmit={(e) => {
      e.preventDefault();
      write.mutate({ op: 'grant', params: { device: target, permissions: offered.filter((p) => perms.includes(p)), scope: kind, devices: kind === 'devices' ? picked : undefined } });
    }}>
      <fieldset className="border-0 p-0">
        <legend className="text-sm font-semibold">{current ? 'Cambiar permisos' : 'Conceder permisos'}</legend>
        <p className="text-xs text-gray-400">Solo los que este equipo tiene. «Administrar permisos» no se puede dar.</p>
        {offered.map((p) => <label key={p} className="block text-sm"><input type="checkbox" checked={perms.includes(p)} onChange={() => toggle(perms, setPerms, p)} /> {labels[p] ?? p}</label>)}
      </fieldset>
      <label className="mt-2 block text-sm">Alcance de las escrituras <select value={kind} onChange={(e) => setKind(e.target.value as Kind)} className="border border-rule bg-panel px-1">
        {kinds.map((k) => <option key={k} value={k}>{kindLabels[k]}</option>)}</select></label>
      {kind === 'devices' && <fieldset className="mt-1 border-0 p-0">
        <legend className="text-sm">Dispositivos del alcance</legend>
        {members.length === 0 ? <p className="text-xs text-gray-400">Sin lista de dispositivos de tu alcance: hace falta el permiso <code>read:devices</code>.</p>
          : members.map((d) => <label key={d.id} className="block text-sm"><input type="checkbox" checked={picked.includes(d.id)} onChange={() => toggle(picked, setPicked, d.id)} /> {d.name}</label>)}
      </fieldset>}
      <button type="submit" className={`${button} mt-2`} disabled={!ready || write.isPending}>Guardar permisos</button>
    </form>
    {current && (confirm
      ? <span className="mt-2 flex gap-2"><button type="button" className={button} disabled={write.isPending} onClick={() => write.mutate({ op: 'ungrant', params: { device: target } }, { onSuccess: () => setConfirm(false) })}>Confirmar retirada de permisos</button>
        <button type="button" className={button} onClick={() => setConfirm(false)}>Cancelar</button></span>
      : <button type="button" className={`${button} mt-2`} onClick={() => setConfirm(true)}>Retirar permisos</button>)}
    {write.error && <p role="alert" className="text-no">{write.error.message}</p>}
  </div>;
}
