// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useSearchParams } from 'react-router';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, Card, ConfirmDialog, CopyButton, EmptyState, RunButton, Status, StatusBadge } from './ui';

type NetworkRole = Model<'NewInvitation'>['role'];
const roleLabels: Record<NetworkRole, string> = { viewer: 'Solo ver', enroller: 'Ver, generar códigos y aprobar' };
const STORAGE = 'atenea-invitacion';

// El servidor responde con códigos; aquí se traducen a frases que dicen qué hacer.
function explain(error: unknown) {
  if (error instanceof ApiError) {
    const code = error.detail.error.code;
    if (code === 'invitation_limit') return 'Hay demasiadas invitaciones vigentes. Anula alguna o espera a que caduquen.';
    if (code === 'already_member') return 'Esta cuenta ya tiene acceso a la red.';
    if (code === 'rate_limited') return 'Demasiadas peticiones seguidas. Espera un momento.';
  }
  return error instanceof Error ? error.message : 'No se pudo completar la acción.';
}

/** Captura `?invitacion=` en el almacenamiento de la sesión del navegador y lo quita de la URL: sobrevive al inicio de sesión. */
export function useInvitationToken() {
  const [params, setParams] = useSearchParams();
  const [token, setToken] = useState(() => typeof sessionStorage === 'undefined' ? '' : sessionStorage.getItem(STORAGE) ?? '');
  const fromUrl = params.get('invitacion');
  useEffect(() => {
    if (!fromUrl) return;
    sessionStorage.setItem(STORAGE, fromUrl); setToken(fromUrl);
    const next = new URLSearchParams(params); next.delete('invitacion'); setParams(next, { replace: true });
  }, [fromUrl, params, setParams]);
  return { token, clear: () => { sessionStorage.removeItem(STORAGE); setToken(''); } };
}

/** Pregunta antes de canjear: la invitación da acceso a una red ajena y no hay forma de deshacerlo sin su propietario. */
export function AcceptInvitation({ token, done, dismiss }: {
  token: string; done: (access: Model<'NetworkAccess'>) => void; dismiss: () => void;
}) {
  const cache = useQueryClient();
  const [message, setMessage] = useState('');
  return <Card title="Invitación a una red">
    <p>Has abierto un enlace de invitación. Al aceptarlo tu cuenta podrá ver esa red; el propietario puede quitarte el acceso
      cuando quiera. No te da acceso a su organización.</p>
    <div className="actions">
      <RunButton variant="primary" run={async () => {
        try {
          const access = await result(api.POST('/api/v1/invitations/accept', { body: { token } }));
          await Promise.all([cache.invalidateQueries({ queryKey: ['shared-networks'] }), cache.invalidateQueries({ queryKey: ['session'] })]);
          done(access);
        } catch (error) {
          // 404: el enlace ya no vale (usado, caducado o anulado). 409: ya tenía acceso; en ambos casos no sirve conservarlo.
          setMessage(error instanceof ApiError && error.status === 404 ?
            'Este enlace ya no es válido: puede haber caducado, haberse usado o haberse anulado. Pide uno nuevo.' : explain(error));
          if (error instanceof ApiError && [404, 409].includes(error.status)) dismiss();
        }
      }}>Aceptar invitación</RunButton>
      <Button onClick={dismiss}>Descartar</Button>
    </div>
    <p role="status">{message}</p>
  </Card>;
}

/** Redes a las que la cuenta pertenece por invitación. */
export function SharedNetworks({ choose }: { choose: (net: string) => void }) {
  const query = useQuery({ queryKey: ['shared-networks'], queryFn: () => result(api.GET('/api/v1/shared-networks')) });
  return <section><h1>Compartidas conmigo</h1><Status query={query}/>
    {query.data?.length === 0 && <EmptyState message="Nadie ha compartido todavía una red contigo."
      action={<p>Cuando alguien te envíe un enlace de invitación, ábrelo en este navegador.</p>}/>}
    {query.data && query.data.length > 0 && <ul className="cards">{query.data.map(network => <li className="card" key={network.id}>
      <h3><Link to={`/?org=compartidas&net=${network.id}&view=Resumen`} onClick={() => choose(network.id)}>{network.name}</Link></h3>
      <code>{network.ipv4_cidr} · {network.ipv6_cidr}</code>
      <ConfirmDialog trigger="Abandonar" title={`Abandonar ${network.name}`}
        description="Perderás el acceso a esta red hasta que su propietario te invite de nuevo. Los dispositivos que aprobaste siguen aprobados."
        label="Abandonar la red" confirm={async () => {
          await api.DELETE('/api/v1/networks/{net}/membership', { params: { path: { net: network.id } } });
          await query.refetch();
        }}/>
      <p><StatusBadge tone={network.role === 'enroller' ? 'warn' : 'neutral'}>{roleLabels[network.role]}</StatusBadge></p>
    </li>)}</ul>}
  </section>;
}

function status(invitation: Model<'Invitation'>) {
  if (invitation.used_at) return 'Usada';
  if (invitation.revoked_at) return 'Anulada';
  return new Date(invitation.expires_at).getTime() < Date.now() ? 'Caducada' : 'Vigente';
}

