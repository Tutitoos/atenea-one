// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from 'react';

const MAX = 3;
const LIFE_MS = 2000;

type Push = (text: string) => void;
const Ctx = createContext<Push | null>(null);

/** Avisos breves y apilables (máximo 3) en una región `status` siempre presente, para que los lectores de pantalla los anuncien. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<{ id: number; text: string }[]>([]);
  const next = useRef(0);
  const push = useCallback<Push>((text) => {
    const id = next.current++;
    setItems((list) => [...list, { id, text }].slice(-MAX));
    setTimeout(() => setItems((list) => list.filter((t) => t.id !== id)), LIFE_MS);
  }, []);
  return (
    <Ctx.Provider value={push}>
      {children}
      <div role="status" aria-live="polite" aria-label="Notificaciones" className="pointer-events-none fixed bottom-3 right-3 z-50 flex flex-col items-end gap-1.5">
        {items.map((t) => <p key={t.id} className="toast rounded-lg bg-fg px-3 py-1.5 text-sm text-bg shadow-lg">{t.text}</p>)}
      </div>
    </Ctx.Provider>
  );
}

/** Sin `ToastProvider` (pantallas sueltas) no hay dónde avisar: devuelve `null`. */
export const useToast = () => useContext(Ctx);

/** Copia al portapapeles y avisa con «Copiado: <qué>» o «No se pudo copiar». Devuelve si se copió. */
export function useCopy() {
  const toast = useToast();
  return useCallback((value: string, what: string) =>
    // `navigator.clipboard` puede faltar (contexto no seguro) o rechazar (permiso denegado).
    Promise.resolve().then(() => navigator.clipboard.writeText(value)).then(
      () => { toast?.(`Copiado: ${what}`); return true; },
      () => { toast?.('No se pudo copiar'); return false; },
    ), [toast]);
}
