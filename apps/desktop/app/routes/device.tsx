// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useRef, useState } from 'react';
import { useParams } from 'react-router';
import type { Rate } from '../lib/agent';
import { ago, bytes, host, ipKind, levelTone, num, pathLabel, peerName, selfName, serverTone, text, type Tone } from '../lib/format';
import { useAgent } from '../lib/live';
import { setName, useNames } from '../lib/names';
import { setServer, useServers } from '../lib/roles';
import type { Peer, Reply } from '../lib/status';
import { Card, CopyButton, Dot, Empty, Heading, Row, Stack } from '../ui';

function Title({ id, name, editable }: { id: string; name: string; editable: boolean }) {
  const names = useNames();
  const [editing, setEditing] = useState(false);
  const cancelled = useRef(false);
  const save = (value: string) => { setName(id, value); setEditing(false); };
  if (!editable) return <h2 className="text-xl font-semibold">{name}</h2>;
  return (
    <h2 className="text-xl font-semibold break-words">
      {editing ? (
        <input
          aria-label="Nombre local"
          placeholder="Nombre local"
          defaultValue={names[id] ?? ''}
          ref={(el) => el?.focus()}
          onBlur={(e) => { if (!cancelled.current) save(e.currentTarget.value); }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') save(e.currentTarget.value);
            if (e.key === 'Escape') { cancelled.current = true; setEditing(false); }
          }}
          className="w-full select-text rounded-md border border-sel bg-card px-1.5 text-fg"
        />
      ) : (
        <button type="button" title="Cambiar nombre local" onClick={() => { cancelled.current = false; setEditing(true); }} className="cursor-text border-b border-dotted border-mut text-left">{name}</button>
      )}
    </h2>
  );
}

const State = ({ tone, children }: { tone: Tone; children: string }) => <div className="mb-5 mt-1 flex items-center gap-2"><Dot tone={tone} /><span>{children}</span></div>;

const Addresses = ({ addrs }: { addrs: string[] }) => (
  <>
    <Heading>Direcciones del túnel</Heading>
    <Card>{addrs.length ? addrs.map((a) => <Stack key={a} label={ipKind(a)}>{a}</Stack>) : <Row label="Direcciones">no las informa este agente</Row>}</Card>
  </>
);

function SelfDetail({ reply }: { reply: Reply }) {
  const { status, health } = reply;
  const names = useNames();
  const left = status.config.valid_until_unix ? Math.round(status.config.valid_until_unix - Date.now() / 1000) : null;
  const seen = (unix?: number | null) => (unix ? `ok hace ${ago(Date.now() / 1000 - unix)}` : null);
  return (
    <>
      <Title id={status.public_key} name={selfName(status, names)} editable />
      <State tone={levelTone(health.level)}>{health.text}</State>
      <Addresses addrs={status.addrs} />
      <Heading>Detalles</Heading>
      <Card>
        <Row label="Nombre del sistema">{status.hostname}</Row>
        <Row label="Lease">{left == null ? null : left > 0 ? `válido ${ago(left)}` : 'caducado'}</Row>
        <Row label="Configuración">{`v${status.config.version}`}</Row>
        <Row label="NAT">{status.nat?.mapping}</Row>
        <Row label="IP pública vista">{status.nat?.reflexive}</Row>
        <Row label="Control plane">{seen(status.sync?.last_ok_unix)}</Row>
        <Row label="Coordinator">{seen(status.coordinator?.last_announce_ok_unix)}</Row>
        <Row label="Relays asignados">{String(status.relay?.allocations ?? 0)}</Row>
        <Row label="Clave pública">{`${status.public_key.slice(0, 16)}…`}<CopyButton value={status.public_key} /></Row>
      </Card>
    </>
  );
}

