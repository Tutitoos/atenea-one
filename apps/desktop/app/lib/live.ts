// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useOutletContext } from 'react-router';
import type { Sample } from './agent';

/**
 * `empty`: ya se sabe que no hay ninguna red unida (no es lo mismo que «aún cargando»).
 * `stale`: `sample` es de la red anterior mientras llega la lectura de la actual; no debe presentarse como suya.
 */
export type Agent = { sample: Sample | undefined; stale: boolean; network: string; error: Error | null; empty: boolean; selectNetwork: (id: string) => void };

/** La lectura se hace una sola vez en el shell y baja por el contexto de las rutas. */
export const useAgent = () => useOutletContext<Agent>();
