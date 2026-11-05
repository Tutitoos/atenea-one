// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from 'react-router';
import { readNotifications, setNotifications } from '../lib/networks';
import { Autostart } from '../preferences';
import { bytes, int, num, pct, text } from '../lib/format';
import { useAgent } from '../lib/live';
import type { Status } from '../lib/status';
import { Card, Empty, Heading, Row } from '../ui';

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

type Group = { title: string; match: RegExp };
// Orden = prioridad: la primera familia que coincide se queda con el contador; lo no reconocido va a «Otros».
const FAMILIES: Group[] = [
  { title: 'Configuración y lease', match: /^(config_|denied_no_lease|lease)/ },
  { title: 'Caminos', match: /^(path_|probe_)/ },
  { title: 'Relay', match: /^relay_/ },
  { title: 'Handshakes', match: /^(tx_init|rx_init|init_|cookie_|tx_cookie|rx_cookie|handshake)/ },
  { title: 'Tráfico denegado o descartado', match: /(_acl_denied|^rx_bad_|^rx_malformed|^rx_oversize|^tx_malformed|^tx_bad_source|^tx_no_route|^tx_encrypt)/ },
];
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

/** Agrupa por familia y, salvo `all`, oculta los que valen 0. Devuelve solo las familias con algo que enseñar. */
export function groupCounters(counters: Record<string, number>, all: boolean) {
  const groups = [...FAMILIES, { title: 'Otros', match: /./ }].map((g) => ({ title: g.title, items: [] as [string, number][] }));
  for (const [k, v] of Object.entries(counters).sort(([a], [b]) => (a < b ? -1 : 1))) {
    if (!all && v === 0) continue;
    const i = FAMILIES.findIndex((f) => f.match.test(k));
    groups[i < 0 ? FAMILIES.length : i].items.push([k, v]);
  }
  return groups.filter((g) => g.items.length);
}

export default function Diagnostics() {
  const { sample, error, empty } = useAgent();
  const [all, setAll] = useState(false);
  const status = sample?.reply.status;
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h2 className="text-xl font-semibold">Diagnóstico</h2>
      <Notifications />
      <Autostart />
      {empty ? <p className="mt-4 text-mut">Sin redes. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link> o sigue el <Link to="/welcome" className="text-sel underline">asistente</Link>.</p>
        : !status ? <Empty>{error?.message ?? 'Sin datos del agente.'}</Empty>
        : <AgentDiagnostics status={status} all={all} onAll={() => setAll((v) => !v)} />}
    </main>
  );
}

function AgentDiagnostics({ status, all, onAll }: { status: Status; all: boolean; onAll: () => void }) {
  const { config, sync, coordinator, relay, counters } = status;
  const entries = relay?.entries ?? [];
  const drops = Object.entries(relay?.drops ?? {});
  const groups = groupCounters(counters, all);
  return (
    <>
      <Heading>Errores</Heading>
      <Card>
        <Row label="Configuración">{config.last_error ?? 'ninguno'}</Row>
        <Row label="Control plane">{sync?.last_error ?? 'ninguno'}</Row>
        <Row label="Coordinator">{coordinator?.last_error ?? 'ninguno'}</Row>
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
                `tx ${bytes(num(e.tx_bytes))} (${int(num(e.tx_packets))} paq.)`,
                `rx ${bytes(num(e.rx_bytes))} (${int(num(e.rx_packets))} paq.)`,
                typeof e.peer_ready === 'boolean' ? (e.peer_ready ? 'par listo' : 'par no listo') : null,
                text(e.last_error) ? `error: ${text(e.last_error)}` : null,
              ].filter(Boolean).join(' · ')}
            </Row>
          );
        }) : <Row label="Asignaciones">ninguna</Row>}
      </Card>
      {drops.length > 0 && <><Heading>Descartes del relay</Heading><Card>{drops.map(([k, v]) => <Row key={k} label={k}>{int(v)}</Row>)}</Card></>}
      <Heading>Contadores</Heading>
      <p className="mb-2 text-sm text-mut">Totales desde el arranque del agente. En ámbar, los que indican un error.</p>
      <label className="mb-2 flex items-center gap-2 text-sm"><input type="checkbox" checked={all} onChange={onAll} />Mostrar todos (incluidos los que valen 0)</label>
      {groups.length ? groups.map((g) => (
        <section key={g.title} aria-label={g.title}>
          <h4 className="mb-1 mt-3 text-sm font-semibold text-mut">{g.title}</h4>
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
