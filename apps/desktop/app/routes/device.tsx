// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useRef, useState } from 'react';
import { useParams } from 'react-router';
import { PanelButton, PingButton } from '../device-actions';
import { TrafficHistory } from '../device-history';
import type { DeviceHistory } from '../lib/history';
import type { Rate } from '../lib/agent';
import { ago, bytes, ipKind, levelTone, pathLabel, peerName, selfName, type Tone } from '../lib/format';
import { useAgent } from '../lib/live';
import { setName, useNames } from '../lib/names';
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
    <Card>{addrs.length ? addrs.map((a) => <Stack key={a} label={ipKind(a)}>{a}<CopyButton value={a} label={`${ipKind(a)} ${a}`} /><CopyButton value={`ssh ${a}`} label="comando SSH" icon="terminal" text="ssh" /><CopyButton value={`ping ${a}`} label="comando ping" icon="pulse" text="ping" /></Stack>) : <Row label="Direcciones">no las informa este agente</Row>}</Card>
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
        <Row label="Clave pública">{`${status.public_key.slice(0, 16)}…`}<CopyButton value={status.public_key} label="clave pública" /></Row>
      </Card>
    </>
  );
}


function PeerDetail({ peer, rate, network, history }: { peer: Peer; rate?: Rate; network: string; history?: DeviceHistory }) {
  const names = useNames();
  const { label, tone } = pathLabel(peer.path);
  const used = peer.paths.find((p) => p.kind === peer.path);
  const ready = peer.paths.filter((p) => p.ready).map((p) => p.kind).join(', ');
  const traffic = (total: number, perSec?: number) => `${bytes(total)}${perSec == null ? '' : `  (${bytes(perSec)}/s)`}`;
  return (
    <>
      <Title id={peer.public_key} name={peerName(peer, names)} editable />
      <State tone={tone}>{label}</State>
      {peer.path === 'none' && <p className="mb-4 text-mut">Sin camino activo: el agente lo establece al enviar tráfico. Si sigue sin conexión al usarlo, comprueba que el dispositivo esté encendido.</p>}
      <div className="mb-4 flex flex-wrap gap-3"><PingButton network={network} addrs={peer.addrs} /><PanelButton network={network} deviceId={peer.device_id} /></div>
      <Addresses addrs={peer.addrs} />
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
        {peer.probe && (peer.probe.failures > 0 || peer.probe.hold_secs != null) && <Row label="Reintentos de conexión">{`${peer.probe.failures} fallos · ${peer.probe.hold_secs == null ? 'sin espera de sondeo' : `${peer.probe.hold_secs} s hasta el próximo intento`}`}</Row>}
        <Row label="Clave pública">{`${peer.public_key.slice(0, 16)}…`}<CopyButton value={peer.public_key} label="clave pública" /></Row>
      </Card>
      <TrafficHistory history={history} />
    </>
  );
}

export default function Device() {
  const { id = '' } = useParams();
  const { sample, error, network } = useAgent();
  if (!sample) return <Empty>{error?.message ?? 'Sin datos del agente.'}</Empty>;
  if (id === 'self') return <SelfDetail reply={sample.reply} />;
  const peer = sample.reply.status.peers.find((p) => p.public_key === id);
  return peer ? <PeerDetail key={`${network}-${id}`} peer={peer} network={network} history={sample.history?.devices[id]} rate={sample.rates[id]} /> : <Empty>Este dispositivo ya no está en la configuración.</Empty>;
}
