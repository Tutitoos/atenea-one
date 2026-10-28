// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, result, type Model } from '@atenea/api-client';
import { Button, ConfirmDialog, Dialog, EmptyState, Field, ShortId, Status } from './ui';
import { policySchema, selector } from './domain';

const policyNotice = 'Las políticas son efectivamente bidireccionales: la regla inversa mantiene el protocolo, pero no restringe puertos. Una política A→B tcp/22 permite también a B iniciar tráfico TCP a A en cualquier puerto; no se limita a respuestas de conexiones iniciadas por A.';
const protocols: Record<number, string> = { 0: 'Cualquiera', 1: 'ICMP', 6: 'TCP', 17: 'UDP', 58: 'ICMPv6' };
function describePorts(proto: number, min: number, max: number) {
  if (proto === 0 || min === 0 && max === 0) return 'Todos los puertos';
  return min === max ? `Puerto ${min}` : `Puertos ${min}–${max}`;
}
type Selection = { kind: string; value: string };
const toSelector = ({ kind, value }: Selection) => kind === 'device' ?
  { kind, device_id: value } : kind === 'prefix' ? { kind, prefix: value } : { kind: 'any' };

function SelectorFields({ label, selection, change, devices, error }: {
  label: string; selection: Selection; change: (value: Selection) => void;
  devices: Model<'Device'>[]; error?: string;
}) {
  return <fieldset><legend>{label === 'origen' ? 'Origen' : 'Destino'}</legend>
    <Field label={`Selector ${label}`}>{props => <select {...props} value={selection.kind}
      onChange={event => change({ kind: event.target.value, value: '' })}>
      <option value="any">Cualquiera</option><option value="device">Dispositivo</option>
      <option value="prefix">Prefijo</option>
    </select>}</Field>
    {selection.kind === 'device' && <Field label={`Dispositivo de ${label}`} error={error}>
      {props => <select {...props} value={selection.value}
        onChange={event => change({ ...selection, value: event.target.value })}>
        <option value="">Selecciona un dispositivo</option>
        {devices.map(device => <option key={device.id} value={device.id}>{device.name}</option>)}
      </select>}
    </Field>}
    {selection.kind === 'prefix' && <Field label={`Prefijo de ${label}`} error={error}
      help="CIDR de red sin bits de host, por ejemplo 10.0.0.0/24 o fd00::/64. Debe pertenecer a la red.">
      {props => <input {...props} value={selection.value}
        onChange={event => change({ ...selection, value: event.target.value })}/>}
    </Field>}
  </fieldset>;
}

