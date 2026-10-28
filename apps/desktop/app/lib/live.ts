// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useOutletContext } from 'react-router';
import type { Sample } from './agent';

/** `empty`: ya se sabe que no hay ninguna red unida (no es lo mismo que «aún cargando»). */
export type Agent = { sample: Sample | undefined; error: Error | null; empty: boolean };

/** La lectura se hace una sola vez en el shell y baja por el contexto de las rutas. */
export const useAgent = () => useOutletContext<Agent>();
