// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { memo, useMemo, useState } from 'react';
import { NavLink, Outlet } from 'react-router';
import { bytes, levelTone, networkSummary, pathLabel, peerName, selfName, type Tone } from '../lib/format';
import { useAgent } from '../lib/live';
import { useNames } from '../lib/names';
import { Dot, Empty, NoNetworks } from '../ui';

type Entry = { id: string; name: string; addr: string; tone: Tone; haystack: string };

const Item = memo(function Item({ e }: { e: Entry }) {
  return (
    <div className="mb-1">
      <NavLink to={`/devices/${encodeURIComponent(e.id)}`} className={({ isActive }) => `flex cursor-default items-start gap-2 rounded-lg px-2 py-1.5 ${isActive ? 'bg-sel text-white' : ''}`}>
        <Dot tone={e.tone} list />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm" title={e.name}>{e.name}</span>
          {e.addr && <span className="block truncate text-xs opacity-70" title={e.addr}>{e.addr}</span>}
        </span>
      </NavLink>
    </div>
  );
});

const Group = memo(function Group({ title, entries }: { title: string; entries: Entry[] }) {
  if (!entries.length) return null;
  return (
    <section aria-label={title}>
      <h2 className="mx-2 mb-1 mt-3.5 text-[13px] font-semibold text-mut">{title}</h2>
      {entries.map((e) => <Item key={e.id} e={e} />)}
    </section>
  );
});

export default function Devices() {
  const agent = useAgent();
  const names = useNames();
  const [query, setQuery] = useState('');
  const sample = agent.sample;
  const status = sample?.reply.status;
  const level = sample?.reply.health.level;

  const { selfEntries, peerEntries } = useMemo(() => {
    const self: Entry[] = status && level ? [{ id: 'self', name: selfName(status, names), addr: status.addrs[0] ?? '', tone: levelTone(level), haystack: `${selfName(status, names)} ${status.hostname ?? ''} este dispositivo ${status.addrs.join(' ')} ${status.public_key}` }] : [];
    const peers: Entry[] = (status?.peers ?? []).filter((p) => p.role !== 'server').map((p) => ({ id: p.public_key, name: peerName(p, names), addr: p.addrs[0] ?? '', tone: pathLabel(p.path).tone, haystack: `${peerName(p, names)} ${p.name ?? ''} ${p.addrs.join(' ')} ${p.public_key}` }));
    const q = query.trim().toLowerCase();
    const match = (e: Entry) => !q || e.haystack.toLowerCase().includes(q);
    return { selfEntries: self.filter(match), peerEntries: peers.filter(match) };
  }, [status, level, names, query]);
  const summary = useMemo(() => networkSummary((status?.peers ?? []).filter((p) => p.role !== 'server'), sample?.rates ?? {}), [status, sample?.rates]);

  if (agent.empty) return <NoNetworks />;

  return (
    <>
      <div className="flex w-[300px] shrink-0 flex-col border-r border-line px-3 py-4">
        <h1 className="mb-1 text-xl font-semibold">Dispositivos</h1>
        {sample && <p aria-label="Resumen de la red" className="mb-3 text-xs text-mut"><span className="whitespace-nowrap">{`↓ ${bytes(summary.down)}/s · ↑ ${bytes(summary.up)}/s`}</span>{' · '}<span className="whitespace-nowrap">{`${summary.peers} ${summary.peers === 1 ? 'par' : 'pares'}${summary.peers ? ` (${[summary.direct && `${summary.direct} directo`, summary.relay && `${summary.relay} relay`, summary.other && `${summary.other} otros`].filter(Boolean).join(', ')})` : ''}`}</span></p>}
        <input
          type="search"
          aria-label="Buscar dispositivos"
          placeholder="Buscar por nombre, IP o clave"
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="w-full select-text rounded-lg border border-line bg-card px-2.5 py-1.5 text-fg"
        />
        <div className="mt-2 flex-1 overflow-y-auto">
          {sample ? <><Group title="Local" entries={selfEntries} /><Group title="Dispositivos" entries={peerEntries} />{!selfEntries.length && !peerEntries.length && <Empty>Sin resultados.</Empty>}</> : <Empty>Sin datos del agente.</Empty>}
        </div>
      </div>
      <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5"><Outlet context={agent} /></main>
    </>
  );
}
