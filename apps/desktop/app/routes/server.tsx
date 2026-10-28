// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useParams } from 'react-router';
import { PanelButton } from '../device-actions';
import { ago, bytes, num, pathLabel, peerName, serverTone, text } from '../lib/format';
import { useAgent } from '../lib/live';
import { changeNetwork, readNetworks, stateText, stateTone } from '../lib/networks';
import { useNames } from '../lib/names';
import { certificateLeft, groupServers, readServerNetworks } from '../lib/servers';
import type { Reply } from '../lib/status';
import { Card, Dot, Empty, Row } from '../ui';

const heading = 'mb-2 mt-6 text-[15px] font-semibold';

function Services({ status }: { status: Reply['status'] }) {
  const { sync, coordinator, relay } = status;
  const seen = (unix?: number | null) => unix ? `ok hace ${ago(Date.now() / 1000 - unix)}` : null;
  const entries = relay?.entries ?? [];
  return <>
    <div className="mt-1 flex items-center gap-2"><Dot tone={serverTone(status)} /><span>{sync?.last_error ? `Control plane: ${sync.last_error}` : coordinator?.last_error ? `Coordinator: ${coordinator.last_error}` : 'Conectado'}</span></div>
    <h2 className={heading}>Servicios</h2>
    <Card>
      <Row label="Control plane">{sync?.last_error ?? seen(sync?.last_ok_unix)}</Row>
      <Row label="Sincronizaciones">{sync?.sync_ok == null ? null : `${sync.sync_ok} correctas · ${sync.sync_denied ?? 0} denegadas · ${sync.sync_errors ?? 0} con error`}</Row>
      <Row label="Coordinator">{coordinator?.last_error ?? seen(coordinator?.last_announce_ok_unix)}</Row>
      <Row label="Anuncios y avisos">{coordinator?.announces == null ? null : `${coordinator.announces} anuncios · ${coordinator.events ?? 0} avisos`}</Row>
      <Row label="NAT">{status.nat?.mapping}</Row>
      <Row label="IP pública vista">{status.nat?.reflexive}</Row>
    </Card>
    <h2 className={heading}>{`Relays (${entries.length})`}</h2>
    <Card>{entries.length ? entries.map((e, i) => <Row key={`${text(e.relay)}-${text(e.transport)}-${i}`} label={`${text(e.relay) ?? 'relay'} · ${text(e.transport)?.toUpperCase() ?? ''}`}>
      {[text(e.state), e.rtt_ms == null ? null : `${num(e.rtt_ms)} ms`, `↑ ${bytes(num(e.tx_bytes))}`, `↓ ${bytes(num(e.rx_bytes))}`].filter(Boolean).join(' · ')}
    </Row>) : <Row label="Asignaciones">ninguna</Row>}</Card>
  </>;
}

function Information({ info }: { info: Reply['status']['server'] }) {
  const left = certificateLeft(info?.issuer_cert_not_after_unix);
  return <>
    <h2 className={heading}>Información del servidor</h2>
    {!info ? <Empty>Este servidor aún no informa de su versión ni de la caducidad del certificado</Empty> : <Card>
      <Row label="Control plane">{`${info.control_plane.version} · ${info.control_plane.commit.slice(0, 8)}`}</Row>
      {info.coordinator && <Row label="Coordinator">{`${info.coordinator.version} · ${info.coordinator.commit.slice(0, 8)}`}</Row>}
      {info.relay && <Row label="Relay">{`${info.relay.version} · ${info.relay.commit.slice(0, 8)}`}</Row>}
      <Row label="Hora del servidor">{new Date(info.server_time_unix * 1000).toLocaleString('es')}</Row>
      <Row label="Desfase de reloj"><span className={Math.abs(info.clock_skew_secs) > 30 ? 'text-warn' : 'text-mut'}>{`${info.clock_skew_secs} s${Math.abs(info.clock_skew_secs) > 30 ? ' · Los leases dependen del reloj' : ''}`}</span></Row>
      <Row label="Caducidad del certificado de la red">{left == null ? 'no informada' : <span className={left <= 3 * 86400 ? 'text-bad' : left <= 14 * 86400 ? 'text-warn' : 'text-mut'}>{`${left <= 0 ? 'El certificado ha caducado' : `Quedan ${Math.ceil(left / 86400)} días`} · ${new Date(info.issuer_cert_not_after_unix! * 1000).toLocaleString('es')}`}</span>}</Row>
      <Row label="Última lectura">{new Date(info.fetched_unix * 1000).toLocaleString('es')}</Row>
      {info.last_error && <Row label="Error de lectura">{info.last_error}</Row>}
    </Card>}
  </>;
}