/** Invitados de la red (solo el propietario, ADR-028 D6 y D7). */
export function Sharing({ net, canManage }: { net: string; canManage: boolean }) {
  const cache = useQueryClient();
  const [role, setRole] = useState<NetworkRole>('viewer');
  const [created, setCreated] = useState<Model<'InvitationCreated'>>();
  const [message, setMessage] = useState('');
  const invitations = useQuery({ queryKey: ['network', net, 'invitations'], enabled: canManage, retry: false,
    queryFn: () => result(api.GET('/api/v1/networks/{net}/invitations', { params: { path: { net } } })) });
  const members = useQuery({ queryKey: ['network', net, 'members'], enabled: canManage, retry: false,
    queryFn: () => result(api.GET('/api/v1/networks/{net}/members', { params: { path: { net } } })) });
  const refresh = () => Promise.all([invitations.refetch(), members.refetch(), cache.invalidateQueries({ queryKey: ['network', net, 'audit'] })]);
  if (!canManage) return <Card title="Compartir la red"><p>Solo el propietario de la organización puede invitar a otras cuentas.</p></Card>;
  return <div className="incorporar-cards">
    <Card title="Invitar a otra cuenta">
      <p>Genera un enlace de un solo uso y envíaselo por el canal que prefieras: este servidor no envía correos. Caduca a los 7 días.
        La cuenta invitada ve esta red, pero no tu organización.</p>
      <form className="action-form" onSubmit={event => {
        event.preventDefault(); setMessage('');
        void (async () => {
          try { setCreated(await result(api.POST('/api/v1/networks/{net}/invitations', { params: { path: { net } }, body: { role } }))); await refresh(); }
          catch (error) { setMessage(explain(error)); }
        })();
      }}>
        <fieldset><legend>Qué podrá hacer</legend>
          {(Object.keys(roleLabels) as NetworkRole[]).map(value => <div className="incorporar-option" key={value}><label>
            <input type="radio" name="invitation-role" checked={role === value} onChange={() => setRole(value)}/> {roleLabels[value]}</label></div>)}
        </fieldset>
        {role === 'enroller' && <p className="muted">Podrá generar códigos de unión y aprobar dispositivos escribiendo su huella completa.
          No crea políticas, no revoca y no cambia DNS ni rutas.</p>}
        <Button type="submit" variant="primary">Crear invitación</Button>
        <p role="status">{message}</p>
      </form>
      {created && <div role="alert">
        <p><strong>Copia el enlace ahora: no se volverá a mostrar.</strong></p>
        <code>{`${window.location.origin}/?invitacion=${created.token}`}</code>
        <CopyButton value={`${window.location.origin}/?invitacion=${created.token}`} label="Copiar enlace de invitación"/>
      </div>}
    </Card>
    <Card title="Invitaciones">
      <Status query={invitations}/>
      {invitations.data?.length === 0 && <p>Todavía no has creado invitaciones.</p>}
      {invitations.data && invitations.data.length > 0 && <table><thead><tr><th>Rol</th><th>Estado</th><th>Caduca</th><th>Acciones</th></tr></thead>
        <tbody>{invitations.data.map(invitation => <tr key={invitation.id}>
          <td>{roleLabels[invitation.role]}</td><td>{status(invitation)}</td>
          <td>{new Date(invitation.expires_at).toLocaleDateString('es')}</td>
          <td>{status(invitation) === 'Vigente' && <ConfirmDialog trigger="Anular" title="Anular invitación"
            description="El enlace dejará de funcionar. Si alguien ya lo había abierto sin aceptarlo, tendrá que pedir otro."
            label="Anular invitación" confirm={async () => {
              await api.DELETE('/api/v1/networks/{net}/invitations/{id}', { params: { path: { net, id: invitation.id } } }); await refresh();
            }}/>}</td></tr>)}</tbody></table>}
    </Card>
    <Card title="Cuentas con acceso">
      <Status query={members}/>
      {members.data?.length === 0 && <p>Ninguna cuenta tiene acceso por invitación.</p>}
      {members.data && members.data.length > 0 && <table><thead><tr><th>Cuenta</th><th>Rol</th><th>Acciones</th></tr></thead>
        <tbody>{members.data.map(member => <tr key={member.user_id}>
          <td>{member.display_name ?? 'Sin nombre'}</td>
          <td><select aria-label={`Rol de ${member.display_name ?? member.user_id}`} value={member.role} onChange={event => {
            void api.PUT('/api/v1/networks/{net}/members/{user}', { params: { path: { net, user: member.user_id } }, body: { role: event.target.value as NetworkRole } })
              .then(refresh, error => setMessage(explain(error)));
          }}>{(Object.keys(roleLabels) as NetworkRole[]).map(value => <option key={value} value={value}>{roleLabels[value]}</option>)}</select></td>
          <td><ConfirmDialog trigger="Quitar" title={`Quitar a ${member.display_name ?? 'esta cuenta'}`}
            description="Perderá el acceso a la red. Los dispositivos que aprobó siguen aprobados: la auditoría dice quién aprobó cada uno."
            label="Quitar acceso" confirm={async () => {
              await api.DELETE('/api/v1/networks/{net}/members/{user}', { params: { path: { net, user: member.user_id } } }); await refresh();
            }}/></td></tr>)}</tbody></table>}
    </Card>
  </div>;
}
