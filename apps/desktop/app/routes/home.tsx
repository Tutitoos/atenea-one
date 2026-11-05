// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Navigate } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { readNetworks } from '../lib/networks';
import { readSection } from '../lib/section';

/** Con redes unidas se abre la última sección visitada (Dispositivos por defecto); sin ninguna, el asistente de primeros pasos (ADR-021). Un fallo al listar no bloquea. */
export default function Home() {
  const networks = useQuery({ queryKey: ['networks'], queryFn: readNetworks });
  if (networks.isPending) return null;
  return <Navigate to={networks.data?.length === 0 ? '/welcome' : readSection()} replace />;
}
