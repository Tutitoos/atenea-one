// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useQueries, useQuery } from '@tanstack/react-query';
import { useNavigate } from 'react-router';
import { readAgent } from '../lib/agent';
import { ago, bytes, networkSummary } from '../lib/format';
import { useAgent } from '../lib/live';
import { readNetworks, stateText, stateTone } from '../lib/networks';
import { Dot, Empty, NoNetworks } from '../ui';

/** Una tarjeta por red unida. Las lecturas comparten clave con el shell; las redes no elegidas se sondean cada 5 s. */
export default function Overview() {
  const { network: chosen, selectNetwork, empty } = useAgent();
  const navigate = useNavigate();
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const list = networks.data ?? [];
  const reads = useQueries({ queries: list.map((n) => ({
    queryKey: ['agent', n.network_id], queryFn: () => readAgent(n.network_id),
    // La elegida ya la sondea el shell cada 2 s.
    refetchInterval: n.network_id === chosen ? false as const : 5000,
  })) });

  if (empty) return <NoNetworks />;
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-4 text-xl font-semibold">Resumen</h1>
      {networks.error ? <Empty>{networks.error.message}</Empty> : !networks.data ? <Empty>Comprobando…</Empty> : (
        <ul className="space-y-3">
          {list.map((n, i) => {
            const read = reads[i];
            const failed = read.error && (read.failureCount >= 2 || !read.data) ? read.error : null;
            const sample = failed ? undefined : read.data;
            const summary = sample && networkSummary(sample.reply.status.peers, sample.rates);
            const health = sample?.reply.health;
            const lease = sample?.reply.status.config.valid_until_unix;
            const left = lease ? Math.round(lease - Date.now() / 1000) : null;
            return (
              <li key={n.network_id}>
                <button type="button" onClick={() => { selectNetwork(n.network_id); void navigate('/devices'); }} className="flex w-full cursor-default flex-col gap-1 rounded-lg bg-card p-3 text-left focus-visible:outline-2 focus-visible:outline-sel">
                  <span className="flex items-center gap-2">
                    <Dot tone={stateTone[n.state]} />
                    <span className="font-semibold">{n.name}</span>
                    <span className="text-xs text-mut">{n.server}</span>
                    {health && <span className="ml-auto text-xs text-mut">{health.text}</span>}
                  </span>
                  <span className="text-xs text-mut">{stateText[n.state]}</span>
                  {failed ? <span className="text-sm text-bad">{failed.message}</span> : !summary ? <span className="text-sm text-mut">Comprobando…</span> : (
                    <span className="text-sm">
                      {summary.peers} {summary.peers === 1 ? 'par' : 'pares'} ({summary.direct} {summary.direct === 1 ? 'directo' : 'directos'}, {summary.relay} por relay, {summary.other} {summary.other === 1 ? 'otro' : 'otros'})
                      {' · '}↓ {bytes(summary.down)}/s ↑ {bytes(summary.up)}/s
                      {left != null && <>{' · '}{left > 0 ? `Lease: quedan ${ago(left)}` : 'Lease caducado'}</>}
                    </span>
                  )}
                  {health?.reasons[0] && <span className="text-sm text-warn">{health.reasons[0]}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </main>
  );
}
