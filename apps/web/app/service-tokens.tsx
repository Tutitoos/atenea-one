// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, result } from '@atenea/api-client';
import { ActionForm, ConfirmDialog, Button, CopyButton, Empty, StatusBadge, Status } from './ui';
import { nameSchema } from './domain';

const DAYS = ['7', '30', '90'] as const;

// ADR-020: el valor del token vive solo en este estado de React; nunca entra en la caché de consultas
// ni en el almacenamiento del navegador, y desaparece al ocultarlo o al salir de la pestaña.
export function ServiceTokens({ org }: { org: string }) {
  const [created, setCreated] = useState<{ name: string; token: string }>();
  const tokens = useQuery({
    queryKey: ['org', org, 'service-tokens'],
    queryFn: () => result(api.GET('/api/v1/orgs/{org}/service-tokens', { params: { path: { org } } })),
  });
  return <><h2>Tokens de IA</h2><Status query={tokens}/>
    <p>Un token de servicio da acceso de solo lectura a esta organización a un asistente de IA (redes, dispositivos, rutas, DNS,
      políticas y auditoría). No puede aprobar, revocar ni modificar nada. Caduca solo y puedes revocarlo en cualquier momento.</p>
    <ActionForm label="Crear token" fields={[
      { name: 'name', label: 'Nombre del token' },
      { name: 'days', label: 'Caducidad (días)', value: '30', options: DAYS },
    ]} submit={async values => {
      const days = Number(DAYS.find(day => day === values.days) ?? '30');
      const token = await result(api.POST('/api/v1/orgs/{org}/service-tokens', {
        params: { path: { org } }, body: { name: nameSchema.parse(values.name), ttl_secs: days * 86400 },
      }));
      setCreated({ name: values.name, token: token.token });
      await tokens.refetch();
    }}/>
    {created && <section role="alert" className="card" aria-label="Token creado">
      <h3>Guarda ahora el token «{created.name}»</h3>
      <p>Se muestra una sola vez y no se puede recuperar después. Guárdalo en un fichero con permisos 0600 fuera de cualquier repositorio.</p>
      <code>{created.token}</code> <CopyButton value={created.token} label="Copiar token"/>
      <p><Button onClick={() => setCreated(undefined)}>Ocultar token</Button></p>
    </section>}
    {tokens.data?.length === 0 && <Empty>Todavía no hay tokens de servicio.</Empty>}
    <ul className="cards">{tokens.data?.map(item => {
      const state = item.revoked_at ? 'Revocado' : new Date(item.expires_at).getTime() <= Date.now() ? 'Caducado' : 'Activo';
      return <li className="card" key={item.id}>
        <h3>{item.name}</h3>
        <p><StatusBadge tone={state === 'Activo' ? 'ok' : 'neutral'}>{state}</StatusBadge></p>
        <p>Caduca: <time dateTime={item.expires_at}>{new Date(item.expires_at).toLocaleString('es')}</time></p>
        <p>Último uso: {item.last_used_at ? <time dateTime={item.last_used_at}>{new Date(item.last_used_at).toLocaleString('es')}</time> : 'nunca'}</p>
        {state === 'Activo' && <ConfirmDialog trigger={`Revocar ${item.name}`} title="Revocar token de servicio"
          description={`El token «${item.name}» dejará de funcionar de inmediato. Esta acción no se puede deshacer.`}
          label="Confirmar revocación" confirm={async () => {
            await api.DELETE('/api/v1/orgs/{org}/service-tokens/{id}', { params: { path: { org, id: item.id } } });
            await tokens.refetch();
          }}/>}
      </li>;
    })}</ul></>;
}
