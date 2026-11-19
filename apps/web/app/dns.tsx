// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, result, type Model } from '@atenea/api-client';
import { dnsSchema } from './domain';
import { Button, Card, Field, RunButton, Status, StatusBadge } from './ui';

type Kind = keyof Model<'Dns'>;
const copy = {
  servers: {
    title: 'Servidores', add: 'Añadir servidor DNS', button: 'Añadir servidor', placeholder: '10.42.0.53', empty: 'Sin servidores.',
    help: 'Una dirección IP del túnel o de una ruta aprobada. Máximo 3.', invalid: 'Indica una dirección IP válida y sin repetir; máximo 3.',
  },
  domains: {
    title: 'Dominios', add: 'Añadir dominio', button: 'Añadir dominio', placeholder: 'servicios.example.test', empty: 'Sin dominios.',
    help: 'En minúsculas y sin punto final. Máximo 8.', invalid: 'Indica un dominio LDH en minúsculas, sin repetir ni punto final; máximo 8.',
  },
} as const;

/** Una fila por valor, como una lista de servidores de nombres; `change` (solo quien edita) envía la lista entera ya modificada. */
function Rows({ kind, data, change }: { kind: Kind; data: Model<'Dns'>; change?: (next: Model<'Dns'>) => Promise<string | undefined> }) {
  const text = copy[kind];
  const [draft, setDraft] = useState('');
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  const send = async (values: string[]) => {
    setPending(true); setError(undefined);
    const failure = await change!({ ...data, [kind]: values });
    setPending(false); setError(failure);
    return !failure;
  };
  return <section aria-label={text.title} className="dns-group"><h3>{text.title}</h3>
    {data[kind].length ? <ul className="rows">{data[kind].map(value => <li key={value}>
      <code>{value}</code>{kind === 'domains' && <StatusBadge>DNS dividido</StatusBadge>}
      {change && <Button variant="link" disabled={pending} aria-label={`Quitar ${value}`}
        onClick={() => void send(data[kind].filter(item => item !== value))}>Quitar</Button>}
    </li>)}</ul> : <p className="muted">{text.empty}</p>}
    {change && <form className="row-add" noValidate onSubmit={event => {
      event.preventDefault();
      if (draft.trim()) void send([...data[kind], draft.trim()]).then(done => { if (done) setDraft(''); });
    }}>
      <Field label={text.add} help={text.help} error={error}>{props =>
        <input {...props} placeholder={text.placeholder} value={draft} onChange={event => setDraft(event.target.value)}/>}</Field>
      <Button type="submit" loading={pending}>{text.button}</Button>
    </form>}
  </section>;
}

export function Dns({ net, canEdit }: { net: string; canEdit: boolean }) {
  const cache = useQueryClient();
  const [message, setMessage] = useState('');
  const query = useQuery({ queryKey: ['network', net, 'dns'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/dns', { params: { path: { net } } })) });
  const change = (kind: Kind) => async (next: Model<'Dns'>) => {
    setMessage('');
    const parsed = dnsSchema.safeParse(next);
    if (!parsed.success) return copy[kind].invalid;
    try {
      await api.PUT('/api/v1/networks/{net}/dns', { params: { path: { net } }, body: parsed.data });
      await cache.invalidateQueries({ queryKey: ['network', net] });
      setMessage('DNS actualizado.');
    } catch (cause) { return cause instanceof Error ? cause.message : 'No se pudo guardar el DNS.'; }
  };
  return <Card title="DNS">
    <p>Los equipos de la red consultan los servidores de abajo solo para los dominios indicados. Cada cambio se guarda al momento.
      El agente necesita systemd-resolved; si no está disponible, informa unsupported y no modifica resolv.conf.</p>
    <ul className="rows" aria-label="DNS general"><li>
      <span>DNS general</span><span className="muted">El de cada equipo. Atenea One no lo cambia ni lo captura.</span>
      <svg className="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" role="img" aria-label="No se puede cambiar">
        <rect x="5" y="11" width="14" height="9"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>
    </li></ul>
    <Status query={query}/>{query.isError && <RunButton run={() => query.refetch()}>Reintentar DNS</RunButton>}
    {query.data && (['servers', 'domains'] as const).map(kind =>
      <Rows key={kind} kind={kind} data={query.data} change={canEdit ? change(kind) : undefined}/>)}
    <p role="status">{message}</p>
  </Card>;
}
