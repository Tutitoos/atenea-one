// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-026, fase 6: «Administración» del propio equipo, visible solo si un administrador le concedió permisos en la red.
// La lectura sale del estado local del agente (sin diálogo); cada escritura es una llamada al CLI con la autorización de
// administrador del sistema, y la decisión final es siempre del servidor.
import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { adminApply, joinCodeReply, type AdminOp, type AdminParams } from './lib/networks';
import type { Status } from './lib/status';
import { button, CopyButton, Heading, type TabDef } from './ui';

/** Lista de filas libres: el `Card` común es una `<dl>` y aquí las filas llevan botones y texto, no pares término-definición. */
export const Card = ({ children }: { children: React.ReactNode }) => <div className="rounded-[10px] bg-card px-3.5">{children}</div>;

export type Admin = NonNullable<Status['admin']>;
export type Dev = NonNullable<Admin['devices']>[number];
type Pol = NonNullable<Admin['policies']>[number];

const protocols: Record<number, string> = { 0: 'cualquiera', 1: 'ICMP', 6: 'TCP', 17: 'UDP', 58: 'ICMPv6' };
export const scopeText = { enrolled: 'los dispositivos que este equipo da de alta, y él mismo', devices: 'una lista de dispositivos', network: 'toda la red' } as const;

/** Una escritura: pide la autorización del sistema, espera a que el agente relea el estado (hasta ~30 s) y avisa del resultado. */
export function useWrite(network: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ op, params }: { op: AdminOp; params: AdminParams }) => adminApply(op, network, params),
    onSettled: () => client.invalidateQueries({ queryKey: ['agent', network] }),
  });
}

export const nameOf = (devices: Dev[], id?: string | null) => devices.find((d) => d.id === id)?.name ?? (id ? `${id.slice(0, 8)}…` : '—');
const end = (devices: Dev[], s: Pol['src']) => (s.kind === 'device' ? nameOf(devices, s.device_id) : s.kind === 'prefix' ? (s.prefix ?? '—') : 'cualquiera');

function JoinCode({ network, allowed, stale }: { network: string; allowed: boolean; stale: boolean }) {
  const write = useWrite(network);
  const [ttl, setTtl] = useState(600);
  const reply = write.data ? joinCodeReply.safeParse(JSON.parse(write.data)) : null;
  return <section aria-label="Código de unión">
    <Heading>Código de unión</Heading>
    <p className="text-sm text-mut">Un solo uso. El equipo que lo use entra pendiente y se aprueba aquí, comparando su huella. El código se muestra una vez y no se guarda.</p>
    <div className="mt-2 flex items-center gap-2">
      <label className="text-sm">Caduca en <select value={ttl} onChange={(e) => setTtl(Number(e.target.value))} className="rounded border border-line bg-card px-1">
        <option value={600}>10 minutos</option><option value={3600}>1 hora</option></select></label>
      <button type="button" className={button} disabled={!allowed || stale || write.isPending} onClick={() => write.mutate({ op: 'join-code', params: { ttl } })}>Generar código</button>
    </div>
    {reply?.success && <div className="mt-2 rounded-md border border-line p-2">
      <code className="break-all select-text" aria-label="Código de unión generado">{reply.data.code}</code>
      <CopyButton value={reply.data.code} label="código de unión" />
      <p className="text-xs text-mut">Caduca el {new Date(reply.data.expires_at).toLocaleString('es')}.</p>
    </div>}
    {write.error && <p role="alert" className="text-bad">{write.error.message}</p>}
  </section>;
}

