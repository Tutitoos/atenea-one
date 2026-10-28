// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useSyncExternalStore } from 'react';

// Qué dispositivos hacen de servidor lo decide quien administra la red, y ni el estado del agente ni la configuración
// firmada lo dicen: es una marca local de este equipo, como los alias (ADR-011). Se guarda por clave pública.
const KEY = 'atenea-servers';
const listeners = new Set<() => void>();

function load(): readonly string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(value) ? value.filter((k): k is string => typeof k === 'string') : [];
  } catch {
    return [];
  }
}

let servers = load();

export function setServer(key: string, on: boolean) {
  const next = servers.filter((k) => k !== key);
  if (on) next.push(key);
  servers = next;
  localStorage.setItem(KEY, JSON.stringify(servers));
  listeners.forEach((l) => l());
}

export const useServers = () => useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => servers);
