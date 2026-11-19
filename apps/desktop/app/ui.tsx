// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { Link } from 'react-router';
import type { Tone } from './lib/format';
import { useCopy, useToast } from './toast';

const bg: Record<Tone, string> = { ok: 'bg-ok', warn: 'bg-accent-200', bad: 'bg-no', off: 'bg-gray-500' };
const fg: Record<Tone, string> = { ok: 'text-ok', warn: 'text-accent-200', bad: 'text-no', off: 'text-gray-400' };

export const Dot = ({ tone, list = false }: { tone: Tone; list?: boolean }) => <span aria-hidden="true" className={`${list ? 'mt-1.5 ' : ''}inline-block size-2.5 shrink-0 ${bg[tone]}`} />;

export const Pill = ({ tone, children }: { tone: Tone; children: ReactNode }) => (
  <span role="status" className={`ml-auto border border-current px-3 py-0.5 font-mono text-xs uppercase ${fg[tone]}`}>{children}</span>
);

export const Heading = ({ children }: { children: ReactNode }) => <h2 className="mb-2 mt-6 font-mono text-[15px] font-semibold text-gray-100">{children}</h2>;

export const Card = ({ children }: { children: ReactNode }) => <dl className="bg-panel border border-rule px-3.5">{children}</dl>;

export function Row({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="flex justify-between gap-3 border-b border-rule py-2.5 last:border-b-0">
      <dt className="max-w-[40%] shrink-0 font-mono text-gray-400">{label}</dt>
      <dd className="min-w-0 select-text break-words text-right tabular-nums">{children ?? '—'}</dd>
    </div>
  );
}

const icons = {
  copy: <><rect x="8" y="3" width="8" height="4" rx="1" /><path d="M8 5H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2" /></>,
  terminal: <><path d="m4 17 6-6-6-6" /><path d="M12 19h8" /></>,
  pulse: <path d="M22 12h-4l-3 9L9 3l-3 9H2" />,
};

/** Botón de copiar con icono propio por tipo (`copy`, `terminal`, `pulse`) y, si se da, una etiqueta corta visible. */
export function CopyButton({ value, label, icon = 'copy', text }: { value: string; label: string; icon?: keyof typeof icons; text?: string }) {
  const [state, setState] = useState<'idle' | 'done' | 'failed'>('idle');
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const name = `Copiar ${label}`;
  const show = (next: 'done' | 'failed') => { clearTimeout(timer.current); setState(next); timer.current = setTimeout(() => setState('idle'), 1500); };
  const doCopy = useCopy();
  const announced = useToast() !== null;
  const copy = () => { void doCopy(value, label).then((ok) => show(ok ? 'done' : 'failed')); };
  return (
    <>
      <button type="button" aria-label={name} title={state === 'done' ? 'Copiado' : state === 'failed' ? 'No se pudo copiar' : name} className={`inline-flex h-6 cursor-default items-center justify-center gap-1 text-accent-200 align-middle ${text ? ' border border-rule bg-shell px-2 text-xs' : 'ml-2 size-6 '}`} onClick={copy}>
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          {state === 'done' ? <path d="m5 12 4 4L19 6" /> : icons[icon]}
        </svg>
        {text && <span aria-hidden="true">{text}</span>}
      </button>
      {/* Sin `ToastProvider` (pantallas sueltas) el fallo se anuncia aquí; con él, lo anuncia el aviso. */}
      {state === 'failed' && !announced && <span role="status" className="sr-only">No se pudo copiar</span>}
    </>
  );
}

export const Empty = ({ children, tone }: { children: ReactNode; tone?: 'error' | 'loading' }) => (
  <p role={tone === 'error' ? 'alert' : tone === 'loading' ? 'status' : undefined} className={`p-4 ${tone === 'error' ? 'text-no' : 'text-gray-400'}`}>{children}</p>
);

/** Bloque gris de carga; la animación se apaga con `prefers-reduced-motion` (ver styles.css). */
export const Skeleton = ({ className = '' }: { className?: string }) => <div aria-hidden="true" className={`skeleton bg-raise ${className}`} />;

/** Región de primera carga: marcada como ocupada y con un texto que solo leen los lectores de pantalla. */
export const Loading = ({ children, className }: { children: ReactNode; className?: string }) => (
  <div aria-busy="true" className={className}>
    <span className="sr-only">Cargando…</span>
    {children}
  </div>
);

export const button = 'cursor-default border border-rule-strong bg-panel px-3 py-1 font-mono text-sm text-gray-100 disabled:opacity-50';
export const primary = 'cursor-default border border-accent-200 bg-accent-200 px-3 py-1 font-mono text-sm text-shell disabled:opacity-50';
export const danger = 'cursor-default border border-no bg-transparent px-3 py-1 font-mono text-sm text-no disabled:opacity-50';

/** Estado vacío de las pantallas que dependen de una red: sin redes unidas no hay nada que enseñar. */
export const NoNetworks = () => (
  <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
    <p className="text-gray-400">Sin redes. Únete a una red desde <Link to="/networks" className="text-accent-200 underline">Redes</Link> o sigue el <Link to="/welcome" className="text-accent-200 underline">asistente</Link>.</p>
  </main>
);

export type TabDef = { id: string; label: string; panel: ReactNode };

/** Pestañas del detalle de un dispositivo. Una sola no pinta barra. Solo se monta el panel activo; flechas, Inicio y Fin cambian de pestaña. */
export function Tabs({ tabs, label }: { tabs: TabDef[]; label: string }) {
  const base = useId();
  const [selected, setSelected] = useState(tabs[0]?.id);
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});
  const active = tabs.find((t) => t.id === selected) ?? tabs[0];
  if (!active) return null;
  if (tabs.length === 1) return <>{active.panel}</>;
  const go = (id: string) => { setSelected(id); refs.current[id]?.focus(); };
  const onKey = (e: KeyboardEvent) => {
    const i = tabs.indexOf(active);
    const next = e.key === 'ArrowRight' ? (i + 1) % tabs.length : e.key === 'ArrowLeft' ? (i - 1 + tabs.length) % tabs.length : e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : -1;
    if (next >= 0) { e.preventDefault(); go(tabs[next]!.id); }
  };
  return (
    <>
      <div role="tablist" aria-label={label} className="mb-4 flex flex-wrap gap-1 border-b border-rule">
        {tabs.map((t) => (
          <button key={t.id} ref={(el) => { refs.current[t.id] = el; }} type="button" role="tab" id={`${base}-${t.id}`} aria-selected={t === active} aria-controls={`${base}-panel`} tabIndex={t === active ? 0 : -1} onClick={() => setSelected(t.id)} onKeyDown={onKey}
            className={`-mb-px cursor-default border-b-2 px-3 py-1.5 text-sm ${t === active ? 'border-accent-200 font-medium text-accent-200' : 'border-transparent text-gray-400'}`}>{t.label}</button>
        ))}
      </div>
      <div role="tabpanel" id={`${base}-panel`} aria-labelledby={`${base}-${active.id}`}>{active.panel}</div>
    </>
  );
}
