// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef } from 'react';
import { detectMac, shortcutList } from './lib/shortcuts';

const FOCUSABLE = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

/** Diálogo modal con los atajos: Escape lo cierra, Tab queda dentro y el foco vuelve a donde estaba. */
export function ShortcutsHelp({ onClose, isMac = detectMac() }: { onClose: () => void; isMac?: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    box.current?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.stopPropagation(); onClose(); return; }
      if (e.key !== 'Tab') return;
      const items = [...box.current!.querySelectorAll<HTMLElement>(FOCUSABLE)];
      const first = items[0], last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => { document.removeEventListener('keydown', onKeyDown); before?.focus(); };
    // `onClose` solo cierra: no hay que reengancharlo.
  }, []);
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50">
      <div ref={box} role="dialog" aria-modal="true" aria-labelledby="atajos-titulo" className="w-96 rounded-lg bg-card p-4 shadow-xl">
        <h2 id="atajos-titulo" className="mb-3 text-base font-semibold">Atajos de teclado</h2>
        <dl className="space-y-1.5 text-sm">
          {shortcutList(isMac).map((s) => (
            <div key={s.keys} className="flex justify-between gap-4"><dt className="text-mut">{s.what}</dt><dd><kbd className="rounded bg-fg/10 px-1.5 py-0.5 font-mono text-xs">{s.keys}</kbd></dd></div>
          ))}
        </dl>
        <button type="button" onClick={onClose} className="mt-4 rounded-md bg-fg/10 px-3 py-1 text-sm focus-visible:outline-2 focus-visible:outline-sel">Cerrar</button>
      </div>
    </div>
  );
}
