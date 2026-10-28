// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { changeNetwork, deviceFingerprint, inspectCode, joinNetwork, migrateLegacy, readLegacy, readNetworks, stateText, stateTone, type CodeInfo, type Network } from '../lib/networks';
import { Card, CopyButton, Dot, Empty, Row } from '../ui';

const button = 'cursor-default rounded-md border border-line bg-card px-3 py-1 text-sm disabled:opacity-50';
const primary = 'cursor-default rounded-md bg-sel px-3 py-1 text-sm text-white disabled:opacity-50';

function NetworkCard({ net, busy, onChange }: { net: Network; busy: boolean; onChange: (op: 'up' | 'down' | 'leave', id: string) => void }) {
  const [leaving, setLeaving] = useState(false);
  const stopped = net.state === 'detenida';
  return (
    <section aria-label={net.name} className="mb-4 rounded-[10px] bg-card px-3.5 py-3">
      <div className="flex items-start gap-2">
        <Dot tone={stateTone[net.state]} list />
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">{net.name}</h2>
          <div className="text-xs text-mut">{stateText[net.state]} · {net.server}</div>
          {net.detail && <div className="mt-1 text-xs text-bad">{net.detail}</div>}
        </div>
        <div className="flex gap-2">
          <button type="button" className={button} disabled={busy} onClick={() => onChange(stopped ? 'up' : 'down', net.network_id)}>
            {stopped ? 'Conectar' : 'Desconectar'}
          </button>
          <button type="button" className={button} disabled={busy} onClick={() => setLeaving(true)}>Salir de la red…</button>
        </div>
      </div>
      {leaving && (
        <div role="alertdialog" aria-label={`Salir de ${net.name}`} className="mt-3 rounded-md border border-line p-3 text-sm">
          <p>Se revocará el dispositivo en el servidor y se borrarán las claves de este equipo para «{net.name}». Si el servidor no responde, no se borrará nada. No se puede deshacer.</p>
          <div className="mt-2 flex gap-2">
            <button type="button" className={primary} disabled={busy} onClick={() => { setLeaving(false); onChange('leave', net.network_id); }}>Salir de la red</button>
            <button type="button" className={button} onClick={() => setLeaving(false)}>Cancelar</button>
          </div>
        </div>
      )}
    </section>
  );
}

function Legacy({ onDone }: { onDone: () => void }) {
  const [asking, setAsking] = useState(false);
  const [name, setName] = useState('');
  const migrate = useMutation({ mutationFn: migrateLegacy, onSuccess: () => { setAsking(false); onDone(); } });
  return (
    <section aria-label="Instalación heredada" className="mb-4 rounded-[10px] bg-warn/15 px-3.5 py-3 text-sm">
      <p>Hay una instalación heredada pendiente de importar</p>
      {!asking && <button type="button" className={`${primary} mt-2`} onClick={() => setAsking(true)}>Importar red existente</button>}
      {asking && (
        <div role="alertdialog" aria-label="Importar red existente" className="mt-2">
          <p>Se parará la instancia actual de este equipo y se importará como una red más. El túnel se cortará unos segundos.</p>
          <label htmlFor="legacy-name" className="mt-2 block">Nombre de la red (opcional)</label>
          <input
            id="legacy-name"
            maxLength={64}
            autoComplete="off"
            spellCheck={false}
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="mt-1 w-64 select-text rounded-lg border border-line bg-bg px-2.5 py-1 text-fg"
          />
          <span className="ml-2 text-xs text-mut">Letras, números, espacio, punto, guion y guion bajo.</span>
          <div className="mt-2 flex items-center gap-2">
            <button type="button" className={primary} disabled={migrate.isPending} onClick={() => migrate.mutate(name)}>Importar</button>
            <button type="button" className={button} disabled={migrate.isPending} onClick={() => { setAsking(false); migrate.reset(); }}>Cancelar</button>
            {migrate.isPending && <span role="status" className="text-mut">Esperando la autorización del sistema…</span>}
          </div>
        </div>
      )}
      {migrate.error && <p role="alert" className="mt-2 text-bad">{migrate.error.message}</p>}
    </section>
  );
}

