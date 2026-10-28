// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useState } from 'react';
import { NavLink, Outlet } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { readAgent } from '../lib/agent';
import { ago, levelTone } from '../lib/format';
import { readNetworks } from '../lib/networks';
import type { Agent } from '../lib/live';
import { Pill } from '../ui';

const NETWORK_KEY = 'atenea.network';
const link = ({ isActive }: { isActive: boolean }) => `cursor-default rounded-md px-2.5 py-1.5 text-sm ${isActive ? 'bg-fg/10' : ''}`;

export default function Shell() {
  // Red cuyo estado se enseña: vacío es la instancia predeterminada. Se recuerda entre sesiones.
  const [chosen, choose] = useState(() => localStorage.getItem(NETWORK_KEY) ?? '');
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  // Si la red elegida ya no existe (salió de ella), se vuelve a la predeterminada sin esperar a un error.
  const network = networks.data?.some((n) => n.network_id === chosen) ? chosen : '';
  const { data, error } = useQuery({ queryKey: ['agent', network], queryFn: () => readAgent(network), refetchInterval: 2000 });
  // Primera ejecución: sin elección guardada y con la predeterminada caída, lo único que hay que enseñar es la
  // primera red unida. No se guarda: si el usuario elige «Predeterminada» a propósito, se respeta.
  const first = networks.data?.[0]?.network_id;
  useEffect(() => {
    if (error && first && network === '' && localStorage.getItem(NETWORK_KEY) === null) choose(first);
  }, [error, first, network]);
  // Con error, los datos anteriores ya no son de fiar: no se enseñan.
  const agent: Agent = { sample: error ? undefined : data, error };
  const health = agent.sample?.reply.health;
  const lease = agent.sample?.reply.status.config.valid_until_unix;
  const left = lease ? Math.round(lease - Date.now() / 1000) : null;
  const subtitle = error ? error.message : health ? (health.reasons[0] ?? (left == null ? '' : left > 0 ? `Lease válido ${ago(left)}` : 'Lease caducado')) : 'Comprobando…';

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
            onChange={(e) => { localStorage.setItem(NETWORK_KEY, e.target.value); choose(e.target.value); }}
            className="rounded-md border border-line bg-card px-2 py-1 text-sm"
          >
            <option value="">Predeterminada</option>
            {networks.data.map((n) => <option key={n.network_id} value={n.network_id}>{n.name}</option>)}
          </select>
        )}
        <Pill tone={health ? levelTone(health.level) : 'off'}>{health ? health.text : 'Sin datos'}</Pill>
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
          <NavLink to="/networks" className={link}>Redes</NavLink>
          <NavLink to="/diagnostics" className={link}>Diagnóstico</NavLink>
        </nav>
        <div className="flex min-w-0 flex-1"><Outlet context={agent} /></div>
      </div>
    </div>
  );
}
