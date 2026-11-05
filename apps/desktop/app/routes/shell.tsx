// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMemo, useState } from 'react';
import { NavLink, Outlet, useMatch, useNavigate } from 'react-router';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { readAgent } from '../lib/agent';
import { ago, levelTone } from '../lib/format';
import { readNetworks, stateText, stateTone } from '../lib/networks';
import { groupServers } from '../lib/servers';
import type { Agent } from '../lib/live';
import { Dot, Pill } from '../ui';

const NETWORK_KEY = 'atenea.network';
const link = ({ isActive }: { isActive: boolean }) => `cursor-default rounded-md px-2.5 py-1.5 text-sm ${isActive ? 'bg-fg/10' : ''}`;

export default function Shell() {
  // Red cuyo estado se enseña: la guardada si sigue unida, si no la primera. Sin redes unidas no hay estado.
  const [chosen, choose] = useState(() => localStorage.getItem(NETWORK_KEY) ?? '');
  const navigate = useNavigate();
  const device = useMatch('/devices/:id');
  const selectNetwork = (id: string) => {
    localStorage.setItem(NETWORK_KEY, id);
    choose(id);
    // Un dispositivo concreto es de una red: en otra daría un falso «ya no está en la configuración».
    if (device && device.params.id !== 'self') void navigate('/devices/self');
  };
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const network = networks.data?.find((n) => n.network_id === chosen)?.network_id ?? networks.data?.[0]?.network_id ?? '';
  const empty = networks.data?.length === 0;
  // Las redes cuelgan de Dispositivos: se ven mientras esa sección (o un dispositivo suyo) está abierta.
  const inDevices = useMatch({ path: '/devices', end: false }) !== null;
  // Al cambiar de red se conserva la lectura anterior, pero marcada como obsoleta (`stale`): no es de la red nueva.
  const read = useQuery({ queryKey: ['agent', network], queryFn: () => readAgent(network), enabled: network !== '', refetchInterval: 2000, placeholderData: keepPreviousData });
  const stale = read.isPlaceholderData;
  // Un fallo puntual se tolera si hay datos previos; con dos seguidos (o sin datos) ya no son de fiar y no se enseñan.
  const readError = read.error && (read.failureCount >= 2 || !read.data) ? read.error : null;
  const error = networks.error ?? readError;
  const agent: Agent = useMemo(
    () => ({ sample: readError ? undefined : read.data, stale, network, error, empty: !!empty, selectNetwork }),
    // `selectNetwork` solo depende de `device`, que cambia con la ruta.
    [readError, read.data, stale, network, error, empty, device?.params.id],
  );
  const health = stale ? undefined : agent.sample?.reply.health;
  const lease = agent.sample?.reply.status.config.valid_until_unix;
  const left = lease ? Math.round(lease - Date.now() / 1000) : null;
  const subtitle = error ? error.message : empty ? 'Sin redes' : health ? (health.reasons[0] ?? (left == null ? '' : left > 0 ? `Lease: quedan ${ago(left)}` : 'Lease caducado')) : 'Comprobando…';

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-4 py-2.5">
        <img src="/icon.svg" alt="" width={34} height={34} />
        <div>
          <div className="text-sm font-semibold">Atenea One</div>
          <div className="text-xs text-mut">{subtitle}</div>
        </div>
        <div className="ml-auto flex items-center gap-2">
        <Pill tone={empty ? 'off' : health ? levelTone(health.level) : 'off'}>{empty ? 'Sin redes' : health ? health.text : 'Sin datos'}</Pill>
        </div>
      </header>
      {health && health.reasons.length > 0 && (
        // Rojo solo si no hay conexión; un aviso (degradado) no es un error.
        <ul aria-label="Avisos" className={`border-b border-line px-4 py-2 ${health.level === 'down' ? 'bg-bad/10 text-bad' : 'bg-warn/15 text-fg'}`}>
          {health.reasons.map((r, i) => <li key={`${i}-${r}`}>{r}</li>)}
        </ul>
      )}
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Secciones" className="flex w-48 shrink-0 flex-col gap-0.5 border-r border-line bg-side p-2">
          <NavLink to="/devices" className={link}>Dispositivos</NavLink>
          {inDevices && networks.data && networks.data.length > 0 && (
            <div role="group" aria-label="Elegir red" className="mb-1 ml-4 flex flex-col border-l border-fg/25 pl-1.5">
              {groupServers(networks.data).map((server) => (
                <div key={server.host} role="group" aria-label={server.host} className="flex flex-col gap-0.5 pb-1">
                  <div aria-hidden="true" title={server.host} className="truncate px-2 pb-0.5 pt-1.5 text-[11px] font-medium text-mut">{server.host}</div>
                  {server.networks.map((n) => <button key={n.network_id} type="button" title={`${n.name} · ${stateText[n.state]}`} aria-pressed={n.network_id === network} onClick={() => selectNetwork(n.network_id)} className={`flex cursor-default items-center gap-2 rounded-md px-2 py-1 text-left text-sm focus-visible:outline-2 focus-visible:outline-sel ${n.network_id === network ? 'bg-sel font-medium text-white' : 'text-fg/80 hover:bg-fg/10'}`}><Dot tone={stateTone[n.state]} /><span className="truncate">{n.name}</span></button>)}
                </div>
              ))}
            </div>
          )}
          <NavLink to="/servers" className={link}>Servidores</NavLink>
          <NavLink to="/networks" className={link}>Redes</NavLink>
          <NavLink to="/diagnostics" className={link}>Diagnóstico</NavLink>
        </nav>
        <div className={`flex min-w-0 flex-1 stale-fade${stale ? ' stale' : ''}`} aria-busy={stale}><Outlet context={agent} /></div>
      </div>
    </div>
  );
}