function PolicyForm({ devices, save }: {
  devices: Model<'Device'>[]; save: (body: {
    src: Model<'Selector'>; dst: Model<'Selector'>; proto: 0 | 1 | 6 | 17 | 58;
    port_min: number; port_max: number;
  }) => Promise<unknown>;
}) {
  const [src, setSrc] = useState<Selection>({ kind: 'any', value: '' });
  const [dst, setDst] = useState<Selection>({ kind: 'any', value: '' });
  const [proto, setProto] = useState(0);
  const [min, setMin] = useState('0');
  const [max, setMax] = useState('0');
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [review, setReview] = useState(false);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState('');
  const body = { src: toSelector(src), dst: toSelector(dst), proto,
    port_min: Number(min), port_max: Number(max) };
  function validate() {
    const next: Record<string, string> = {};
    for (const [key, value] of [['src', src], ['dst', dst]] as const) {
      if (!selector.safeParse(toSelector(value)).success) {
        next[key] = value.kind === 'device' ? 'Selecciona un dispositivo.' : 'Indica un CIDR de red válido, sin bits de host.';
      }
    }
    if (!min || !max || !policySchema.safeParse(body).success && !next.src && !next.dst) {
      next.ports = 'Usa puertos enteros de 0 a 65535, en orden; solo TCP y UDP admiten puertos. 0–0 no restringe.';
    }
    setErrors(next);
    return !Object.keys(next).length;
  }
  return <form className="action-form" onChange={() => setReview(false)} onSubmit={event => {
    event.preventDefault(); setMessage(''); if (validate()) setReview(true);
  }}>
    <SelectorFields label="origen" selection={src} change={setSrc} devices={devices} error={errors.src}/>
    <SelectorFields label="destino" selection={dst} change={setDst} devices={devices} error={errors.dst}/>
    <Field label="Protocolo">{props => <select {...props} value={proto} onChange={event => {
      const value = Number(event.target.value); setProto(value);
      if (![6, 17].includes(value)) { setMin('0'); setMax('0'); }
    }}>{Object.entries(protocols).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>}</Field>
    <Field label="Puerto mínimo" error={errors.ports} help="Solo TCP/UDP; 0–0 permite cualquier puerto.">
      {props => <input {...props} type="number" min="0" max="65535" step="1" value={min}
        disabled={![6, 17].includes(proto)} onChange={event => setMin(event.target.value)}/>}
    </Field>
    <Field label="Puerto máximo" error={errors.ports} help="Debe ser igual o mayor que el mínimo.">
      {props => <input {...props} type="number" min="0" max="65535" step="1" value={max}
        disabled={![6, 17].includes(proto)} onChange={event => setMax(event.target.value)}/>}
    </Field>
    <Button type="submit" variant="primary">Revisar política</Button>
    {review && <div className="warning"><p>¿Crear esta política {protocols[proto]} · {describePorts(proto, Number(min), Number(max))}? Se aplicará en ambos sentidos.</p>
      <Button variant="primary" loading={pending} onClick={() => {
        setPending(true);
        void save(policySchema.parse(body)).then(() => {
          setMessage('Política creada.'); setReview(false);
        }, error => setMessage(error instanceof Error ? error.message : 'No se pudo crear la política.'))
          .finally(() => setPending(false));
      }}>Confirmar creación</Button>
    </div>}
    <p role="status">{message}</p>
  </form>;
}

export function Policies({ net, canEdit, devices = [] }: {
  net: string; canEdit: boolean; devices?: Model<'Device'>[];
}) {
  const params = { path: { net } };
  const query = useQuery({ queryKey: ['network', net, 'policies'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/policies', { params })) });
  const describe = (value: Model<'Selector'>) => value.kind === 'any' ? 'Cualquiera' :
    value.kind === 'prefix' ? value.prefix : value.device_id ?
      devices.find(device => device.id === value.device_id)?.name ?? <ShortId value={value.device_id}/> : 'Dispositivo desconocido';
  const create = canEdit && <Dialog trigger="Crear política" title="Crear política" description={policyNotice}>
    <PolicyForm devices={devices} save={async body => {
      await api.POST('/api/v1/networks/{net}/policies', { params, body }); await query.refetch();
    }}/>
  </Dialog>;
  return <section className="policies-page"><h2>Políticas</h2>
    <p>Los prefijos deben estar dentro de los CIDR de la red; el servidor comprueba pertenencia y límites.</p>
    <aside className="warning" role="note" aria-label="Bidireccionalidad de las políticas">
      <strong>Reglas bidireccionales · ACL sin estado</strong><p>{policyNotice}</p>
    </aside>
    <Status query={query}/>{create}
    {query.data?.length ? <ul className="cards">{query.data.map(policy => <li className="card" key={policy.id}>
      <p>{describe(policy.src)} → {describe(policy.dst)}</p>
      <p>{protocols[policy.proto] ?? policy.proto} · {describePorts(policy.proto, policy.port_min, policy.port_max)}</p>
      {canEdit && <ConfirmDialog trigger="Borrar" title="Borrar política"
        description="Los pares afectados dejarán de comunicarse al renovar la configuración."
        label="Confirmar borrado" confirm={async () => {
          await api.DELETE('/api/v1/networks/{net}/policies/{id}', { params: { path: { net, id: policy.id } } });
          await query.refetch();
        }}/>}
    </li>)}</ul> : !query.isPending && !query.isError ?
      <EmptyState message="Sin políticas no hay pares: los dispositivos no pueden comunicarse. Crea la primera política con «Crear política» para permitir tráfico." action={null}/> : null}
  </section>;
}
