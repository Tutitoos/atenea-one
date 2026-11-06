// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useRef } from 'react';

export type ShortcutActions = {
  search: () => void;
  /** `n` empieza en 1, como se escribe en la tecla. */
  network: (n: number) => void;
  refresh: () => void;
  help: () => void;
};

export const detectMac = () => typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

/** Lista para la ayuda: la tecla principal depende de la plataforma. */
export const shortcutList = (isMac: boolean) => {
  const mod = isMac ? '⌘' : 'Ctrl+';
  return [
    { keys: `${mod}F`, what: 'Buscar dispositivos' },
    { keys: `${mod}1 – ${mod}9`, what: 'Cambiar a la red n.º N de la lista' },
    { keys: `${mod}R`, what: 'Actualizar ahora' },
    { keys: '?', what: 'Mostrar esta ayuda' },
  ];
};

const typing = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName));

/** Atajos globales. Con el foco en un campo de texto solo se atiende buscar; `preventDefault` solo si se atiende. */
export function useShortcuts(actions: ShortcutActions, isMac = detectMac()) {
  const ref = useRef(actions);
  ref.current = actions;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey) return;
      const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      const key = e.key.toLowerCase();
      const run = (fn: () => void) => { e.preventDefault(); fn(); };
      if (mod && !e.shiftKey) {
        if (key === 'f') return run(ref.current.search);
        if (typing(e.target)) return;
        if (key === 'r') return run(ref.current.refresh);
        if (/^[1-9]$/.test(key)) return run(() => ref.current.network(Number(key)));
        return;
      }
      if (!e.metaKey && !e.ctrlKey && e.key === '?' && !typing(e.target)) run(ref.current.help);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isMac]);
}