export default function Server() {
  const { host = '' } = useParams();
  const { selectNetwork } = useAgent();
  const names = useNames();
  const client = useQueryClient();
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const server = groupServers(networks.error ? [] : networks.data ?? []).find((s) => s.host === host);
  const connected = server?.networks.filter((n) => n.state === 'conectada').map((n) => n.network_id) ?? [];
  const read = useQuery({
    queryKey: ['server-status', host, connected], enabled: connected.length > 0,
    queryFn: () => readServerNetworks(connected),
    refetchInterval: 2000,
  });
  const change = useMutation({
    mutationFn: ({ op, id }: { op: 'up' | 'down'; id: string }) => changeNetwork(op, id),
    onSuccess: async () => { await client.invalidateQueries({ queryKey: ['networks'] }); await client.invalidateQueries({ queryKey: ['agent'] }); await client.invalidateQueries({ queryKey: ['server-status'] }); },
  });
  return <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
    <Link to="/servers" className="text-sel underline">Servidores</Link>
    <h1 className="mt-2 text-xl font-semibold">{host}</h1>
    {networks.error ? <Empty>{networks.error.message}</Empty> : !networks.data ? <Empty>Comprobando…</Empty> : !server ? <Empty>Este servidor ya no tiene redes unidas.</Empty> : <>
      {connected.length > 0 && read.isPending ? <Empty>Comprobando…</Empty> : read.data?.length ? <Services status={read.data[0].status} /> : <Empty>Sin redes conectadas a este servidor</Empty>}
      {read.data?.length ? read.data.map(({ id, status }) => <section key={id} aria-label={`Información de ${server.networks.find((net) => net.network_id === id)?.name}`}>
        <h2 className={heading}>{server.networks.find((net) => net.network_id === id)?.name}</h2>
        <Information info={status.server} />
      </section>) : <Information info={null} />}
      <h2 className={heading}>Dispositivos del servidor</h2>
      <ul>{read.data?.flatMap(({ id, status }) => status.peers.filter((peer) => peer.role === 'server').map((peer) => <li key={`${id}-${peer.public_key}`}>
        <Link to={`/devices/${encodeURIComponent(peer.public_key)}`} className="flex items-center gap-2 rounded-lg bg-card p-3" onClick={() => selectNetwork(id)}>
          <Dot tone={pathLabel(peer.path).tone} />{peerName(peer, names)} · {peer.addrs.join(', ')} · {pathLabel(peer.path).label} · {server.networks.find((net) => net.network_id === id)?.name}
        </Link>
      </li>))}</ul>
      <h2 className={heading}>Redes unidas</h2>
      {change.error && <p role="alert" className="text-bad">{change.error.message}</p>}
      {change.isPending && <p role="status">Esperando la autorización del sistema…</p>}
      {server.networks.map((net) => <section key={net.network_id} aria-label={net.name} className="mb-3 rounded-lg bg-card p-3">
        <div className="flex items-center gap-2"><Dot tone={stateTone[net.state]} /><h3 className="font-semibold">{net.name}</h3><span className="text-mut">{stateText[net.state]}</span></div>
        {net.detail && <p className="text-mut">{net.detail}</p>}
        <div className="mt-2 flex items-center gap-3">
          <button type="button" disabled={change.isPending} className="rounded-md border border-line px-3 py-1 disabled:opacity-50" onClick={() => change.mutate({ op: net.state === 'detenida' ? 'up' : 'down', id: net.network_id })}>{net.state === 'detenida' ? 'Conectar' : 'Desconectar'}</button>
          <Link to="/devices" className="text-sel underline" onClick={() => selectNetwork(net.network_id)}>Ver dispositivos</Link>
          <PanelButton network={net.network_id} />
        </div>
      </section>)}
    </>}
  </main>;
}
