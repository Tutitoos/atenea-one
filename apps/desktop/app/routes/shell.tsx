// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { NavLink, Outlet } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { readAgent } from '../lib/agent';
import { ago, levelTone } from '../lib/format';
import { readNetworks } from '../lib/networks';
import { groupServers } from '../lib/servers';
import type { Agent } from '../lib/live';
import { Pill } from '../ui';

const NETWORK_KEY = 'atenea.network';
const link = ({ isActive }: { isActive: boolean }) => `cursor-default rounded-md px-2.5 py-1.5 text-sm ${isActive ? 'bg-fg/10' : ''}`;

export default function Shell() {
  // Red cuyo estado se enseña: la guardada si sigue unida, si no la primera. Sin redes unidas no hay estado.
  const [chosen, choose] = useState(() => localStorage.getItem(NETWORK_KEY) ?? '');
  const selectNetwork = (id: string) => { localStorage.setItem(NETWORK_KEY, id); choose(id); };
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const network = networks.data?.find((n) => n.network_id === chosen)?.network_id ?? networks.data?.[0]?.network_id ?? '';
  const empty = networks.data?.length === 0;
  const read = useQuery({ queryKey: ['agent', network], queryFn: () => readAgent(network), enabled: network !== '', refetchInterval: 2000 });
  // Con error, los datos anteriores ya no son de fiar: no se enseñan.
  const error = networks.error ?? read.error;
  const agent: Agent = { sample: error ? undefined : read.data, network, error, empty: !!empty, selectNetwork };
  const health = agent.sample?.reply.health;
  const lease = agent.sample?.reply.status.config.valid_until_unix;
  const left = lease ? Math.round(lease - Date.now() / 1000) : null;
  const subtitle = error ? error.message : empty ? 'Sin redes' : health ? (health.reasons[0] ?? (left == null ? '' : left > 0 ? `Lease válido ${ago(left)}` : 'Lease caducado')) : 'Comprobando…';

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-4 py-2.5">
        <img src="/icon.svg" alt="" width={34} height={34} />
        <div>
          <div className="text-sm font-semibold">Atenea One</div>
          <div className="text-xs text-mut">{subtitle}</div>
        </div>
        <div className="ml-auto flex items-center gap-2">
        {networks.data && networks.data.length > 0 && (
          <select
            aria-label="Red"
            value={network}
            onChange={(e) => selectNetwork(e.target.value)}
            className="rounded-md border border-line bg-card px-2 py-1 text-sm"
          >
            {groupServers(networks.data).map((server) => <optgroup key={server.host} label={server.host}>{server.networks.map((n) => <option key={n.network_id} value={n.network_id}>{n.name}</option>)}</optgroup>)}
          </select>
        )}
        <Pill tone={empty ? 'off' : health ? levelTone(health.level) : 'off'}>{empty ? 'Sin redes' : health ? health.text : 'Sin datos'}</Pill>
        </div>
      </header>
      {health && health.reasons.length > 0 && (
        // Rojo solo si no hay conexión; un aviso (degradado) no es un error.
        <ul aria-label="Avisos" className={`border-b border-line px-4 py-2 ${health.level === 'down' ? 'bg-bad/10 text-bad' : 'bg-warn/15 text-fg'}`}>
          {health.reasons.map((r) => <li key={r}>{r}</li>)}
        </ul>
      )}
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Secciones" className="flex w-48 shrink-0 flex-col gap-0.5 border-r border-line bg-side p-2">
          <NavLink to="/devices" className={link}>Dispositivos</NavLink>
          <NavLink to="/servers" className={link}>Servidores</NavLink>
          <NavLink to="/networks" className={link}>Redes</NavLink>
          <NavLink to="/diagnostics" className={link}>Diagnóstico</NavLink>
        </nav>
        <div className="flex min-w-0 flex-1"><Outlet context={agent} /></div>
      </div>
    </div>
  );
}
