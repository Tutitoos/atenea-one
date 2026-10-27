// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useSyncExternalStore } from 'react';

// La configuración firmada no lleva nombres: son una etiqueta local de este equipo (ADR-011).
const KEY = 'atenea-names';
const listeners = new Set<() => void>();

function load(): Record<string, string> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return value && typeof value === 'object' ? (value as Record<string, string>) : {};
  } catch {
    return {};
  }
}

let names = load();

export function setName(key: string, name: string) {
  const next = { ...names };
  if (name.trim()) next[key] = name.trim(); else delete next[key];
  names = next;
  localStorage.setItem(KEY, JSON.stringify(names));
  listeners.forEach((l) => l());
}

export const useNames = () => useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => names);
