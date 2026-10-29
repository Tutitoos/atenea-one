// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { readAutostart, readNotifications, setAutostart, setNotifications } from '../lib/networks';
import { bytes, num, text } from '../lib/format';
import { useAgent } from '../lib/live';
import { Card, Empty, Heading, NoNetworks, Row } from '../ui';

function Notifications() {
  const client = useQueryClient();
  const preference = useQuery({ queryKey: ['notifications'], queryFn: readNotifications });
  const change = useMutation({
    mutationFn: setNotifications,
    onSuccess: (_, on) => client.setQueryData(['notifications'], on),
  });
  return <section aria-label="Preferencias">
    <Heading>Notificaciones</Heading>
    <label className="flex items-center gap-2"><input type="checkbox" role="switch" checked={preference.data ?? false} disabled={preference.isPending || !!preference.error || change.isPending} onChange={(event) => change.mutate(event.target.checked)} />Notificaciones del sistema</label>
    <p className="mt-1 text-sm text-mut">Avisa de conexión a relay prolongada, red caída o recuperada y certificado por caducar.</p>
    {(change.error ?? preference.error) && <p role="alert" className="text-bad">{(change.error ?? preference.error)?.message}</p>}
  </section>;
}

function Autostart() {
  const client = useQueryClient();
  const preference = useQuery({ queryKey: ['autostart'], queryFn: readAutostart });
  const change = useMutation({
    mutationFn: setAutostart,
    onSuccess: (_, on) => client.setQueryData(['autostart'], on),
  });
  if (preference.data === null) return null;
  return <section aria-label="Inicio">
    <Heading>Inicio</Heading>
    <label className="flex items-center gap-2"><input type="checkbox" role="switch" checked={preference.data ?? false} disabled={preference.isPending || !!preference.error || change.isPending} onChange={(event) => change.mutate(event.target.checked)} />Abrir al iniciar sesión</label>
    <p className="mt-1 text-sm text-mut">Abre Atenea One en la bandeja al iniciar sesión, sin mostrar la ventana. No cambia la conexión de las redes: las gestiona el sistema.</p>
    {(change.error ?? preference.error) && <p role="alert" className="text-bad">{(change.error ?? preference.error)?.message}</p>}
  </section>;
}

export default function Diagnostics() {
  const { sample, error, empty } = useAgent();
  if (empty) return <NoNetworks />;
  if (!sample) return <Empty>{error?.message ?? 'Sin datos del agente.'}</Empty>;
  const { config, sync, coordinator, relay, counters } = sample.reply.status;
  const entries = relay?.entries ?? [];
  const drops = Object.entries(relay?.drops ?? {});
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h2 className="text-xl font-semibold">Diagnóstico</h2>
      <Notifications />
      <Autostart />
      <Heading>Errores</Heading>
      <Card>
        <Row label="Configuración">{config.last_error ?? 'ninguno'}</Row>
        <Row label="Control plane">{sync?.last_error ?? 'ninguno'}</Row>
        <Row label="Coordinator">{coordinator?.last_error ?? 'ninguno'}</Row>
      </Card>
      <Heading>{`Relays (${entries.length})`}</Heading>
      <Card>
        {entries.length ? entries.map((e, i) => (
          <Row key={`${text(e.peer)}-${text(e.transport)}-${i}`} label={`${text(e.peer)?.slice(0, 8) ?? '?'} · ${text(e.transport) ?? ''}`}>
            {[text(e.state), `tx ${bytes(num(e.tx_bytes))}`, `rx ${bytes(num(e.rx_bytes))}`, num(e.pings_lost) ? `${num(e.pings_lost)} pings perdidos` : null].filter(Boolean).join(' · ')}
          </Row>
        )) : <Row label="Asignaciones">ninguna</Row>}
      </Card>
      {drops.length > 0 && <><Heading>Descartes del relay</Heading><Card>{drops.map(([k, v]) => <Row key={k} label={k}>{String(v)}</Row>)}</Card></>}
      <Heading>Contadores</Heading>
      <Card>{Object.entries(counters).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, v]) => <Row key={k} label={k}>{String(v)}</Row>)}</Card>
    </main>
  );
}
