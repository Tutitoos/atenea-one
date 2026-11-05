// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link } from 'react-router';
import type { Tone } from './lib/format';

const bg: Record<Tone, string> = { ok: 'bg-ok', warn: 'bg-warn', bad: 'bg-bad', off: 'bg-off' };

export const Dot = ({ tone, list = false }: { tone: Tone; list?: boolean }) => <span aria-hidden="true" className={`${list ? 'mt-1.5 ' : ''}inline-block size-2.5 shrink-0 rounded-full ${bg[tone]}`} />;

export const Pill = ({ tone, children }: { tone: Tone; children: ReactNode }) => (
  <span role="status" className={`ml-auto rounded-full px-3 py-0.5 text-xs font-medium text-white ${bg[tone]}`}>{children}</span>
);

export const Heading = ({ children }: { children: ReactNode }) => <h3 className="mb-2 mt-6 text-[15px] font-semibold">{children}</h3>;

export const Card = ({ children }: { children: ReactNode }) => <dl className="rounded-[10px] bg-card px-3.5">{children}</dl>;

export function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex justify-between gap-3 border-b border-line py-2.5 last:border-b-0">
      <dt className="whitespace-nowrap text-mut">{label}</dt>
      <dd className="select-text break-words text-right tabular-nums">{children ?? '—'}</dd>
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
  // `navigator.clipboard` puede faltar (contexto no seguro) o rechazar (permiso denegado).
  const copy = () => { Promise.resolve().then(() => navigator.clipboard.writeText(value)).then(() => show('done'), () => show('failed')); };
  return (
    <>
      <button type="button" aria-label={name} title={state === 'done' ? 'Copiado' : state === 'failed' ? 'No se pudo copiar' : name} className={`inline-flex h-6 cursor-default items-center justify-center gap-1 text-sel align-middle ${text ? 'rounded-md border border-line bg-bg px-2 text-xs' : 'ml-2 size-6 rounded'}`} onClick={copy}>
        <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          {state === 'done' ? <path d="m5 12 4 4L19 6" /> : icons[icon]}
        </svg>
        {text && <span aria-hidden="true">{text}</span>}
      </button>
      {state === 'failed' && <span role="status" className="sr-only">No se pudo copiar</span>}
    </>
  );
}

export const Empty = ({ children }: { children: ReactNode }) => <p className="p-4 text-mut">{children}</p>;

export const button = 'cursor-default rounded-md border border-line bg-card px-3 py-1 text-sm disabled:opacity-50';
export const primary = 'cursor-default rounded-md bg-sel px-3 py-1 text-sm text-white disabled:opacity-50';

/** Estado vacío de las pantallas que dependen de una red: sin redes unidas no hay nada que enseñar. */
export const NoNetworks = () => (
  <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
    <p className="text-mut">Sin redes. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link> o sigue el <Link to="/welcome" className="text-sel underline">asistente</Link>.</p>
  </main>
);
