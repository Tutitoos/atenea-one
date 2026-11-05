// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { memo, useCallback, useMemo, useRef, useState } from 'react';
import { NavLink, Outlet, useNavigate } from 'react-router';
import { ContextMenu, type MenuItem } from '../context-menu';
import { bytes, levelTone, networkSummary, pathLabel, peerName, selfName, type Tone } from '../lib/format';
import { useAgent } from '../lib/live';
import { useNames } from '../lib/names';
import { Sparkline } from '../sparkline';
import { useCopy } from '../toast';
import { button, Dot, Empty, NoNetworks } from '../ui';

type Point = { at: number; down: number; up: number };
/** `path` solo existe en los pares: el dispositivo local no lleva insignia de camino ni se filtra. */
type Entry = { id: string; name: string; addr: string; address: string; tone: Tone; haystack: string; path?: string; points?: Point[] };
type OpenMenu = (id: string, x: number, y: number, opener: HTMLElement) => void;

const FILTER_KEY = 'atenea.devices.filter';
const FILTERS = [
  { id: 'all', label: 'Todos' },
  { id: 'connected', label: 'Conectados' },
  { id: 'relay', label: 'Por relay' },
  { id: 'off', label: 'Sin conexión' },
] as const;
type Filter = (typeof FILTERS)[number]['id'];

const readFilter = (): Filter => FILTERS.find((f) => f.id === localStorage.getItem(FILTER_KEY))?.id ?? 'all';
const inFilter = (path: string, f: Filter) => f === 'all' || (f === 'connected' ? path === 'direct' || path.startsWith('relay') : f === 'relay' ? path.startsWith('relay') : path === 'none' || path === 'probing');
/** Dirección preferida: IPv4 si la hay, como el botón de probar conexión. */
const preferred = (addrs: string[]) => addrs.find((a) => !a.includes(':')) ?? addrs[0] ?? '';

/** Solo se vuelve a pintar una fila si cambia algo que se ve; las listas de puntos se recrean en cada lectura. */
const sameItem = (a: { e: Entry; onMenu: OpenMenu }, b: { e: Entry; onMenu: OpenMenu }) =>
  a.onMenu === b.onMenu && a.e.id === b.e.id && a.e.name === b.e.name && a.e.addr === b.e.addr && a.e.tone === b.e.tone && a.e.path === b.e.path
  && a.e.points?.length === b.e.points?.length && a.e.points?.at(-1)?.at === b.e.points?.at(-1)?.at;

const Item = memo(function Item({ e, onMenu }: { e: Entry; onMenu: OpenMenu }) {
  const below = (el: HTMLElement) => { const r = el.getBoundingClientRect(); onMenu(e.id, r.left, r.bottom, el); };
  const path = e.path === undefined ? null : pathLabel(e.path);
  return (
    <div className="mb-1 flex items-start gap-0.5">
      <NavLink
        to={`/devices/${encodeURIComponent(e.id)}`}
        onContextMenu={(ev) => { ev.preventDefault(); onMenu(e.id, ev.clientX, ev.clientY, ev.currentTarget); }}
        onKeyDown={(ev) => { if (ev.key === 'ContextMenu' || (ev.shiftKey && ev.key === 'F10')) { ev.preventDefault(); below(ev.currentTarget); } }}
        className={({ isActive }) => `flex min-w-0 flex-1 cursor-default items-start gap-2 rounded-lg px-2 py-1.5 ${isActive ? 'bg-sel text-white' : ''}`}
      >
        <Dot tone={e.tone} list />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm" title={e.name}>{e.name}</span>
          {e.addr && <span className="block truncate text-xs opacity-70" title={e.addr}>{e.addr}</span>}
        </span>
        {path && (
          <span className="flex shrink-0 flex-col items-end gap-0.5">
            <span className="rounded border border-current px-1 text-[11px] leading-4">{path.label}</span>
            <Sparkline points={e.points} />
          </span>
        )}
      </NavLink>
      <button type="button" aria-label={`Acciones de ${e.name}`} aria-haspopup="menu" onClick={(ev) => below(ev.currentTarget)} className="mt-1 size-6 shrink-0 cursor-default rounded text-mut hover:bg-fg/10 focus-visible:outline-2 focus-visible:outline-sel">⋯</button>
    </div>
  );
}, sameItem);

const Group = memo(function Group({ title, entries, onMenu }: { title: string; entries: Entry[]; onMenu: OpenMenu }) {
  if (!entries.length) return null;
  return (
    <section aria-label={title}>
      <h2 className="mx-2 mb-1 mt-3.5 text-[13px] font-semibold text-mut">{title}</h2>
      {entries.map((e) => <Item key={e.id} e={e} onMenu={onMenu} />)}
    </section>
  );
});

