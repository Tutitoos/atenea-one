// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useState } from 'react';
import { Link, useSearchParams } from 'react-router';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { PageHeader, QueryAnnouncements, RunButton, Status, StatusBadge, UpdatedIndicator } from './ui';
import { Organizations } from './organizations';
import { Breadcrumbs } from './breadcrumbs';
import { sections } from './sections';
import { useClock, useNetworkEvents } from './live';
import { secondsSince } from './domain';
import { useServerInfo } from './server-status';

function Theme() {
  const [theme, setTheme] = useState('sistema');
  useEffect(() => { setTheme(localStorage.getItem('atenea-theme') ?? 'sistema'); }, []);
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => { document.documentElement.dataset.theme = theme === 'sistema' ? (media.matches ? 'oscuro' : 'claro') : theme; };
    apply(); media.addEventListener('change', apply);
    return () => media.removeEventListener('change', apply);
  }, [theme]);
  return <label>Tema<select value={theme} onChange={event => {
    setTheme(event.target.value); localStorage.setItem('atenea-theme', event.target.value);
  }}>{['sistema', 'claro', 'oscuro'].map(value => <option key={value}>{value}</option>)}</select></label>;
}
export function Login({ expired = false }: { expired?: boolean }) {
  return <main className="login"><div className="card"><p className="eyebrow">ATENEA ONE</p>
    <h1>Tu red, bajo tu control.</h1><p>{expired ?
      'Tu sesión ha caducado o no has iniciado sesión. Tus cambios sin guardar no se enviarán.' :
      'Identifícate con el proveedor de tu organización para operar la red privada.'}</p>
    <a className="button primary" href="/auth/login">Iniciar sesión con OIDC</a>
    <p className="muted">Las claves privadas permanecen fuera del panel.</p></div></main>;
}
export default function Panel() {
  const [params, setParams] = useSearchParams();
  const cache = useQueryClient();
  const session = useQuery({ queryKey: ['session'], queryFn: () => result(api.GET('/api/v1/session')), refetchInterval: 30_000 });
  const server = useServerInfo();
  const orgs = useQuery({ queryKey: ['organizations'], queryFn: () => result(api.GET('/api/v1/orgs')), enabled: !!session.data });
  const selected = orgs.data?.find(org => org.id === params.get('org'));
  const net = params.get('net') ?? '';
  const section = sections.find(item => item.id === params.get('view')) ?? sections[0];
  const navigate = (org: string, network = '', view = sections[0].id) =>
    setParams({ org, ...(network ? { net: network, view } : {}) });
  const networks = useQuery({ queryKey: ['org', selected?.id, 'networks'], enabled: !!selected,
    queryFn: () => result(api.GET('/api/v1/orgs/{org}/networks', { params: { path: { org: selected!.id } } })) });
  const selectedNetwork = useQuery({
    queryKey: ['network', net, 'detail'], enabled: !!session.data && !!net,
    queryFn: () => result(api.GET('/api/v1/networks/{net}', { params: { path: { net } } })),
  });
  const devices = useQuery({ queryKey: ['network', net, 'devices'], enabled: !!session.data && !!net,
    queryFn: () => result(api.GET('/api/v1/networks/{net}/devices', { params: { path: { net } } })) });
  const pending = devices.data?.filter(device => device.status === 'pending').length ?? 0;
  // Enlace directo desde escritorio: sin org se deduce de la red, conservando la sección.
  const owner = useQuery({ queryKey: ['network-org', net], enabled: !!session.data && !!net && !params.get('org'),
    queryFn: () => result(api.GET('/api/v1/networks/{net}', { params: { path: { net } } })) });
  useEffect(() => {
    if (owner.data) setParams({ org: owner.data.org_id, net, view: section.id }, { replace: true });
  }, [owner.data, net, section.id, setParams]);
  if (session.error instanceof ApiError && session.error.status === 401) return <Login expired/>;
  if (!session.data) return <main><h1>Atenea One</h1><Status query={session}/>
    {session.isError && <RunButton run={() => session.refetch()}>Reintentar</RunButton>}</main>;
  return <div className="shell"><a className="skip" href="#contenido">Saltar al contenido</a>
    <QueryAnnouncements/>
    <aside className="sidebar"><header><a href="/" className="brand">△ Atenea One</a>
      <p className="eyebrow">PLANO DE CONTROL</p></header>
      <label>Organización<select value={selected?.id ?? ''} onChange={event => navigate(event.target.value)}>
        <option value="">Selecciona organización</option>
        {orgs.data?.map(org => <option key={org.id} value={org.id}>{org.name} · {org.role}</option>)}
      </select></label><Status query={orgs}/>
      {selected && <><label>Red<select value={net} onChange={event => navigate(selected.id, event.target.value)}>
        <option value="">Selecciona red</option>{net && !networks.data?.some(network => network.id === net) &&
          <option value={net}>{selectedNetwork.data?.name ?? 'Cargando nombre de red…'}</option>}
        {networks.data?.map(network => <option key={network.id} value={network.id}>{network.name}</option>)}
      </select></label><Status query={networks}/></>}
      {selected && <Link className="all-networks" to={`/?org=${selected.id}`}>Todas las redes</Link>}
      {net && selected && <nav aria-label="Secciones de red">{[...new Set(sections.map(item => item.group))].map(group =>
        <div className="nav-group" key={group}><p className="eyebrow">{group}</p>
          {sections.filter(item => item.group === group).map(item => <Link key={item.id}
            aria-current={section.id === item.id ? 'page' : undefined}
            to={`/?org=${selected.id}&net=${net}&view=${encodeURIComponent(item.id)}`}>
            {item.id === 'Observabilidad' ? 'Vista conjunta' : item.label}{item.id === 'Dispositivos' && pending > 0 && <StatusBadge tone="warn">
              <span aria-label={`${pending} solicitud${pending === 1 ? '' : 'es'} pendiente${pending === 1 ? '' : 's'}`}>{pending}</span>
            </StatusBadge>}
          </Link>)}
        </div>)}</nav>}
      <footer><Theme/><p className="muted">{session.data.user.display_name ?? session.data.user.email ?? 'Usuario autenticado'}</p>
        <p className="muted">{server.data?.control_plane ? `Control plane ${server.data.control_plane.version}` :
          'Este servidor aún no informa de su versión'}</p>
        <RunButton run={async () => { await api.POST('/auth/logout'); cache.clear(); window.location.assign('/'); }}>Cerrar sesión</RunButton>
      </footer>
    </aside>
    <main id="contenido">{net && selected ? <Network key={net} net={net} section={section}
      organization={selected} memberships={session.data.memberships}/> : <>
      {selected && <Breadcrumbs organization={selected}/>}
      <Organizations key={selected?.id} org={selected} choose={network => navigate(selected!.id, network)}/>
    </>}</main>
  </div>;
}
function Network({ net, section, organization, memberships }: {
  net: string; section: typeof sections[number]; organization: Model<'Organization'>; memberships: Model<'Membership'>[];
}) {
  const cache = useQueryClient();
  const connected = useNetworkEvents(net);
  const now = useClock();
  const network = useQuery({ queryKey: ['network', net, 'detail'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}', { params: { path: { net } } })) });
  const devices = useQuery({ queryKey: ['network', net, 'devices'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/devices', { params: { path: { net } } })) });
  const routes = useQuery({ queryKey: ['network', net, 'routes'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/routes', { params: { path: { net } } })) });
  const role = memberships.find(membership => membership.org_id === network.data?.org_id)?.role ?? 'member';
  const Component = section.Component;
  return <><Breadcrumbs organization={organization} network={{ id: net, name: network.data?.name ?? 'Red' }} section={section.label}/>
    <PageHeader title={`${network.data?.name ?? 'Red'} · ${section.label}`}
      description={network.data && `${network.data.ipv4_cidr} · ${network.data.ipv6_cidr}`}
      actions={<UpdatedIndicator updatedAt={devices.dataUpdatedAt}/>}/>
    <Status query={network}/>
    <p className={connected ? 'muted' : 'warning'} role="status">{connected ? 'Eventos en vivo conectados.' :
      'Desconectado: los datos pueden estar desactualizados. Sondeo REST de respaldo cada 30 s.'}</p>
    <p className="muted">Los informes son declaraciones autenticadas de los agentes, no verificaciones independientes.</p>
    <Component net={net} network={network} devices={devices} routes={routes} role={role} canEdit={role !== 'member'}
      elapsed={secondsSince(devices.dataUpdatedAt, now)} refresh={() => cache.invalidateQueries({ queryKey: ['network', net] })}/>
  </>;
}
