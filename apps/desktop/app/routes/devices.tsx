// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { NavLink, Outlet } from 'react-router';
import { levelTone, pathLabel, peerName, type Tone } from '../lib/format';
import { useAgent } from '../lib/live';
import { useNames } from '../lib/names';
import { Dot, Empty } from '../ui';

type Entry = { id: string; name: string; addr: string; tone: Tone; haystack: string };

function Group({ title, entries }: { title: string; entries: Entry[] }) {
  if (!entries.length) return null;
  return (
    <section aria-label={title}>
      <h2 className="mx-2 mb-1 mt-3.5 text-[13px] font-semibold text-mut">{title}</h2>
      {entries.map((e) => (
        <NavLink key={e.id} to={`/devices/${encodeURIComponent(e.id)}`} className={({ isActive }) => `flex cursor-default items-start gap-2 rounded-lg px-2 py-1.5 ${isActive ? 'bg-sel text-white' : ''}`}>
          <Dot tone={e.tone} />
          <span>
            <span className="block text-sm">{e.name}</span>
            {e.addr && <span className="block text-xs opacity-70">{e.addr}</span>}
          </span>
        </NavLink>
      ))}
    </section>
  );
}

export default function Devices() {
  const agent = useAgent();
  const names = useNames();
  const [query, setQuery] = useState('');
  const sample = agent.sample;
  const status = sample?.reply.status;

  const self: Entry[] = status && sample ? [{ id: 'self', name: 'Este dispositivo', addr: status.addrs[0] ?? '', tone: levelTone(sample.reply.health.level), haystack: `este dispositivo ${status.addrs.join(' ')} ${status.public_key}` }] : [];
  const peers: Entry[] = (status?.peers ?? []).map((p) => ({ id: p.public_key, name: peerName(p, names), addr: p.addrs[0] ?? '', tone: pathLabel(p.path).tone, haystack: `${peerName(p, names)} ${p.addrs.join(' ')} ${p.public_key}` }));
  const q = query.trim().toLowerCase();
  const match = (e: Entry) => !q || e.haystack.toLowerCase().includes(q);
  const visible = [...self.filter(match), ...peers.filter(match)];

  return (
    <>
      <div className="flex w-[300px] shrink-0 flex-col border-r border-line px-3 py-4">
        <h1 className="mb-3 text-xl font-semibold">Dispositivos</h1>
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
          {sample ? <><Group title="Local" entries={self.filter(match)} /><Group title="Pares" entries={peers.filter(match)} />{!visible.length && <Empty>Sin resultados.</Empty>}</> : <Empty>Sin datos del agente.</Empty>}
        </div>
      </div>
      <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5"><Outlet context={agent} /></main>
    </>
  );
}
