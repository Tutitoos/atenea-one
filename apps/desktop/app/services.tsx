// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { openService, readClient, setServicesPublish } from './lib/networks';
import type { Peer, Status } from './lib/status';
import { button, Card, CopyButton, Heading } from './ui';

type PeerService = NonNullable<Peer['services']>[number];

/** Dirección para copiar o leer (la que construye Go para abrir): IPv4 si la hay, si no IPv6 entre corchetes. */
const hostOf = (addrs: string[]) => { const a = addrs.find((x) => !x.includes(':')) ?? addrs[0]; return a?.includes(':') ? `[${a}]` : a; };

const Notice = () => <p className="mb-2 text-xs text-mut">Que un servicio figure aquí no significa que se pueda alcanzar: lo deciden las políticas de la red y el cortafuegos de cada equipo.</p>;

function Open({ network, deviceId, service, disabled }: { network: string; deviceId?: string | null; service: PeerService; disabled: boolean }) {
  // La interfaz solo envía el id del par y el puerto: Go construye la URL con lo que ese par publicó.
  const action = useMutation({ mutationFn: () => openService(network, deviceId ?? '', service.port) });
  return <>
    <button type="button" className={button} disabled={disabled || action.isPending || !deviceId} aria-label={`Abrir el puerto ${service.port}`} onClick={() => action.mutate()}>Abrir</button>
    {action.error && <p role="alert" className="basis-full text-bad">{action.error.message}</p>}
  </>;
}

/** Servicios que un par publica en esta red (detalle del par). */
export function PeerServices({ peer, network, stale }: { peer: Peer; network: string; stale: boolean }) {
  const list = peer.services ?? [];
  const host = hostOf(peer.addrs);
  return <section aria-label="Servicios">
    <Heading>Servicios</Heading>
    <Notice />
    {list.length === 0 || !host
      ? <p className="text-mut">Este dispositivo no publica servicios en esta red. Solo los publica si tiene la publicación activada y hay una política hacia él.</p>
      : <Card>{list.map((s) => {
        const address = `${s.scheme}://${host}:${s.port}/`;
        return <div key={s.port} className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line py-2.5 last:border-b-0">
          <dt className="min-w-0 flex-1 break-words"><span className="tabular-nums">{s.port}</span>{s.label ? <> · {s.label}</> : null}</dt>
          <dd className="flex items-center gap-2"><span className="rounded border border-line px-1 text-xs">{s.scheme}</span>
            <Open network={network} deviceId={peer.device_id} service={s} disabled={stale} />
            <CopyButton value={address} label={`dirección ${address}`} />
          </dd>
        </div>;
      })}</Card>}
  </section>;
}

/** Interruptor «Publicar mis servicios en esta red» y lista de lo que detecta este equipo (Diagnóstico). */
export function OwnServices({ status, network, stale }: { status: Status; network: string; stale: boolean }) {
  const client = useQueryClient();
  const info = useQuery({ queryKey: ['client'], queryFn: readClient });
  const canJoin = info.data?.can_join === true;
  const on = status.services_publish === true;
  const change = useMutation({
    mutationFn: (next: boolean) => setServicesPublish(network, next),
    onSuccess: () => client.invalidateQueries({ queryKey: ['agent', network] }),
  });
  const own = status.services ?? [];
  return <section aria-label="Servicios">
    <Heading>Servicios</Heading>
    <label className="flex items-center gap-2"><input type="checkbox" role="switch" checked={on} disabled={stale || !canJoin || change.isPending} onChange={(e) => change.mutate(e.target.checked)} />Publicar mis servicios en esta red</label>
    <p className="mt-1 text-sm text-mut">Apagado por defecto. Al activarlo, los dispositivos con una política hacia este equipo ven los puertos TCP que escucha y los abren con un clic. Publicar no abre nada: el acceso lo deciden las políticas y el cortafuegos. Pide la autorización de administrador del sistema.</p>
    {!canJoin && !info.isPending && <p className="mt-1 text-sm text-mut">En este sistema la aplicación no puede cambiar esta opción: se configura en el agente (<code>[services] publish</code> en <code>agent.toml</code>).</p>}
    {change.error && <p role="alert" className="text-bad">{change.error.message}</p>}
    <h3 className="mb-1 mt-4 text-sm font-semibold">Servicios detectados en este equipo</h3>
    {own.length === 0 ? <p className="text-sm text-mut">No se detecta ningún servicio TCP.</p> : <>
      <p className="mb-1 text-sm tabular-nums" aria-label="Puertos detectados">{own.map((s) => s.port).join(', ')}{status.services_truncated ? ' …' : ''}</p>
      {status.services_truncated && <p className="text-sm text-mut">Lista recortada: hay más servicios de los que se muestran.</p>}
    </>}
    <p className="text-sm text-mut">Solo lectura. {on ? 'Se publican en esta red.' : 'No se publican mientras el interruptor esté apagado.'}</p>
  </section>;
}
