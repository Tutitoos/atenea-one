// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useQuery } from '@tanstack/react-query';
import { api, result } from '@atenea/api-client';
import { Card, StatusBadge } from './ui';

export function certificateExpiry(expires: string | null | undefined, now = Date.now()) {
  if (!expires) return null;
  const days = Math.ceil((Date.parse(expires) - now) / 86400000);
  return { days, tone: days > 14 ? 'ok' as const : days > 3 ? 'warn' as const : 'bad' as const };
}
export function CertificateStatus({ installed, expires }: { installed: boolean; expires?: string | null }) {
  const expiry = installed ? certificateExpiry(expires) : null;
  return <><StatusBadge tone={installed ? expiry?.tone ?? 'ok' : 'warn'}>
    {installed ? 'Certificado instalado' : 'Falta certificado de emisor'}
  </StatusBadge>{expiry && <p><time dateTime={expires!}>{expiry.days <= 0 ? 'Caducó' : 'Caduca'}: {new Date(expires!).toLocaleString('es')}</time>
    {' · '}{expiry.days < 0 ? `hace ${-expiry.days} días` : `${expiry.days} días restantes`}</p>}</>;
}
// La hora se compara al recibir la respuesta, no al renderizar ni al avanzar el reloj.
export function useServerInfo() {
  return useQuery({ queryKey: ['server'], retry: false, staleTime: 300_000, refetchInterval: 300_000,
    refetchOnWindowFocus: false, queryFn: async () => {
      try {
        const data = await result(api.GET('/api/v1/server'));
        return { ...data, offset: Math.round((Date.parse(data.server_time) - Date.now()) / 1000) };
      } catch { return null; }
    } });
}
export function ServerInfo() {
  const server = useServerInfo();
  return <Card title="Servidor">{server.data?.control_plane ? <>
    <p>Control plane: {server.data.control_plane.version} · <code>{server.data.control_plane.commit?.slice(0, 8) || 'Commit no disponible'}</code></p>
    <p>Hora del servidor: <time dateTime={server.data.server_time}>{new Date(server.data.server_time).toLocaleString('es')}</time></p>
    <p className={Math.abs(server.data.offset) > 30 ? 'warning' : 'muted'}>Desfase respecto al reloj local: {server.data.offset > 0 ? '+' : ''}{server.data.offset} s.
      {Math.abs(server.data.offset) > 30 && ' Revisa los relojes: los leases dependen del reloj.'}</p>
    <p className="muted">Las versiones de coordinator y relay no están disponibles desde el panel.</p>
  </> : <p>{server.isPending ? 'Consultando versión…' : 'Este servidor aún no informa de su versión'}</p>}</Card>;
}
