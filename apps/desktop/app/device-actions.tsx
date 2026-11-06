// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { openPanel, pingDevice } from './lib/networks';
import { button } from './ui';

export function PanelButton({ network, deviceId }: { network: string; deviceId?: string | null }) {
  const action = useMutation({ mutationFn: () => openPanel(network, deviceId) });
  return <div>
    <button type="button" className={button} disabled={action.isPending} onClick={() => action.mutate()}>Abrir en el panel</button>
    {action.error && <p role="alert" className="text-bad">{action.error.message}</p>}
  </div>;
}

/** `auto`: lanza la prueba una vez al pasar a verdadero (el menú de la lista); al volver a falso se rearma. */
export function PingButton({ network, addrs, name = 'Probar conexión', auto = false }: { network: string; addrs: string[]; name?: string; auto?: boolean }) {
  const client = useQueryClient();
  const address = addrs.find((addr) => !addr.includes(':')) ?? addrs[0];
  const ping = useMutation({
    mutationFn: () => pingDevice(network, address),
    onSuccess: () => client.invalidateQueries({ queryKey: ['agent', network] }),
  });
  const fired = useRef(false);
  useEffect(() => {
    if (!auto) fired.current = false;
    else if (!fired.current && address) { fired.current = true; ping.mutate(); }
    // `ping.mutate` es estable: solo importa el cambio de `auto`.
  }, [auto]);
  return <div>
    <button type="button" aria-label={ping.isPending ? 'Probando…' : name} disabled={ping.isPending || !address} className="rounded-md border border-line px-3 py-1 disabled:opacity-50" onClick={() => ping.mutate()}>{ping.isPending ? 'Probando…' : 'Probar conexión'}</button>
    {ping.error && <p role="alert" className="text-bad">{ping.error.message}</p>}
    {ping.data && <div role="status" className="mt-2 text-sm tabular-nums">
      <p>Enviados: {ping.data.sent} · Recibidos: {ping.data.received}</p>
      <p>Pérdida: {ping.data.loss_pct} %</p>
      <p>Mín/med/máx: {ping.data.min_ms ?? '—'} / {ping.data.avg_ms ?? '—'} / {ping.data.max_ms ?? '—'} ms</p>
    </div>}
  </div>;
}
