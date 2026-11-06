// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { keepPreviousData, useQueries, useQuery } from '@tanstack/react-query';
import { Link } from 'react-router';
import { readNetworks, stateText, stateTone } from '../lib/networks';
import { certificateLeft, groupServers, readServerNetworks } from '../lib/servers';
import { button, Dot, Empty, Loading, Skeleton } from '../ui';

export default function Servers() {
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const servers = groupServers(networks.error ? [] : networks.data ?? []);
  const statuses = useQueries({ queries: servers.map((server) => {
    const connected = server.networks.filter((net) => net.state === 'conectada').map((net) => net.network_id);
    return { queryKey: ['server-status', server.host, connected], queryFn: () => readServerNetworks(connected), enabled: connected.length > 0, refetchInterval: 10000, placeholderData: keepPreviousData };
  }) });
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-4 text-xl font-semibold">Servidores</h1>
      {networks.error ? <Empty tone="error">{networks.error.message} <button type="button" className={button} onClick={() => { void networks.refetch(); }}>Reintentar</button></Empty> : !networks.data ? <Loading className="space-y-3">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-14 w-full" />)}</Loading> : networks.data.length === 0 ? <Empty>Sin servidores. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link>.</Empty> : (
        <ul className="space-y-3">
          {servers.map((server, index) => {
            const lefts = (statuses[index].data ?? []).map(({ status }) => certificateLeft(status.server?.issuer_cert_not_after_unix)).filter((left) => left != null);
            const left = lefts.length ? Math.min(...lefts) : null;
            return <li key={server.host}>
              <Link to={`/servers/${encodeURIComponent(server.host)}`} className="flex items-center gap-2 rounded-[10px] bg-card p-3">
                <Dot tone={stateTone[server.state]} />
                <span className="min-w-0"><span title={server.host} className="block min-w-0 truncate font-semibold">{server.host}</span>{' '}<span className="text-xs text-mut">{server.networks.length} {server.networks.length === 1 ? 'red' : 'redes'} · {stateText[server.state]}</span></span>
                {left != null && left <= 14 * 86400 && <span className="shrink-0 rounded-full bg-warn/15 px-2 py-1 text-xs text-warn">{left <= 0 ? 'Certificado caducado' : `Certificado: quedan ${Math.ceil(left / 86400)} ${Math.ceil(left / 86400) === 1 ? 'día' : 'días'}`}</span>}
                <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="ml-auto shrink-0 text-mut"><path d="m9 6 6 6-6 6" /></svg>
              </Link>
            </li>;
          })}
        </ul>
      )}
    </main>
  );
}
