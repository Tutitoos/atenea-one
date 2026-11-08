// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from 'react-router';
import { readNotifications, setNotifications } from '../lib/networks';
import { Autostart } from '../preferences';
import { OwnServices } from '../services';
import { bytes, int, num, pct, text } from '../lib/format';
import { useAgent } from '../lib/live';
import { buildReport, groupCounters } from '../lib/report';
import type { Status } from '../lib/status';
import { useCopy } from '../toast';
import { button, Card, Empty, Heading, Row } from '../ui';

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

const ERRORS: Record<string, true> = { tx_encrypt_failed: true, rx_init_invalid: true, relay_renewal_failed: true, denied_no_lease: true, config_rejected: true };
const HINTS: Record<string, string> = {
  path_lost: 'caminos que dejaron de responder',
  path_switches: 'cambios entre directo y relay',
  probe_failed: 'sondeos de camino sin respuesta',
  relay_renewal_failed: 'renovaciones de la asignación de relay fallidas',
  tx_init: 'handshakes iniciados',
  rx_init_invalid: 'handshakes recibidos no válidos',
  config_applied: 'configuraciones aplicadas',
  config_rejected: 'configuraciones rechazadas',
  denied_no_lease: 'paquetes denegados por no haber lease vigente',
  tx_acl_denied: 'salientes denegados por la política',
  rx_acl_denied: 'entrantes denegados por la política',
  rx_malformed: 'paquetes recibidos mal formados',
  rx_oversize: 'paquetes recibidos demasiado grandes',
  tx_malformed: 'paquetes propios mal formados',
  tx_bad_source: 'salientes con origen no permitido',
  tx_no_route: 'salientes sin ruta a ningún par',
  tx_encrypt_failed: 'fallos al cifrar',
};

export default function Diagnostics() {
  const { sample, error, empty, stale, network } = useAgent();
  const [all, setAll] = useState(false);
  const copy = useCopy();
  const client = useQueryClient();
  const status = sample?.reply.status;
  return (
    <main aria-busy={stale} className={`min-w-0 flex-1 overflow-y-auto px-6 py-5 ${stale ? 'opacity-60' : ''}`}>
      <h1 className="text-xl font-semibold">Diagnóstico</h1>
      {empty ? <p className="mt-4 text-mut">Sin redes. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link> o sigue el <Link to="/welcome" className="text-sel underline">asistente</Link>.</p>
        : !status || !sample ? <>
          <Empty tone={error ? 'error' : undefined}>{error?.message ?? 'Sin datos del agente.'}</Empty>
          {error && <button type="button" className={button} onClick={() => { void client.invalidateQueries({ queryKey: ['agent'] }); }}>Reintentar</button>}
        </>
        : <>
          {stale && <p className="mt-4 text-sm text-mut">Actualizando…</p>}
          <Heading>Informe</Heading>
          <p className="mb-2 text-sm text-mut">Resumen en texto para pegar en un correo o una incidencia: estado, lease, NAT, pares por camino, contadores distintos de cero, relays y errores de sincronización. Va anonimizado: sin IP, claves, nombres de equipo ni direcciones.</p>
          {!stale && <button type="button" className={button} onClick={() => { void copy(buildReport({ reply: sample.reply, network }), 'informe de diagnóstico'); }}>Copiar informe de diagnóstico</button>}
          <AgentDiagnostics status={status} all={all} onAll={() => setAll((v) => !v)} />
        </>}
      <Notifications />
      <Autostart />
      {!empty && status && <OwnServices status={status} network={network} stale={stale} />}
    </main>
  );
}

const none = (error: string | null | undefined) => <span className={error ? 'text-bad' : 'text-mut'}>{error ?? 'ninguno'}</span>;

function AgentDiagnostics({ status, all, onAll }: { status: Status; all: boolean; onAll: () => void }) {
  const { config, sync, coordinator, relay, counters } = status;
  const entries = relay?.entries ?? [];
  const drops = Object.entries(relay?.drops ?? {});
  const groups = groupCounters(counters, all);
  return (
    <>
      <Heading>Errores</Heading>
      <Card>
        <Row label="Configuración">{none(config.last_error)}</Row>
        <Row label="Control plane (servidor de control)">{none(sync?.last_error)}</Row>
        <Row label="Coordinator (coordinador)">{none(coordinator?.last_error)}</Row>
      </Card>
      <Heading>{`Relays (${entries.length})`}</Heading>
      <p className="mb-2 text-sm text-mut">Los bytes del relay son datagramas cifrados del relay y no equivalen a los bytes de datos por dispositivo.</p>
      <Card>
        {entries.length ? entries.map((e, i) => {
          const sent = num(e.pings_sent), lost = num(e.pings_lost);
          return (
            <Row key={`${text(e.peer)}-${text(e.transport)}-${i}`} label={`${text(e.peer)?.slice(0, 8) ?? '?'} · ${text(e.transport) ?? ''}`}>
              {[
                text(e.state),
                e.rtt_ms == null ? null : `${num(e.rtt_ms)} ms`,
                sent > 0 ? `pings perdidos ${pct(lost, sent)} (${int(lost)}/${int(sent)})` : null,
                `enviados ${bytes(num(e.tx_bytes))} (${int(num(e.tx_packets))} paq.)`,
                `recibidos ${bytes(num(e.rx_bytes))} (${int(num(e.rx_packets))} paq.)`,
                typeof e.peer_ready === 'boolean' ? (e.peer_ready ? 'par listo' : 'par no listo') : null,
                text(e.last_error) ? `error: ${text(e.last_error)}` : null,
              ].filter(Boolean).join(' · ')}
            </Row>
          );
        }) : <Row label="Asignaciones">Este equipo no usa ningún relay ahora</Row>}
      </Card>
      {drops.length > 0 && <><Heading>Descartes del relay</Heading><Card>{drops.map(([k, v]) => <Row key={k} label={k}>{int(v)}</Row>)}</Card></>}
      <Heading>Contadores</Heading>
      <p className="mb-2 text-sm text-mut">Totales desde el arranque del agente. En ámbar, los que indican un error.</p>
      <label className="mb-2 flex items-center gap-2 text-sm"><input type="checkbox" checked={all} onChange={onAll} />Mostrar todos (incluidos los que valen 0)</label>
      {groups.length ? groups.map((g) => (
        <section key={g.title} aria-label={g.title}>
          <h3 className="mb-1 mt-3 text-sm font-semibold text-mut">{g.title}</h3>
          <Card>{g.items.map(([k, v]) => (
            <Row key={k} label={k}>
              <span className={ERRORS[k] && v > 0 ? 'text-warn' : undefined}>{int(v)}</span>{HINTS[k] && <span className="ml-2 text-xs text-mut">{HINTS[k]}</span>}
            </Row>
          ))}</Card>
        </section>
      )) : <p className="text-mut">Todos los contadores valen 0.</p>}
    </>
  );
}
