// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { z } from 'zod';

// Frontera con el agente: se valida lo que se usa y se toleran campos ausentes (agentes anteriores
// no informan `addrs`) o añadidos. Cualquier otra forma se rechaza con un error visible.
const path = z.object({ kind: z.string(), ready: z.boolean(), healthy: z.boolean(), rtt_ms: z.number().nullish() });
const peer = z.object({
  public_key: z.string(),
  addrs: z.array(z.string()).default([]),
  // Nombre de visualización que reparte el control plane (ADR-012); los agentes anteriores no lo informan.
  name: z.string().nullish(),
  endpoint: z.string().nullish(),
  session: z.string().nullish(),
  last_handshake_secs_ago: z.number().nullish(),
  tx_bytes: z.number(),
  rx_bytes: z.number(),
  path: z.string(),
  candidates: z.number().default(0),
  paths: z.array(path).default([]),
});
const status = z.object({
  public_key: z.string(),
  addrs: z.array(z.string()).default([]),
  hostname: z.string().nullish(),
  config: z.object({ version: z.number(), valid_until_unix: z.number().nullish(), lease_active: z.boolean(), last_error: z.string().nullish() }),
  nat: z.object({ mapping: z.string(), reflexive: z.string().nullish() }).nullish(),
  sync: z.object({ url: z.string().nullish(), last_ok_unix: z.number().nullish(), last_error: z.string().nullish(), sync_ok: z.number().nullish(), sync_errors: z.number().nullish(), sync_denied: z.number().nullish() }).nullish(),
  coordinator: z.object({ url: z.string().nullish(), last_announce_ok_unix: z.number().nullish(), last_error: z.string().nullish(), announces: z.number().nullish(), events: z.number().nullish() }).nullish(),
  relay: z.object({ allocations: z.number().default(0), drops: z.record(z.string(), z.number()).default({}), entries: z.array(z.record(z.string(), z.unknown())).default([]) }).nullish(),
  counters: z.record(z.string(), z.number()).default({}),
  peers: z.array(peer).default([]),
});

// `health` lo calcula Rust (src/main.rs::assess): la bandeja y la ventana comparten criterio.
export const reply = z.object({
  status,
  health: z.object({ level: z.enum(['ok', 'degraded', 'down']), text: z.string(), reasons: z.array(z.string()) }),
});

export type Reply = z.infer<typeof reply>;
export type Status = Reply['status'];
export type Peer = Status['peers'][number];
export type Level = Reply['health']['level'];