export default function Devices() {
  const agent = useAgent();
  const names = useNames();
  const navigate = useNavigate();
  const copy = useCopy();
  const [query, setQuery] = useState('');
  const [filter, setFilterState] = useState(readFilter);
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const sample = agent.sample;
  const status = sample?.reply.status;
  const level = sample?.reply.health.level;
  const history = sample?.history;

  const setFilter = (f: Filter) => { localStorage.setItem(FILTER_KEY, f); setFilterState(f); };
  const openMenu = useCallback<OpenMenu>((id, x, y, el) => { opener.current = el; setMenu({ id, x, y }); }, []);
  const closeMenu = useCallback((restoreFocus: boolean) => { setMenu(null); if (restoreFocus) opener.current?.focus(); }, []);

  const { selfEntries, peerEntries, counts } = useMemo(() => {
    const addrs = status?.addrs ?? [];
    const self: Entry[] = status && level ? [{ id: 'self', name: selfName(status, names), addr: addrs[0] ?? '', address: preferred(addrs), tone: levelTone(level), haystack: `${selfName(status, names)} ${status.hostname ?? ''} este dispositivo ${addrs.join(' ')} ${status.public_key}` }] : [];
    const peers: Entry[] = (status?.peers ?? []).filter((p) => p.role !== 'server').map((p) => ({ id: p.public_key, name: peerName(p, names), addr: p.addrs[0] ?? '', address: preferred(p.addrs), tone: pathLabel(p.path).tone, haystack: `${peerName(p, names)} ${p.name ?? ''} ${p.addrs.join(' ')} ${p.public_key}`, path: p.path, points: history?.devices[p.public_key]?.points }));
    const q = query.trim().toLowerCase();
    const match = (e: Entry) => !q || e.haystack.toLowerCase().includes(q);
    const found = peers.filter(match);
    // Los recuentos son los de la búsqueda actual: lo que dejaría ver cada filtro.
    const counts = Object.fromEntries(FILTERS.map((f) => [f.id, found.filter((e) => inFilter(e.path!, f.id)).length])) as Record<Filter, number>;
    return { selfEntries: self.filter(match), peerEntries: found.filter((e) => inFilter(e.path!, filter)), counts };
  }, [status, level, names, query, filter, history]);
  const summary = useMemo(() => networkSummary((status?.peers ?? []).filter((p) => p.role !== 'server'), sample?.rates ?? {}), [status, sample?.rates]);

  if (agent.empty) return <NoNetworks />;

  const target = menu && [...selfEntries, ...peerEntries].find((e) => e.id === menu.id);
  const items = (e: Entry): MenuItem[] => [
    { label: 'Abrir detalle', onSelect: () => void navigate(`/devices/${encodeURIComponent(e.id)}`) },
    ...(e.address ? [{ label: 'Copiar dirección', onSelect: () => void copy(e.address, 'dirección') }] : []),
    ...(e.path === undefined ? [] : [
      ...(e.address ? [
        { label: 'Copiar comando ssh', onSelect: () => void copy(`ssh ${e.address}`, 'comando ssh') },
        { label: 'Copiar comando ping', onSelect: () => void copy(`ping ${e.address}`, 'comando ping') },
      ] : []),
      // El detalle lee este estado (ver `Device`) para abrir el editor o lanzar la prueba una vez.
      { label: 'Renombrar…', onSelect: () => void navigate(`/devices/${encodeURIComponent(e.id)}`, { state: { rename: true } }) },
      ...(e.address ? [{ label: 'Probar conexión', onSelect: () => void navigate(`/devices/${encodeURIComponent(e.id)}`, { state: { ping: true } }) }] : []),
    ]),
  ];
  const filtered = query.trim() !== '' || filter !== 'all';
  const filterLabel = FILTERS.find((f) => f.id === filter)!.label;

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
        <div role="group" aria-label="Filtrar por conexión" className="mt-2 flex flex-wrap gap-1">
          {FILTERS.map((f) => (
            <button key={f.id} type="button" aria-pressed={filter === f.id} onClick={() => setFilter(f.id)} className={`cursor-default rounded-md px-2 py-0.5 text-xs focus-visible:outline-2 focus-visible:outline-sel ${filter === f.id ? 'bg-sel text-white' : 'border border-line bg-card'}`}>
              {f.label} <span className="tabular-nums">{counts[f.id]}</span>
            </button>
          ))}
        </div>
        <div className="mt-2 flex-1 overflow-y-auto">
          {sample ? <><Group title="Local" entries={selfEntries} onMenu={openMenu} /><Group title="Dispositivos" entries={peerEntries} onMenu={openMenu} />{!selfEntries.length && !peerEntries.length && (
            <div>
              <Empty>Sin resultados.</Empty>
              {filtered && <div className="px-4 pb-2"><p className="mb-2 text-xs text-mut">{[query.trim() && `la búsqueda «${query.trim()}»`, filter !== 'all' && `el filtro «${filterLabel}»`].filter(Boolean).join(' y ')} no deja ningún dispositivo.</p><button type="button" className={button} onClick={() => { setQuery(''); setFilter('all'); }}>Quitar filtros</button></div>}
            </div>
          )}</> : <Empty>Sin datos del agente.</Empty>}
        </div>
      </div>
      <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5"><Outlet context={agent} /></main>
      {menu && target && <ContextMenu label={`Acciones de ${target.name}`} x={menu.x} y={menu.y} items={items(target)} onClose={closeMenu} />}
    </>
  );
}
