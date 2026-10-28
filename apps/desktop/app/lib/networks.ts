// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import type { Tone } from './format';

export const netState = z.enum(['detenida', 'iniciada_sin_configuracion', 'conectada', 'conflicto', 'desconocida']);
export type NetState = z.infer<typeof netState>;

/** Lo que imprime `atenea network list --json` (solo los campos que usa la pantalla). */
export const network = z.object({
  network_id: z.string(),
  name: z.string(),
  server: z.string(),
  interface: z.string(),
  listen_port: z.number(),
  join_status: z.string(),
  state: netState,
  detail: z.string().optional(),
});
export type Network = z.infer<typeof network>;

/** Lo que imprime `atenea network inspect --json`: nunca incluye el token del código. */
export const codeInfo = z.object({
  name: z.string(),
  server: z.string(),
  network_id: z.string(),
  root_fingerprint: z.string(),
  has_token: z.boolean(),
});
export type CodeInfo = z.infer<typeof codeInfo>;

export const stateText: Record<NetState, string> = {
  detenida: 'Desconectada',
  iniciada_sin_configuracion: 'Sin configuración (pendiente de aprobación)',
  conectada: 'Conectada',
  conflicto: 'Conflicto',
  desconocida: 'Sin acceso al estado',
};
export const stateTone: Record<NetState, Tone> = { detenida: 'off', iniciada_sin_configuracion: 'warn', conectada: 'ok', conflicto: 'bad', desconocida: 'off' };

/** Tauri rechaza con el texto devuelto por la orden, no con un Error. */
async function call(cmd: string, args?: Record<string, unknown>): Promise<string> {
  try {
    return await invoke<string>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === 'string' ? e : 'La operación falló.', { cause: e });
  }
}

export async function readNetworks(): Promise<Network[]> {
  const parsed = z.array(network).safeParse(JSON.parse(await call('network_list')));
  if (!parsed.success) throw new Error('El CLI respondió con un formato inesperado.');
  return parsed.data;
}

export async function inspectCode(code: string): Promise<CodeInfo> {
  const parsed = codeInfo.safeParse(JSON.parse(await call('network_inspect', { code })));
  if (!parsed.success) throw new Error('El CLI respondió con un formato inesperado.');
  return parsed.data;
}

/** Cada operación pide la autorización de administrador del sistema; la app no guarda credenciales. */
export const joinNetwork = (code: string) => call('network_apply', { op: 'join', code });
export const changeNetwork = (op: 'up' | 'down' | 'leave', network: string) => call('network_apply', { op, network });

/** Datos que `join` imprime y que el administrador compara al aprobar el equipo. */
export const deviceFingerprint = (output: string) => /^huella_dispositivo=(\S+)/m.exec(output)?.[1];
