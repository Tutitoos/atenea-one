// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { readAutostart, setAutostart } from './lib/networks';
import { Heading } from './ui';

/** Interruptor de «Abrir al iniciar sesión» (un fichero de arranque del propio usuario, sin privilegios). Oculto si el sistema no lo admite. */
export function Autostart() {
  const client = useQueryClient();
  const preference = useQuery({ queryKey: ['autostart'], queryFn: readAutostart });
  const change = useMutation({
    mutationFn: setAutostart,
    onSuccess: (_, on) => client.setQueryData(['autostart'], on),
  });
  if (preference.data === null) return null;
  return <section aria-label="Inicio">
    <Heading>Inicio</Heading>
    <label className="flex items-center gap-2"><input type="checkbox" role="switch" checked={preference.data ?? false} disabled={preference.isPending || !!preference.error || change.isPending} onChange={(event) => change.mutate(event.target.checked)} />Abrir al iniciar sesión</label>
    <p className="mt-1 text-sm text-mut">Abre Atenea One en la bandeja al iniciar sesión, sin mostrar la ventana. No cambia la conexión de las redes: las gestiona el sistema.</p>
    {(change.error ?? preference.error) && <p role="alert" className="text-bad">{(change.error ?? preference.error)?.message}</p>}
  </section>;
}
