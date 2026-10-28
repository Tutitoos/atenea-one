// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { Link, useSearchParams } from 'react-router';
import { Tabs } from 'radix-ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, result, type Model } from '@atenea/api-client';
import { z } from 'zod';
import { ActionForm, ConfirmDialog, Dialog, EmptyState, PageHeader, ShortId, Status, StatusBadge } from './ui';
import { nameSchema, networkSchema } from './domain';

export function Organizations({ org, choose }: { org?: Model<'Organization'>; choose: (net: string) => void }) {
  const cache = useQueryClient();
  const [, setParams] = useSearchParams();
  const [deleted, setDeleted] = useState(false);
  const current = deleted ? undefined : org;
  const networks = useQuery({
    queryKey: ['org', current?.id, 'networks'], enabled: !!current,
    queryFn: () => result(api.GET('/api/v1/orgs/{org}/networks', { params: { path: { org: current!.id } } })),
  });
  const members = useQuery({
    queryKey: ['org', current?.id, 'members'], enabled: !!current,
    queryFn: () => result(api.GET('/api/v1/orgs/{org}/members', { params: { path: { org: current!.id } } })),
  });
  const createOrganization = <Dialog trigger="Crear organización" title="Crear organización"
    description="Serás propietario de la nueva organización.">
    <ActionForm label="Crear organización" fields={[{ name: 'name', label: 'Nombre' }]} submit={async values => {
      await result(api.POST('/api/v1/orgs', { body: { name: nameSchema.parse(values.name) } }));
      await Promise.all([
        cache.invalidateQueries({ queryKey: ['organizations'] }),
        cache.invalidateQueries({ queryKey: ['session'] }),
      ]);
    }}/>
  </Dialog>;
  if (!current) return <section><PageHeader title="Organizaciones" description="Selecciona una organización en la barra lateral." actions={createOrganization}/></section>;
  const owner = current.role === 'owner';
  const createNetwork = current.role !== 'member' ? <Dialog trigger="Crear red" title="Crear red"
    description="Las claves no se generan en el panel. Prepara el ancla pública offline con atenea root-init (ADR-005).">
    <pre>atenea root-init --out raiz-offline</pre>
    <p>Guarda la raíz en la estación offline. Copia únicamente su clave pública hexadecimal al campo ancla. El emisor se configura fuera del panel.</p>
    <ActionForm label="Crear red" fields={[
      { name: 'name', label: 'Nombre' }, { name: 'ipv4_cidr', label: 'CIDR IPv4' },
      { name: 'ipv6_cidr', label: 'CIDR IPv6' }, { name: 'root_anchor', label: 'Ancla pública raíz (64 hex)' },
    ]} submit={async values => {
      const created = await result(api.POST('/api/v1/orgs/{org}/networks', {
        params: { path: { org: current.id } }, body: networkSchema.parse(values),
      }));
      await networks.refetch(); choose(created.id);
    }}/>
  </Dialog> : null;
  return <section><PageHeader title={current.name} description={`Rol: ${current.role}`} actions={createOrganization}/>
    <Tabs.Root defaultValue="networks" key={current.id}>
      <Tabs.List className="tabs" aria-label="Organización">
        <Tabs.Trigger value="networks">Redes</Tabs.Trigger><Tabs.Trigger value="members">Miembros</Tabs.Trigger>
        <Tabs.Trigger value="settings">Ajustes</Tabs.Trigger>
      </Tabs.List>
      <Tabs.Content value="networks"><h2>Redes</h2>{networks.data?.length ? createNetwork : null}<Status query={networks}/>
        {networks.data?.length ? <ul className="cards">{networks.data.map(network => <li className="card" key={network.id}>
          <h3><Link to={`/?org=${current.id}&net=${network.id}&view=Resumen`}>{network.name}</Link></h3>
          <code>{network.ipv4_cidr} · {network.ipv6_cidr}</code><p>
            <StatusBadge tone={network.has_issuer_cert ? 'ok' : 'warn'}>
              {network.has_issuer_cert ? 'Certificado instalado' : 'Falta certificado de emisor'}
            </StatusBadge>
          </p>
        </li>)}</ul> : !networks.isPending && !networks.isError &&
          <EmptyState message="No hay redes." action={createNetwork ?? <p>Pide a un administrador que cree una red.</p>}/>}
      </Tabs.Content>
      <Tabs.Content value="members"><h2>Miembros y roles</h2><Status query={members}/>
        <p>Solo el propietario gestiona roles. Siempre debe quedar al menos un owner. Los usuarios deben haber iniciado sesión previamente.</p>
        {owner && <ActionForm label="Añadir o cambiar rol" fields={[
          { name: 'user', label: 'UUID del usuario' }, { name: 'role', label: 'Rol', value: 'member', options: ['member', 'admin', 'owner'] },
        ]} submit={async values => {
          const user = z.guid().parse(values.user);
          const role = z.enum(['member', 'admin', 'owner']).parse(values.role);
          const lastOwner = members.data?.filter(member => member.role === 'owner').length === 1;
          if (lastOwner && role !== 'owner' && members.data?.some(member => member.user_id === user && member.role === 'owner')) {
            throw new Error('Siempre debe quedar al menos un owner.');
          }
          await api.PUT('/api/v1/orgs/{org}/members/{user}', { params: { path: { org: current.id, user } }, body: { role } });
          await members.refetch();
        }}/>}<ul className="cards">{members.data?.map(member => <li className="card" key={member.user_id}>
          <ShortId value={member.user_id}/><p>{member.role}</p>
          {owner && <ConfirmDialog trigger="Retirar miembro" title="Retirar miembro" description={`Se retirará el acceso de ${member.user_id}.`}
            label="Confirmar retirada" confirm={async () => {
              if (member.role === 'owner' && members.data?.filter(item => item.role === 'owner').length === 1) {
                throw new Error('Siempre debe quedar al menos un owner.');
              }
              await api.DELETE('/api/v1/orgs/{org}/members/{user}', { params: { path: { org: current.id, user: member.user_id } } });
              await members.refetch();
            }}/>}</li>)}</ul>
      </Tabs.Content>
      <Tabs.Content value="settings"><h2>Ajustes</h2>{owner ? <section className="danger-zone">
        <h3>Zona de peligro</h3><p>El borrado de la organización y sus redes es irreversible.</p>
        <ConfirmDialog trigger="Borrar organización" title="Borrar organización definitivamente"
          description="Se eliminarán la organización y sus redes. Esta acción no se puede deshacer."
          confirmation={current.name} label="Confirmar borrado" confirm={async () => {
            await api.DELETE('/api/v1/orgs/{org}', { params: { path: { org: current.id } } });
            setDeleted(true); setParams({});
            await cache.invalidateQueries({ queryKey: ['organizations'] });
          }}/>
      </section> : <p>Solo el owner puede borrar la organización.</p>}</Tabs.Content>
    </Tabs.Root>
  </section>;
}
