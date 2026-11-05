// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useLayoutEffect, useRef } from 'react';

export type MenuItem = { label: string; onSelect: () => void };

const MARGIN = 4;

/** Menú contextual: flechas/Inicio/Fin mueven, Intro/Espacio activan (botones nativos), Escape y Tab cierran y devuelven el foco, un clic fuera cierra. */
export function ContextMenu({ label, x, y, items, onClose }: { label: string; x: number; y: number; items: MenuItem[]; onClose: (restoreFocus: boolean) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const entries = () => [...ref.current!.querySelectorAll<HTMLElement>('[role="menuitem"]')];

  useLayoutEffect(() => {
    const el = ref.current!;
    const r = el.getBoundingClientRect();
    el.style.left = `${Math.max(MARGIN, Math.min(x, window.innerWidth - r.width - MARGIN))}px`;
    el.style.top = `${Math.max(MARGIN, Math.min(y, window.innerHeight - r.height - MARGIN))}px`;
    entries()[0]?.focus();
  }, [x, y]);

  useEffect(() => {
    const outside = (e: PointerEvent) => { if (!ref.current?.contains(e.target as Node)) onClose(false); };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [onClose]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const list = entries();
    const at = list.indexOf(document.activeElement as HTMLElement);
    const go = (i: number) => { e.preventDefault(); list[(i + list.length) % list.length]?.focus(); };
    if (e.key === 'ArrowDown') go(at + 1);
    else if (e.key === 'ArrowUp') go(at < 0 ? -1 : at - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(-1);
    else if (e.key === 'Escape' || e.key === 'Tab') { e.preventDefault(); onClose(true); }
  };

  return (
    <div ref={ref} role="menu" aria-label={label} tabIndex={-1} onKeyDown={onKeyDown} style={{ left: x, top: y }} className="fixed z-40 min-w-48 rounded-lg border border-line bg-card p-1 shadow-lg focus:outline-none">
      {items.map((item) => (
        <button key={item.label} type="button" role="menuitem" tabIndex={-1} onClick={() => { item.onSelect(); onClose(true); }} className="block w-full cursor-default rounded px-2.5 py-1 text-left text-sm focus:bg-sel focus:text-white focus:outline-none">{item.label}</button>
      ))}
    </div>
  );
}
