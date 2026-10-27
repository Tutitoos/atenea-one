// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { NavLink, Outlet } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { readAgent } from '../lib/agent';
import { ago, levelTone } from '../lib/format';
import type { Agent } from '../lib/live';
import { Pill } from '../ui';

const link = ({ isActive }: { isActive: boolean }) => `cursor-default rounded-md px-2.5 py-1.5 text-sm ${isActive ? 'bg-fg/10' : ''}`;

export default function Shell() {
  const { data, error } = useQuery({ queryKey: ['agent'], queryFn: readAgent, refetchInterval: 2000 });
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
        <Pill tone={health ? levelTone(health.level) : 'off'}>{health ? health.text : 'Sin datos'}</Pill>
      </header>
      {health && health.level !== 'ok' && (
        <ul className="border-b border-line bg-bad/10 px-4 py-2 text-bad">
          {health.reasons.map((r) => <li key={r}>{r}</li>)}
        </ul>
      )}
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Secciones" className="flex w-48 shrink-0 flex-col gap-0.5 border-r border-line bg-side p-2">
          <NavLink to="/devices" className={link}>Dispositivos</NavLink>
          <NavLink to="/diagnostics" className={link}>Diagnóstico</NavLink>
        </nav>
        <div className="flex min-w-0 flex-1"><Outlet context={agent} /></div>
      </div>
    </div>
  );
}