function Pending({ network, devices, allowed, stale }: { network: string; devices: Dev[]; allowed: boolean; stale: boolean }) {
  const write = useWrite(network);
  const [open, setOpen] = useState<string | null>(null);
  const pending = devices.filter((d) => d.status === 'pending' && d.in_scope);
  return <section aria-label="Pendientes">
    <Heading>Pendientes de aprobar</Heading>
    {pending.length === 0 ? <p className="text-sm text-mut">No hay dispositivos pendientes de este alcance.</p> : <Card>{pending.map((d) => <div key={d.id} className="border-b border-line py-2.5 last:border-b-0">
      <div className="flex items-center justify-between gap-3"><span>{d.name}</span>
        <button type="button" className={button} disabled={!allowed || stale} onClick={() => setOpen(open === d.id ? null : d.id)}>{open === d.id ? 'Cancelar' : 'Aprobar…'}</button></div>
      {open === d.id && <div className="mt-2">
        <p className="text-sm">Compara esta huella con la que muestra el equipo que se une. Si no coincide, no apruebes.</p>
        <code className="block break-all select-text" aria-label={`Huella de ${d.name}`}>{d.fingerprint}</code>
        <button type="button" className={`${button} mt-2`} disabled={write.isPending} onClick={() => write.mutate({ op: 'approve', params: { fingerprint: d.fingerprint } }, { onSuccess: () => setOpen(null) })}>Confirmar aprobación</button>
      </div>}
    </div>)}</Card>}
    {write.error && <p role="alert" className="text-bad">{write.error.message}</p>}
  </section>;
}

function Devices({ network, devices, allowed, stale }: { network: string; devices: Dev[]; allowed: boolean; stale: boolean }) {
  const write = useWrite(network);
  const [confirm, setConfirm] = useState<string | null>(null);
  return <section aria-label="Dispositivos de la red">
    <Heading>Dispositivos de la red</Heading>
    <Card>{devices.map((d) => <div key={d.id} className="flex flex-wrap items-center justify-between gap-2 border-b border-line py-2.5 last:border-b-0">
      <span>{d.name} <span className="text-xs text-mut">· {d.status === 'approved' ? 'aprobado' : d.status === 'pending' ? 'pendiente' : 'revocado'}{d.in_scope ? '' : ' · fuera de tu alcance'}</span></span>
      {allowed && d.in_scope && d.status !== 'revoked' && (confirm === d.id
        ? <span className="flex gap-2"><button type="button" className={button} disabled={stale || write.isPending} onClick={() => write.mutate({ op: 'revoke', params: { device: d.id } }, { onSuccess: () => setConfirm(null) })}>{`Confirmar revocación de ${d.name}`}</button>
          <button type="button" className={button} onClick={() => setConfirm(null)}>Cancelar</button></span>
        : <button type="button" className={button} disabled={stale} onClick={() => setConfirm(d.id)}>{`Revocar ${d.name}`}</button>)}
    </div>)}</Card>
    {write.error && <p role="alert" className="text-bad">{write.error.message}</p>}
  </section>;
}

function Policies({ network, devices, policies, canWrite, stale }: { network: string; devices: Dev[]; policies: Pol[]; canWrite: boolean; stale: boolean }) {
  const write = useWrite(network);
  const members = devices.filter((d) => d.in_scope && d.status === 'approved');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [proto, setProto] = useState('tcp');
  const [ports, setPorts] = useState('');
  const withPorts = proto === 'tcp' || proto === 'udp';
  const mine = (p: Pol) => !!p.created_by_device && [p.src, p.dst].every((s) => s.kind === 'device' && devices.some((d) => d.id === s.device_id && d.in_scope));
  return <section aria-label="Políticas">
    <Heading>Políticas</Heading>
    {policies.length === 0 ? <p className="text-sm text-mut">No hay políticas.</p> : <Card>{policies.map((p) => <div key={p.id} className="flex flex-wrap items-center justify-between gap-2 border-b border-line py-2.5 last:border-b-0">
      <span>{end(devices, p.src)} → {end(devices, p.dst)} · {protocols[p.proto] ?? p.proto}{p.proto === 6 || p.proto === 17 ? (p.port_min === 0 ? ' · todos los puertos' : ` · ${p.port_min === p.port_max ? p.port_min : `${p.port_min}–${p.port_max}`}`) : ''}
        <span className="text-xs text-mut">{p.created_by_device ? ` · creada por ${nameOf(devices, p.created_by_device)}` : ' · de un administrador'}</span></span>
      {canWrite && mine(p) && <button type="button" className={button} disabled={stale || write.isPending} onClick={() => write.mutate({ op: 'policy-remove', params: { policy: p.id } })}>{`Borrar política ${end(devices, p.src)} → ${end(devices, p.dst)}`}</button>}
    </div>)}</Card>}
    {canWrite && <form className="mt-3 flex flex-wrap items-end gap-2" onSubmit={(e) => { e.preventDefault(); write.mutate({ op: 'policy-add', params: { from, to, proto, ports: withPorts ? ports.trim() : '' } }); }}>
      <label className="text-sm">Origen <select required value={from} onChange={(e) => setFrom(e.target.value)} className="rounded border border-line bg-card px-1"><option value="">—</option>{members.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}</select></label>
      <label className="text-sm">Destino <select required value={to} onChange={(e) => setTo(e.target.value)} className="rounded border border-line bg-card px-1"><option value="">—</option>{members.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}</select></label>
      <label className="text-sm">Protocolo <select value={proto} onChange={(e) => setProto(e.target.value)} className="rounded border border-line bg-card px-1">
        <option value="tcp">TCP</option><option value="udp">UDP</option><option value="icmp">ICMP</option><option value="icmpv6">ICMPv6</option><option value="any">Cualquiera</option></select></label>
      {withPorts && <label className="text-sm">Puertos <input value={ports} onChange={(e) => setPorts(e.target.value)} placeholder="22 o 8000-8100" pattern="[0-9]{1,5}(-[0-9]{1,5})?" className="w-36 rounded border border-line bg-card px-1" /></label>}
      <button type="submit" className={button} disabled={stale || !from || !to || write.isPending}>Crear política</button>
      <p className="basis-full text-xs text-mut">Como toda política, también permite el sentido inverso: sin puertos, el tráfico de ida y vuelta; con puertos, el inverso no restringe puertos.</p>
    </form>}
    {write.error && <p role="alert" className="text-bad">{write.error.message}</p>}
  </section>;
}

