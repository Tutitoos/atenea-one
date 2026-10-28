// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState, type ReactNode } from 'react';
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

/** Valor destacado con su etiqueta debajo, como la tarjeta de direcciones. */
export function Stack({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col-reverse border-b border-line py-2.5 last:border-b-0">
      <dt className="text-xs text-mut">{label}</dt>
      <dd className="select-text text-sm">{children}</dd>
    </div>
  );
}

export function CopyButton({ value, label }: { value: string; label: string }) {
  const [done, setDone] = useState(false);
  const name = `Copiar ${label}`;
  return (
    <button type="button" aria-label={name} title={done ? 'Copiado' : name} className="ml-2 inline-flex size-6 cursor-default items-center justify-center rounded text-sel align-middle" onClick={() => { void navigator.clipboard.writeText(value).then(() => { setDone(true); setTimeout(() => setDone(false), 1500); }); }}>
      <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        {done ? <path d="m5 12 4 4L19 6" /> : <><rect x="8" y="3" width="8" height="4" rx="1" /><path d="M8 5H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2" /></>}
      </svg>
    </button>
  );
}

export const Empty = ({ children }: { children: ReactNode }) => <p className="p-4 text-mut">{children}</p>;

/** Estado vacío de las pantallas que dependen de una red: sin redes unidas no hay nada que enseñar. */
export const NoNetworks = () => (
  <main className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
    <p className="text-mut">Sin redes. Únete a una red desde <Link to="/networks" className="text-sel underline">Redes</Link>.</p>
  </main>
);