function ServerDetail({ status }: { status: Reply['status'] }) {
  const { sync, coordinator, relay } = status;
  const seen = (unix?: number | null) => (unix ? `ok hace ${ago(Date.now() / 1000 - unix)}` : null);
  const tone = serverTone(status);
  const summary = sync?.last_error ? `Control plane: ${sync.last_error}` : coordinator?.last_error ? `Coordinator: ${coordinator.last_error}` : 'Conectado';
  const entries = relay?.entries ?? [];
  return (
    <>
      <Title id="server" name={host(sync?.url) ?? 'Servidor'} editable={false} />
      <State tone={tone}>{summary}</State>
      <Heading>Servicios</Heading>
      <Card>
        <Row label="Control plane">{sync?.last_error ?? seen(sync?.last_ok_unix)}</Row>
        <Row label="Sincronizaciones">{sync?.sync_ok == null ? null : `${sync.sync_ok} correctas · ${sync.sync_denied ?? 0} denegadas · ${sync.sync_errors ?? 0} con error`}</Row>
        <Row label="Coordinator">{coordinator?.last_error ?? seen(coordinator?.last_announce_ok_unix)}</Row>
        <Row label="Anuncios y avisos">{coordinator?.announces == null ? null : `${coordinator.announces} anuncios · ${coordinator.events ?? 0} avisos`}</Row>
      </Card>
      <Heading>{`Relays (${entries.length})`}</Heading>
      <Card>
        {entries.length ? entries.map((e, i) => (
          <Row key={`${text(e.relay)}-${text(e.transport)}-${i}`} label={`${text(e.relay) ?? 'relay'} · ${text(e.transport)?.toUpperCase() ?? ''}`}>
            {[text(e.state), e.rtt_ms == null ? null : `${num(e.rtt_ms)} ms`, `↑ ${bytes(num(e.tx_bytes))}`, `↓ ${bytes(num(e.rx_bytes))}`].filter(Boolean).join(' · ')}
          </Row>
        )) : <Row label="Asignaciones">ninguna</Row>}
      </Card>
    </>
  );
}

function PeerDetail({ peer, rate }: { peer: Peer; rate?: Rate }) {
  const names = useNames();
  const isServer = useServers().includes(peer.public_key);
  const { label, tone } = pathLabel(peer.path);
  const used = peer.paths.find((p) => p.kind === peer.path);
  const ready = peer.paths.filter((p) => p.ready).map((p) => p.kind).join(', ');
  const traffic = (total: number, perSec?: number) => `${bytes(total)}${perSec == null ? '' : `  (${bytes(perSec)}/s)`}`;
  return (
    <>
      <Title id={peer.public_key} name={peerName(peer, names)} editable />
      <State tone={tone}>{label}</State>
      <Addresses addrs={peer.addrs} />
      <label className="mb-1 flex cursor-default items-center gap-2 text-sm">
        <input type="checkbox" checked={isServer} onChange={(e) => setServer(peer.public_key, e.target.checked)} />
        Marcar como servidor (solo en este equipo)
      </label>
      <Heading>Detalles</Heading>
      <Card>
        <Row label="Nombre en la red">{peer.name}</Row>
        <Row label="Camino en uso">{label}</Row>
        <Row label="Caminos listos">{ready}</Row>
        <Row label="Latencia">{used?.rtt_ms == null ? null : `${used.rtt_ms} ms`}</Row>
        <Row label="Recibido">{traffic(peer.rx_bytes, rate?.down)}</Row>
        <Row label="Enviado">{traffic(peer.tx_bytes, rate?.up)}</Row>
        <Row label="Última sesión">{peer.session ? `hace ${ago(peer.last_handshake_secs_ago ?? 0)}` : 'sin sesión'}</Row>
        <Row label="Dirección remota">{peer.endpoint}</Row>
        <Row label="Candidatos">{String(peer.candidates)}</Row>
        <Row label="Clave pública">{`${peer.public_key.slice(0, 16)}…`}<CopyButton value={peer.public_key} /></Row>
      </Card>
    </>
  );
}

export default function Device() {
  const { id = '' } = useParams();
  const { sample, error } = useAgent();
  if (!sample) return <Empty>{error?.message ?? 'Sin datos del agente.'}</Empty>;
  if (id === 'self') return <SelfDetail reply={sample.reply} />;
  if (id === 'server') return sample.reply.status.sync?.url ? <ServerDetail status={sample.reply.status} /> : <Empty>Este agente no tiene un servidor configurado.</Empty>;
  const peer = sample.reply.status.peers.find((p) => p.public_key === id);
  return peer ? <PeerDetail peer={peer} rate={sample.rates[id]} /> : <Empty>Este dispositivo ya no está en la configuración.</Empty>;
}
