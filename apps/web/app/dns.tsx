// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, result, type Model } from '@atenea/api-client';
import { dnsSchema } from './domain';
import { Button, Card, Field, RunButton, Status } from './ui';

function DnsForm({ net, data }: { net: string; data: Model<'Dns'> }) {
  const cache = useQueryClient();
  const [servers, setServers] = useState(data.servers.join(' '));
  const [domains, setDomains] = useState(data.domains.join(' '));
  const [errors, setErrors] = useState<{ servers?: string; domains?: string }>({});
  const [message, setMessage] = useState('');
  const [pending, setPending] = useState(false);
  return <form className="incorporar-form" noValidate onSubmit={async event => {
    event.preventDefault(); setErrors({}); setMessage('');
    const parsed = dnsSchema.safeParse({
      servers: servers.trim() ? servers.trim().split(/\s+/) : [],
      domains: domains.trim() ? domains.trim().split(/\s+/) : [],
    });
    if (!parsed.success) {
      const next: typeof errors = {};
      for (const issue of parsed.error.issues) {
        if (issue.path[0] === 'servers') next.servers = 'Indica hasta 3 direcciones IP válidas, sin repetir.';
        if (issue.path[0] === 'domains') next.domains = 'Indica hasta 8 dominios LDH en minúsculas, sin repetir ni punto final.';
      }
      setErrors(next); return;
    }
    setPending(true);
    try {
      await api.PUT('/api/v1/networks/{net}/dns', { params: { path: { net } }, body: parsed.data });
      await cache.invalidateQueries({ queryKey: ['network', net] });
      setMessage('DNS actualizado.');
    } catch (cause) { setMessage(cause instanceof Error ? cause.message : 'No se pudo guardar el DNS.'); }
    finally { setPending(false); }
  }}>
    <Field label="Servidores DNS (separados por espacios)" help="Ejemplo sintético: 10.42.0.53. Deben estar en el túnel o en una ruta aprobada."
      error={errors.servers}>{props => <input {...props} value={servers} onChange={event => setServers(event.target.value)}/>}</Field>
    <Field label="Dominios de enrutamiento (separados por espacios)" help="Ejemplo sintético: servicios.example.test. Sin punto final ni dominio raíz."
      error={errors.domains}>{props => <input {...props} value={domains} onChange={event => setDomains(event.target.value)}/>}</Field>
    <Button type="submit" variant="primary" loading={pending}>Guardar DNS</Button><p role="status">{message}</p>
  </form>;
}
export function Dns({ net, canEdit }: { net: string; canEdit: boolean }) {
  const query = useQuery({ queryKey: ['network', net, 'dns'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/dns', { params: { path: { net } } })) });
  return <Card title="DNS dividido">
    <p>Solo enruta las consultas de los dominios indicados. Nunca captura el DNS general ni configura el dominio raíz «.».
      El agente necesita systemd-resolved; si no está disponible, informa unsupported y no modifica resolv.conf.</p>
    <p>Hasta 3 servidores IP y 8 dominios LDH en minúsculas, sin punto final. El servidor valida el alcance de las IP.
      Vaciar ambos campos elimina la configuración.</p>
    <Status query={query}/>{query.isError && <RunButton run={() => query.refetch()}>Reintentar DNS</RunButton>}
    {query.data && (canEdit ? <DnsForm key={net} net={net} data={query.data}/> : <>
      <p>Solo lectura</p><dl><dt>Servidores DNS</dt><dd>{query.data.servers.join(', ') || 'Sin servidores'}</dd>
        <dt>Dominios de enrutamiento</dt><dd>{query.data.domains.join(', ') || 'Sin dominios'}</dd></dl>
    </>)}
  </Card>;
}