function Join({ onDone }: { onDone: () => void }) {
  const [code, setCode] = useState('');
  const [info, setInfo] = useState<CodeInfo>();
  const check = useMutation({ mutationFn: inspectCode, onSuccess: setInfo });
  const join = useMutation({ mutationFn: joinNetwork, onSuccess: onDone });
  const reset = () => { setInfo(undefined); check.reset(); join.reset(); };

  if (join.isSuccess && info) {
    const fp = deviceFingerprint(join.data);
    return (
      <div role="status" className="rounded-[10px] bg-card p-3.5 text-sm">
        <p>{info.has_token ? `Equipo unido a «${info.name}».` : `Solicitud enviada a «${info.name}»: un administrador debe aprobar este equipo.`}</p>
        {fp && <p className="mt-1">Huella del dispositivo (la compara el administrador): <span className="select-text font-mono">{fp}</span><CopyButton value={fp} label="huella del dispositivo" /></p>}
        <button type="button" className={`${button} mt-2`} onClick={() => { reset(); setCode(''); }}>Unirme a otra red</button>
      </div>
    );
  }

  if (info) {
    return (
      <div role="group" aria-label="Confirmar unión" className="rounded-[10px] bg-card px-3.5 py-3">
        <Card>
          <Row label="Red">{info.name}</Row>
          <Row label="Servidor">{info.server}</Row>
          <Row label="Id de la red">{info.network_id}</Row>
          <Row label="Huella de la raíz">{info.root_fingerprint}<CopyButton value={info.root_fingerprint} label="huella de la raíz" /></Row>
          <Row label="Tipo de código">{info.has_token ? 'Con token de un solo uso' : 'De solicitud: un administrador debe aprobar este equipo'}</Row>
        </Card>
        <p className="mt-3 text-sm">Compara el nombre, el servidor y la huella con los que te dio quien te pasó el código, <strong>por otro canal</strong>. Una red puede instalar rutas privadas y DNS para dominios concretos en este equipo.</p>
        <div className="mt-3 flex items-center gap-2">
          <button type="button" className={primary} disabled={join.isPending} onClick={() => join.mutate(code)}>Unirme a esta red</button>
          <button type="button" className={button} disabled={join.isPending} onClick={reset}>Cancelar</button>
          {join.isPending && <span role="status" className="text-sm text-mut">Esperando la autorización del sistema…</span>}
        </div>
        {join.error && <p role="alert" className="mt-2 text-sm text-bad">{join.error.message}</p>}
      </div>
    );
  }

  return (
    <form className="rounded-[10px] bg-card px-3.5 py-3" onSubmit={(e) => { e.preventDefault(); if (code.trim()) check.mutate(code); }}>
      <label htmlFor="join-code" className="block text-sm">Código de red</label>
      <textarea
        id="join-code"
        rows={3}
        spellCheck={false}
        autoComplete="off"
        placeholder="ATENEA1-…"
        value={code}
        onChange={(e) => { setCode(e.target.value); check.reset(); }}
        className="mt-1 w-full select-text rounded-lg border border-line bg-bg px-2.5 py-1.5 font-mono text-xs text-fg"
      />
      <div className="mt-2 flex items-center gap-2">
        <button type="submit" className={primary} disabled={!code.trim() || check.isPending}>Revisar código</button>
        <span className="text-xs text-mut">Todavía no se cambia nada: primero verás a qué red te unes.</span>
      </div>
      {check.error && <p role="alert" className="mt-2 text-sm text-bad">{check.error.message}</p>}
    </form>
  );
}

export default function Networks() {
  const client = useQueryClient();
  const list = useQuery({ queryKey: ['networks'], queryFn: readNetworks, refetchInterval: 5000 });
  const refresh = () => { void client.invalidateQueries({ queryKey: ['networks'] }); void client.invalidateQueries({ queryKey: ['agent'] }); void client.invalidateQueries({ queryKey: ['legacy'] }); };
  const change = useMutation({ mutationFn: ({ op, id }: { op: 'up' | 'down' | 'leave'; id: string }) => changeNetwork(op, id), onSuccess: refresh });
  // Un fallo al consultar la instalación heredada no impide usar las redes: simplemente no hay banner.
  const legacy = useQuery({ queryKey: ['legacy'], queryFn: readLegacy, refetchInterval: 5000 });

  return (
    <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
      <h1 className="mb-3 text-xl font-semibold">Redes</h1>
      {legacy.data && <Legacy onDone={refresh} />}
      {change.isPending && <p role="status" className="mb-3 text-sm text-mut">Esperando la autorización del sistema…</p>}
      {change.error && <p role="alert" className="mb-3 text-sm text-bad">{change.error.message}</p>}
      {list.error && <p role="alert" className="mb-3 text-sm text-bad">{list.error.message}</p>}
      {list.data?.length === 0 && <Empty>No hay redes unidas con un código.</Empty>}
      {list.data?.map((n) => <NetworkCard key={n.network_id} net={n} busy={change.isPending} onChange={(op, id) => change.mutate({ op, id })} />)}
      <h2 className="mb-2 mt-6 text-[15px] font-semibold">Unirse a una red</h2>
      <Join onDone={refresh} />
    </main>
  );
}
