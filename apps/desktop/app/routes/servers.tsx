// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useQueries, useQuery } from '@tanstack/react-query';
import { Link } from 'react-router';
import { readNetworks, stateText, stateTone } from '../lib/networks';
import { certificateLeft, groupServers, readServerNetworks } from '../lib/servers';
import { Dot, Empty } from '../ui';

export default function Servers() {
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const servers = groupServers(networks.error ? [] : networks.data ?? []);
  const statuses = useQueries({ queries: servers.map((server) => {
    const connected = server.networks.filter((net) => net.state === 'conectada').map((net) => net.network_id);
    return { queryKey: ['server-status', server.host, connected], queryFn: () => readServerNetworks(connected), enabled: connected.length > 0, refetchInterval: 2000 };
  }) });
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-4 text-xl font-semibold">Servidores</h1>
      {networks.error ? <Empty>{networks.error.message}</Empty> : !networks.data ? <Empty>Comprobando…</Empty> : networks.data.length === 0 ? <Empty>Sin servidores. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link>.</Empty> : (
        <ul className="space-y-3">
          {servers.map((server, index) => <li key={server.host}>
            <Link to={`/servers/${encodeURIComponent(server.host)}`} className="flex items-center gap-2 rounded-lg bg-card p-3">
              <Dot tone={stateTone[server.state]} />
              <span><span className="block font-semibold">{server.host}</span>{' '}<span className="text-xs text-mut">{server.networks.length} {server.networks.length === 1 ? 'red' : 'redes'} · {stateText[server.state]}</span></span>
              {statuses[index].data?.some(({ status }) => { const left = certificateLeft(status.server?.issuer_cert_not_after_unix); return left != null && left <= 14 * 86400; }) && <span className="rounded-full bg-warn/15 px-2 py-1 text-xs text-warn">Certificado próximo a caducar</span>}
            </Link>
          </li>)}
        </ul>
      )}
    </main>
  );
}
