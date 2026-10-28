// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router';
import { readNetworks, stateText, stateTone } from '../lib/networks';
import { groupServers } from '../lib/servers';
import { Dot, Empty } from '../ui';

export default function Servers() {
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-4 text-xl font-semibold">Servidores</h1>
      {networks.error ? <Empty>{networks.error.message}</Empty> : !networks.data ? <Empty>Comprobando…</Empty> : networks.data.length === 0 ? <Empty>Sin servidores. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link>.</Empty> : (
        <ul className="space-y-3">
          {groupServers(networks.data).map((server) => <li key={server.host}>
            <Link to={`/servers/${encodeURIComponent(server.host)}`} className="flex items-center gap-2 rounded-lg bg-card p-3">
              <Dot tone={stateTone[server.state]} />
              <span><span className="block font-semibold">{server.host}</span>{' '}<span className="text-xs text-mut">{server.networks.length} {server.networks.length === 1 ? 'red' : 'redes'} · {stateText[server.state]}</span></span>
            </Link>
          </li>)}
        </ul>
      )}
    </main>
  );
}
