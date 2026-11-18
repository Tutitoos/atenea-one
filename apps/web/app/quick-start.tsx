// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, Card, ConfirmDialog } from './ui';
import { CopyCode, JoinCommand } from './join-network';

// El token de «Añadir un equipo» vale una hora y aprueba solo: lo crea quien ya es de confianza para la red y se lo
// pega a un equipo suyo. El equipo genera sus claves en local; el servidor no ve ninguna clave privada.
const TTL_SECS = 3600;

/** Primeros pasos de una red (ADR-029 D5): un comando para unir un equipo y, si aún no hay políticas, un clic para que se vean. */
export function QuickStart({ net, canEdit, enroll, policies }: {
  net: string; canEdit: boolean; enroll: boolean; policies?: number;
}) {
  const cache = useQueryClient();
  const [token, setToken] = useState<Model<'EnrollmentToken'>>();
  const [message, setMessage] = useState('');
  const [pending, setPending] = useState(false);
  if (!canEdit && !enroll) return null;
  const create = async () => {
    setMessage(''); setPending(true);
    try {
      setToken(await result(api.POST('/api/v1/networks/{net}/enrollment-tokens', {
        params: { path: { net } }, body: { ttl_secs: TTL_SECS, auto_approve: true, role: 'device' },
      })));
      await cache.invalidateQueries({ queryKey: ['network', net, 'audit'] });
    } catch (error) { setMessage(error instanceof Error ? error.message : 'No se pudo crear el comando.'); }
    finally { setPending(false); }
  };
  return <Card title="Añadir un equipo" className="overview-audit">
    <p>Instala el CLI de Atenea One en el equipo y ejecuta allí el comando. El equipo crea sus claves y queda unido a esta red.
      El comando contiene un token de un solo uso que caduca en una hora: no lo compartas por canales abiertos.</p>
    {token ? <div>
      {token.code ? <>
        <JoinCommand code={token.code} connectivity={token.connectivity} url={token.join.url}/>
        <CopyCode code={token.code} label="Código con token"/>
      </> : <p role="alert">El nombre de la red o la URL pública no caben en el formato del código: usa «Incorporar» para el flujo con documento join.</p>}
      <p className="muted">No se vuelve a mostrar. Caduca: {new Date(token.expires_at).toLocaleString('es')}.</p>
      <Button onClick={() => setToken(undefined)}>Ya lo copié: ocultarlo</Button>
    </div> : <Button variant="primary" loading={pending} onClick={() => void create()}>Generar comando</Button>}
    <p role="status">{message}</p>
    {canEdit && policies === 0 && <>
      <h3>Que los equipos se vean entre sí</h3>
      <p>Unir un equipo no le da acceso a los demás: sin políticas no hay tráfico entre equipos. Para una red personal puedes
        permitir todo con una política; después puedes restringirla en «Políticas».</p>
      <ConfirmDialog trigger="Permitir todo entre equipos" title="Permitir todo entre los equipos de esta red"
        description="Crea una política de cualquiera a cualquiera, en todos los protocolos y puertos. Cualquier equipo unido podrá hablar con los demás."
        label="Crear la política" confirm={async () => {
          try {
            await api.POST('/api/v1/networks/{net}/policies', { params: { path: { net } },
              body: { src: { kind: 'any' }, dst: { kind: 'any' }, proto: 0, port_min: 0, port_max: 0 } });
          } catch (error) {
            throw new Error(error instanceof ApiError ? 'El servidor no aceptó la política. Créala desde «Políticas».' : 'No se pudo crear la política.', { cause: error });
          }
          await cache.invalidateQueries({ queryKey: ['network', net, 'policies'] });
        }}/>
    </>}
  </Card>;
}
