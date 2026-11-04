// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Call } from '@wailsio/runtime';

/** Orden de la interfaz → método del servicio Go (`AteneaService`) y orden de sus argumentos. Es la única superficie que la página invoca. */
const methods = {
  client_status: ['ClientStatus', []],
  status: ['Status', ['network']],
  network_list: ['NetworkList', []],
  network_legacy: ['NetworkLegacy', []],
  network_inspect: ['NetworkInspect', ['code']],
  network_apply: ['NetworkApply', ['op', 'network', 'code', 'name']],
  device_ping: ['DevicePing', ['network', 'address']],
  open_panel: ['OpenPanel', ['network', 'deviceId']],
  notifications_get: ['NotificationsGet', []],
  notifications_set: ['NotificationsSet', ['on']],
  autostart_get: ['AutostartGet', []],
  autostart_set: ['AutostartSet', ['on']],
} as const satisfies Record<string, readonly [string, readonly string[]]>;

/** Llama a una operación de Go. Rechaza con el texto del error (no con un objeto), como la versión de Tauri. */
export async function invoke<T = string>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const entry = (methods as Record<string, readonly [string, readonly string[]]>)[cmd];
  if (!entry) throw 'Operación desconocida.';
  const [method, names] = entry;
  try {
    // Un argumento omitido viaja como cadena vacía (Go no distingue ausente de vacío en estos campos).
    return (await Call.ByName(`main.AteneaService.${method}`, ...names.map((n) => args[n] ?? (n === 'on' ? false : '')))) as T;
  } catch (e) {
    throw e instanceof Error ? e.message : e;
  }
}
