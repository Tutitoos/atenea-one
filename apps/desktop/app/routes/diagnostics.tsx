// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { bytes, num, text } from '../lib/format';
import { useAgent } from '../lib/live';
import { Card, Empty, Heading, Row } from '../ui';

export default function Diagnostics() {
  const { sample, error } = useAgent();
  if (!sample) return <Empty>{error?.message ?? 'Sin datos del agente.'}</Empty>;
  const { config, sync, coordinator, relay, counters } = sample.reply.status;
  const entries = relay?.entries ?? [];
  const drops = Object.entries(relay?.drops ?? {});
  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h2 className="text-xl font-semibold">Diagnóstico</h2>
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
