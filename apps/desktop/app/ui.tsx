// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState, type ReactNode } from 'react';
import type { Tone } from './lib/format';

const bg: Record<Tone, string> = { ok: 'bg-ok', warn: 'bg-warn', bad: 'bg-bad', off: 'bg-off' };

export const Dot = ({ tone }: { tone: Tone }) => <span aria-hidden="true" className={`mt-1.5 inline-block size-2.5 shrink-0 rounded-full ${bg[tone]}`} />;

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

export function CopyButton({ value }: { value: string }) {
  const [done, setDone] = useState(false);
  return (
    <button type="button" className="ml-2 cursor-default text-xs text-sel" onClick={() => { void navigator.clipboard.writeText(value).then(() => { setDone(true); setTimeout(() => setDone(false), 1500); }); }}>
      {done ? 'Copiado' : 'Copiar'}
    </button>
  );
}

export const Empty = ({ children }: { children: ReactNode }) => <p className="p-4 text-mut">{children}</p>;
