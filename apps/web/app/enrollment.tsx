// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { api, result, type Model } from '@atenea/api-client';
import { Button, Card, CopyButton, Field } from './ui';
import { shellQuote, tokenSchema } from './domain';
import { CopyCode } from './join-network';

export function Enrollment({ net }: { net: string }) {
  const [token, setToken] = useState<Model<'EnrollmentToken'> | null>(null);
  const [role, setRole] = useState('Dispositivo');
  const [ttl, setTtl] = useState('3600');
  const [auto, setAuto] = useState(false);
  const [error, setError] = useState('');
  const [ttlError, setTtlError] = useState('');
  const [pending, setPending] = useState(false);
  const commands = token ? 'atenea device-init --dir ./dispositivo\n' +
    `atenea enroll --url ${shellQuote(token.join.url)} --token ${shellQuote(token.join.token)}` +
    " --dir ./dispositivo --name 'mi-dispositivo'" : '';
  return <Card title="Token de inscripción" className="incorporar-token">
    <p>El token se muestra una sola vez y solo se conserva en memoria. Compártelo por un canal seguro.
      Al ocultarlo o salir de esta pestaña no se puede recuperar.</p>
    {token ? <div>
      <h3>Guarda ahora el documento join</h3>
      <p>Caduca: {new Date(token.expires_at).toLocaleString('es')}</p>
      {token.code ? <>
        <p>Este código contiene el token y se muestra una sola vez. Guárdalo ahora y compártelo solo por un canal seguro.</p>
        <CopyCode code={token.code} label="Código con token"/>
      </> : <p>El nombre de la red o la URL pública no caben en el formato del código. Puedes usar el flujo anterior con el documento join.</p>}
      <p>Huella de la raíz: <code>{token.root_anchor_fingerprint}</code>
        {token.root_anchor_fingerprint && <CopyButton value={token.root_anchor_fingerprint} label="Copiar huella de la raíz"/>}</p>
      <p>Compara la huella por otro canal de confianza.</p>
      <pre>{JSON.stringify(token.join, null, 2)}</pre>
      <CopyButton value={JSON.stringify(token.join, null, 2)} label="Copiar join"/>
      <p>Ejecuta en el dispositivo (el token también aparecerá en el historial de tu shell; usa un entorno protegido):</p>
      <pre>{commands}</pre><CopyButton value={commands} label="Copiar comandos de instalación"/>
      <p>Conserva network_id y root_anchor de join como ancla de confianza para configurar el agente. Verifica la huella antes de aprobar.</p>
      <Button onClick={() => setToken(null)}>Ya lo guardé: ocultar definitivamente</Button>
    </div> : <form className="incorporar-form" noValidate onSubmit={async event => {
      event.preventDefault(); setError(''); setTtlError('');
      const parsed = tokenSchema.safeParse({ ttl_secs: Number(ttl), auto_approve: auto });
      if (!parsed.success) { setTtlError('Usa un número entero entre 60 y 86400 segundos.'); return; }
      setPending(true);
      try {
        setToken(await result(api.POST('/api/v1/networks/{net}/enrollment-tokens', {
          params: { path: { net } }, body: { ...parsed.data, role: role === 'Servidor' ? 'server' : 'device' },
        })));
      } catch (cause) { setError(cause instanceof Error ? cause.message : 'No se pudo crear el token.'); }
      finally { setPending(false); }
    }}>
      <Field label="Tipo" help="Dispositivo: equipo de usuario. Servidor: equipo que puede anunciar subredes y ofrecer servicios a la red.">{props =>
        <select {...props} value={role} onChange={event => setRole(event.target.value)}>
          <option>Dispositivo</option><option>Servidor</option>
        </select>}
      </Field>
      <Field label="Validez (segundos, 60–86400)" help="Una hora son 3600 segundos. El token deja de permitir inscripciones al caducar."
        error={ttlError}>{props => <input {...props} type="number" min="60" max="86400" step="1"
          value={ttl} onChange={event => setTtl(event.target.value)}/>}</Field>
      <Field label="Aprobar automáticamente (omite la revisión de huella)"
        help="Advertencia: quien tenga el token entrará sin aprobación manual. Actívalo solo si puedes verificar a quién se lo entregas.">{props =>
        <input {...props} type="checkbox" checked={auto} onChange={event => setAuto(event.target.checked)}/>}
      </Field>
      <Button type="submit" variant="primary" loading={pending}>Crear token</Button>
      {error && <p role="alert">{error}</p>}
    </form>}
  </Card>;
}
