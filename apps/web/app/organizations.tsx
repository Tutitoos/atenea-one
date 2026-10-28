// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { Link, useSearchParams } from 'react-router';
import { Tabs } from 'radix-ui';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { z } from 'zod';
import { ActionForm, ConfirmDialog, Dialog, EmptyState, PageHeader, ShortId, Status, StatusBadge } from './ui';
import { nameSchema, networkSchema } from './domain';
import { CertificateStatus } from './server-status';
import { RenameDialog } from './rename';

export function Organizations({ org, choose }: { org?: Model<'Organization'>; choose: (net: string) => void }) {
  const cache = useQueryClient();
  const [, setParams] = useSearchParams();
  const [deleted, setDeleted] = useState(false);
  const [uuidChange, setUuidChange] = useState<{ user: string; role: 'member' | 'admin' | 'owner' }>();
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
  const protectOwner = (user: string, role: string) => {
    if (role !== 'owner' && members.data?.filter(member => member.role === 'owner').length === 1 &&
      members.data.some(member => member.user_id === user && member.role === 'owner')) {
      throw new Error('Siempre debe quedar al menos un owner.');
    }
  };
  const changeRole = async (user: string, role: 'member' | 'admin' | 'owner') => {
    protectOwner(user, role);
    await api.PUT('/api/v1/orgs/{org}/members/{user}', { params: { path: { org: current.id, user } }, body: { role } });
    await members.refetch();
  };
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
          <code>{network.ipv4_cidr} · {network.ipv6_cidr}</code><div>
            <CertificateStatus installed={network.has_issuer_cert} expires={network.issuer_cert_not_after}/>
          </div>
        </li>)}</ul> : !networks.isPending && !networks.isError &&
          <EmptyState message="No hay redes." action={createNetwork ?? <p>Pide a un administrador que cree una red.</p>}/>}
      </Tabs.Content>
      <Tabs.Content value="members"><h2>Miembros y roles</h2><Status query={members}/>
        <p>Solo el propietario gestiona roles. Siempre debe quedar al menos un owner. Los usuarios deben haber iniciado sesión previamente.</p>
        {owner && <>
          <ActionForm label="Añadir por correo" fields={[
            { name: 'email', label: 'Correo electrónico', type: 'email' },
            { name: 'role', label: 'Rol', value: 'member', options: ['member', 'admin', 'owner'] },
          ]} submit={async values => {
            try {
              await api.POST('/api/v1/orgs/{org}/members', { params: { path: { org: current.id } },
                body: { email: z.email().parse(values.email), role: z.enum(['member', 'admin', 'owner']).parse(values.role) } });
              await members.refetch();
            } catch (error) {
              if (error instanceof ApiError) {
                if (error.status === 404) throw new Error('No hay ningún usuario con ese correo que haya iniciado sesión antes. Pide que entre una vez en el panel.', { cause: error });
                if (error.status === 429) throw new Error('Demasiados intentos; espera un minuto.', { cause: error });
                if (error.status === 409 && error.detail.error.code === 'last_owner') throw new Error('Siempre debe quedar al menos un owner.', { cause: error });
                if (error.status === 409 && error.detail.error.code === 'conflict') throw new Error('Varias identidades comparten ese correo; resuélvelo en el proveedor de identidad.', { cause: error });
              }
              throw error;
            }
          }}/>
          <details><summary>Alternativa: añadir o cambiar por UUID</summary>
            <ActionForm label="Revisar cambio por UUID" fields={[
              { name: 'user', label: 'UUID del usuario' },
              { name: 'role', label: 'Rol', value: 'member', options: ['member', 'admin', 'owner'] },
            ]} submit={async values => {
              setUuidChange({ user: z.guid().parse(values.user), role: z.enum(['member', 'admin', 'owner']).parse(values.role) });
            }}/>
            {uuidChange && <Dialog trigger="Confirmar operación por UUID" title="Confirmar operación por UUID"
              description={`Se asignará el rol ${uuidChange.role} a ${uuidChange.user}.`}>
              <ActionForm label="Confirmar asignación" fields={[]} submit={() => changeRole(uuidChange.user, uuidChange.role)}/>
            </Dialog>}
          </details>
        </>}
        <ul className="cards">{members.data?.map(member => <li className="card" key={member.user_id}>
          <h3>{member.display_name ?? `${member.user_id.slice(0, 8)}…`}</h3>
          <div className="muted"><ShortId value={member.user_id}/></div>
          {current.role !== 'member' && member.email && <p>{member.email}</p>}
          <p><StatusBadge>{member.role}</StatusBadge></p>
          {owner && <>
            <Dialog trigger="Cambiar rol" title="Cambiar rol" description={`Cambiar el rol de ${member.display_name ?? member.user_id}. Confirma la asignación antes de enviarla.`}>
              <ActionForm label="Confirmar cambio de rol" fields={[
                { name: 'role', label: 'Rol', value: member.role, options: ['member', 'admin', 'owner'] },
              ]} submit={values => changeRole(member.user_id, z.enum(['member', 'admin', 'owner']).parse(values.role))}/>
            </Dialog>
            <ConfirmDialog trigger="Retirar miembro" title="Retirar miembro" description={`Se retirará el acceso de ${member.display_name ?? member.user_id}.`}
              label="Confirmar retirada" confirm={async () => {
                protectOwner(member.user_id, 'member');
                await api.DELETE('/api/v1/orgs/{org}/members/{user}', { params: { path: { org: current.id, user: member.user_id } } });
                await members.refetch();
              }}/>
          </>}</li>)}</ul>
      </Tabs.Content>
      <Tabs.Content value="settings"><h2>Ajustes</h2>{owner ? <>
        <RenameDialog kind="organización" name={current.name} rename={async name => {
          await api.PATCH('/api/v1/orgs/{org}', { params: { path: { org: current.id } }, body: { name } });
          await cache.invalidateQueries({ queryKey: ['organizations'] });
        }}/>
        <section className="danger-zone">
        <h3>Zona de peligro</h3><p>El borrado de la organización y sus redes es irreversible.</p>
        <ConfirmDialog trigger="Borrar organización" title="Borrar organización definitivamente"
          description="Se eliminarán la organización y sus redes. Esta acción no se puede deshacer."
          confirmation={current.name} label="Confirmar borrado" confirm={async () => {
            await api.DELETE('/api/v1/orgs/{org}', { params: { path: { org: current.id } } });
            setDeleted(true); setParams({});
            await cache.invalidateQueries({ queryKey: ['organizations'] });
          }}/>
      </section></> : <p>Solo el owner puede renombrar o borrar la organización.</p>}</Tabs.Content>
    </Tabs.Root>
  </section>;
}
