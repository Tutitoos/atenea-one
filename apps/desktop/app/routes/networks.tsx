// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { changeNetwork, readLegacy, readNetworks, stateText, stateTone, type Network } from '../lib/networks';
import { Join, Legacy } from '../join';
import { button, Dot, Empty, primary } from '../ui';

function NetworkCard({ net, busy, onChange }: { net: Network; busy: boolean; onChange: (op: 'up' | 'down' | 'leave', id: string) => void }) {
  const [leaving, setLeaving] = useState(false);
  const stopped = net.state === 'detenida';
  return (
    <section aria-label={net.name} className="mb-4 rounded-[10px] bg-card px-3.5 py-3">
      <div className="flex items-start gap-2">
        <Dot tone={stateTone[net.state]} list />
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">{net.name}</h2>
          <div className="text-xs text-mut">{stateText[net.state]} · {net.server}</div>
          {net.detail && <div className="mt-1 text-xs text-bad">{net.detail}</div>}
        </div>
        <div className="flex gap-2">
          <button type="button" className={button} disabled={busy} onClick={() => onChange(stopped ? 'up' : 'down', net.network_id)}>
            {stopped ? 'Conectar' : 'Desconectar'}
          </button>
          <button type="button" className={button} disabled={busy} onClick={() => setLeaving(true)}>Salir de la red…</button>
        </div>
      </div>
      {leaving && (
        <div role="alertdialog" aria-label={`Salir de ${net.name}`} className="mt-3 rounded-md border border-line p-3 text-sm">
          <p>Se revocará el dispositivo en el servidor y se borrarán las claves de este equipo para «{net.name}». Si el servidor no responde, no se borrará nada. No se puede deshacer.</p>
          <div className="mt-2 flex gap-2">
            <button type="button" className={primary} disabled={busy} onClick={() => { setLeaving(false); onChange('leave', net.network_id); }}>Salir de la red</button>
            <button type="button" className={button} onClick={() => setLeaving(false)}>Cancelar</button>
          </div>
        </div>
      )}
    </section>
  );
}

export default function Networks() {
  const client = useQueryClient();
  const list = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const refresh = () => { void client.invalidateQueries({ queryKey: ['networks'] }); void client.invalidateQueries({ queryKey: ['agent'] }); void client.invalidateQueries({ queryKey: ['legacy'] }); };
  const change = useMutation({ mutationFn: ({ op, id }: { op: 'up' | 'down' | 'leave'; id: string }) => changeNetwork(op, id), onSuccess: refresh });
  // Un fallo al consultar la instalación heredada no impide usar las redes: simplemente no hay banner.
  const legacy = useQuery({ queryKey: ['legacy'], queryFn: readLegacy, refetchInterval: 5000 });

  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-3 text-xl font-semibold">Redes</h1>
      {legacy.data && <Legacy onDone={refresh} />}
      {change.isPending && <p role="status" className="mb-3 text-sm text-mut">Esperando la autorización del sistema…</p>}
      {change.error && <p role="alert" className="mb-3 text-sm text-bad">{change.error.message}</p>}
      {list.error && <p role="alert" className="mb-3 text-sm text-bad">{list.error.message}</p>}
      {list.data?.length === 0 && <Empty>No hay redes unidas con un código.</Empty>}
      {list.data?.map((n) => <NetworkCard key={n.network_id} net={n} busy={change.isPending} onChange={(op, id) => change.mutate({ op, id })} />)}
      <h2 className="mb-2 mt-6 text-[15px] font-semibold">Unirse a una red</h2>
      <Join onDone={refresh} />
    </main>
  );
}