/** Resumen de la concesión (en «General» del propio equipo); no pinta nada sin concesión. `canWrite` = la aplicación puede elevar en este sistema (no en Windows aún). */
export function AdminSummary({ status, canWrite }: { status: Status; canWrite: boolean }) {
  const admin = status.admin;
  if (!admin?.grant) return null;
  const { scopes, scope } = admin.grant;
  return <section aria-label="Administración">
    <Heading>Administración</Heading>
    <p className="text-sm text-mut">Un administrador concedió a este equipo permisos sobre la red. Alcance de las escrituras: {scopeText[scope.kind]}. Cada cambio pide la autorización de administrador del sistema y puede tardar hasta 30 s en verse.</p>
    <ul className="mt-1 text-sm" aria-label="Permisos concedidos">{scopes.map((s) => <li key={s}><code>{s}</code></li>)}</ul>
    {!canWrite && <p className="mt-1 text-sm text-mut">En este sistema la aplicación no puede hacer cambios: usa <code>atenea network</code> en una terminal con privilegios.</p>}
    {admin.last_error && <p className="mt-1 text-sm text-warn" role="status">La última lectura falló ({admin.last_error}); se muestra la anterior.</p>}
  </section>;
}

/** Una pestaña por función a la que da acceso la concesión; sin concesión, ninguna. */
export function adminTabs({ status, network, canWrite, stale }: { status: Status; network: string; canWrite: boolean; stale: boolean }): TabDef[] {
  const admin = status.admin;
  if (!admin?.grant) return [];
  const { scopes } = admin.grant;
  const may = (p: string) => canWrite && scopes.includes(p);
  const devices = admin.devices ?? [];
  const tabs: TabDef[] = [];
  if (scopes.includes('write:join-codes')) tabs.push({ id: 'code', label: 'Código de unión', panel: <JoinCode network={network} allowed={may('write:join-codes')} stale={stale} /> });
  if (scopes.includes('read:devices')) tabs.push({ id: 'devices', label: 'Dispositivos', panel: <>
    {scopes.includes('write:approve-devices') && <Pending network={network} devices={devices} allowed={may('write:approve-devices')} stale={stale} />}
    <Devices network={network} devices={devices} allowed={may('write:revoke-devices')} stale={stale} />
  </> });
  if (scopes.includes('read:policies')) tabs.push({ id: 'policies', label: 'Políticas', panel: <Policies network={network} devices={devices} policies={admin.policies ?? []} canWrite={may('write:policies') && scopes.includes('read:devices')} stale={stale} /> });
  return tabs;
}
