// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router';
import { PanelButton, PingButton } from '../device-actions';
import { TrafficHistory } from '../device-history';
import type { DeviceHistory } from '../lib/history';
import type { Rate } from '../lib/agent';
import { ago, bytes, int, ipKind, levelTone, pathLabel, peerName, selfName, type Tone } from '../lib/format';
import { useAgent } from '../lib/live';
import { setName, useNames } from '../lib/names';
import type { Peer, Reply } from '../lib/status';
import { PeerServices } from '../services';
import { Card, CopyButton, Dot, Empty, Heading, Loading, Row, Skeleton } from '../ui';

function Title({ id, name, editable, rename = false }: { id: string; name: string; editable: boolean; rename?: boolean }) {
  const names = useNames();
  const [editing, setEditing] = useState(rename);
  // El menú de la lista pide abrir el editor también cuando este detalle ya estaba abierto.
  useEffect(() => { if (rename) setEditing(true); }, [rename]);
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
    <p className="-mt-1 mb-2 text-xs text-mut">Los botones <code>ssh</code> y <code>ping</code> copian el comando para pegarlo en una terminal.</p>
    <Card>{addrs.length ? addrs.map((a) => (
      <div key={a} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 border-b border-line py-2.5 last:border-b-0">
        <dt className="col-start-1 row-start-2 text-xs text-mut">{ipKind(a)}</dt>
        <dd className="col-start-1 row-start-1 select-text break-all text-sm">{a}<CopyButton value={a} label={`${ipKind(a)} ${a}`} /></dd>
        <dd className="col-start-2 row-span-2 row-start-1 flex gap-1.5">
          <CopyButton value={`ssh ${a}`} label="comando SSH" icon="terminal" text="ssh" />
          <CopyButton value={`ping ${a}`} label="comando ping" icon="pulse" text="ping" />
        </dd>
      </div>
    )) : <Row label="Direcciones">no las informa este agente</Row>}</Card>
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
        <Row label="Lease">{left == null ? null : left > 0 ? `quedan ${ago(left)}` : 'caducado'}</Row>
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


function PeerDetail({ peer, rate, network, history, rename, ping, stale }: { peer: Peer; rate?: Rate; network: string; history?: DeviceHistory; rename: boolean; ping: boolean; stale: boolean }) {
  const names = useNames();
  const { label, tone } = pathLabel(peer.path);
  const used = peer.paths.find((p) => p.kind === peer.path);
  const ready = peer.paths.map((p) => <Row key={p.kind} label={`Camino ${pathLabel(p.kind).label.toLowerCase()}`}>{[p.ready ? 'listo' : 'no listo', p.healthy ? 'sano' : 'con fallos', p.last_rx_secs == null ? 'sin recepción' : `sin recibir desde hace ${ago(p.last_rx_secs)}`].join(' · ')}</Row>);
  // `rtt_ms` es el RTT cliente↔relay (no con el par) y en conexión directa nunca existe.
  const latency = peer.path.startsWith('relay') ? <Row label="Latencia al relay">{used?.rtt_ms == null ? null : `${used.rtt_ms} ms`}</Row> : peer.path === 'direct' ? <Row label="Latencia">No medida en conexión directa</Row> : null;
  const traffic = (total: number, perSec?: number) => `${bytes(total)}${perSec == null ? '' : `  (${bytes(perSec)}/s)`}`;
  return (
    <>
      <Title id={peer.public_key} name={peerName(peer, names)} editable={!stale} rename={rename} />
      <State tone={tone}>{label}</State>
      {peer.path === 'none' && <p className="mb-4 text-mut">Sin camino activo: el agente lo establece al enviar tráfico. Si sigue sin conexión al usarlo, comprueba que el dispositivo esté encendido.</p>}
      {/* Con datos de otra red, `fieldset disabled` apaga los botones de acción sin tocarlos uno a uno. */}
      <fieldset disabled={stale} className="mb-4 flex flex-wrap gap-3 border-0 p-0"><PingButton network={network} addrs={peer.addrs} auto={ping} /><PanelButton network={network} deviceId={peer.device_id} /></fieldset>
      <Addresses addrs={peer.addrs} />
      <PeerServices peer={peer} network={network} stale={stale} />
      <Heading>Detalles</Heading>
      <Card>
        <Row label="Nombre en la red">{peer.name}</Row>
        <Row label="Camino en uso">{label}</Row>
        {ready}
        {latency}
        <Row label="Recibido">{traffic(peer.rx_bytes, rate?.down)}</Row>
        <Row label="Enviado">{traffic(peer.tx_bytes, rate?.up)}</Row>
        <Row label="Última sesión">{peer.session ? (peer.last_handshake_secs_ago == null ? '—' : `hace ${ago(peer.last_handshake_secs_ago)}`) : 'sin sesión'}</Row>
        <Row label="Dirección remota">{peer.endpoint}</Row>
        <Row label="Direcciones conocidas">{int(peer.candidates)}</Row>
        {peer.probe && (peer.probe.failures > 0 || peer.probe.hold_secs != null) && <Row label="Reintentos de conexión">{`${peer.probe.failures} fallos · ${peer.probe.hold_secs == null ? 'sin espera de sondeo' : `${peer.probe.hold_secs} s hasta el próximo intento`}`}</Row>}
        <Row label="Clave pública">{`${peer.public_key.slice(0, 16)}…`}<CopyButton value={peer.public_key} label="clave pública" /></Row>
      </Card>
      <TrafficHistory history={history} />
    </>
  );
}

/** El menú de la lista navega aquí con `state` `{ rename }` o `{ ping }`; se consume y se limpia para no repetirse al volver. */
export default function Device() {
  const { id = '' } = useParams();
  const { sample, error, network, stale } = useAgent();
  const location = useLocation();
  const navigate = useNavigate();
  const asked = location.state as { rename?: boolean; ping?: boolean } | null;
  useEffect(() => { if (asked) void navigate(location.pathname, { replace: true, state: null }); }, [asked, location.pathname, navigate]);
  if (!sample) return error ? <Empty>{error.message}</Empty> : (
    <Loading className="space-y-3">
      <Skeleton className="h-6 w-1/3" />
      <Skeleton className="h-4 w-1/5" />
      <Skeleton className="h-32 w-full" />
      <Skeleton className="h-32 w-full" />
    </Loading>
  );
  if (id === 'self') return <SelfDetail reply={sample.reply} />;
  const peer = sample.reply.status.peers.find((p) => p.public_key === id);
  return peer ? <PeerDetail key={`${network}-${id}`} peer={peer} network={network} history={sample.history?.devices[id]} rate={sample.rates[id]} rename={asked?.rename === true && !stale} ping={asked?.ping === true && !stale} stale={stale} /> : <Empty>Este dispositivo ya no está en la configuración.</Empty>;
}
