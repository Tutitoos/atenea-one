// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useId, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, Card, CopyButton, Dialog, Field, RunButton } from './ui';
import { shellQuote } from './domain';

export function CopyCode({ code, label }: { code: string; label: string }) {
  const [message, setMessage] = useState('');
  return <div>
    <Field label={label}>{props => <textarea {...props} className="join-code" readOnly value={code} rows={4}/>}</Field>
    <Button onClick={async () => {
      try { await navigator.clipboard.writeText(code); setMessage('Código copiado.'); }
      catch { setMessage('No se pudo copiar. Selecciona y copia el código del campo.'); }
    }}>Copiar {label.toLocaleLowerCase('es')}</Button>
    <p role="status">{message}</p>
  </div>;
}

/** ADR-018: lo que anuncia el código (el panel lo recibe del servidor; el CLI lo verifica con el hash). */
type Connectivity = { stun_host: string; relay_ca_sha256?: string | null } | null | undefined;

/** Unirse con solo el código: un comando y la aprobación. Sin conectividad en el código hacen falta las opciones. */
export function JoinCommand({ code, connectivity, legacy = false, stun, url }: { code: string; connectivity: Connectivity; legacy?: boolean; stun?: string; url?: string }) {
  if (legacy) {
    // Un CLI 0.1.8 o anterior ni entiende el bloque del código ni tiene --up: las opciones y el arranque van aparte.
    const old = `sudo atenea network join ${shellQuote(code)} --stun-host ${stun ?? 'HOST'}`;
    return <div>
      <p>En un equipo con CLI 0.1.8 o anterior (no admite el código con conectividad ni <code>--up</code>):</p>
      <pre>{old}</pre>
      {!stun && <p className="warning">Sustituye HOST por el STUN del servidor (el que anuncia el código de red de esta misma pestaña): no se ha podido obtener aquí.</p>}
      <CopyButton value={old} label="Copiar comando de unión (CLI anterior)"/>
      <p>Si el relay usa una CA propia, añade <code>--relay-ca RUTA</code> con la CA que publica el servidor{url ? ` en ${url}/api/v1/relay-ca` : ''}; compara su SHA-256 con el del código de red.</p>
      <p>Después, <code>sudo atenea network up</code> con el nombre o el id de la red, y un administrador aprueba su huella.</p>
    </div>;
  }
  const command = `sudo atenea network join ${shellQuote(code)} --up`;
  return <div>
    <p>En el equipo que se une (CLI 0.1.9 o posterior), después de instalar el paquete:</p>
    <pre>{command}</pre>
    <CopyButton value={command} label="Copiar comando de unión"/>
    {connectivity ? <p>{`El código anuncia el STUN ${connectivity.stun_host} y ${connectivity.relay_ca_sha256 ?
      `la CA del relay con SHA-256 ${connectivity.relay_ca_sha256.slice(0, 16)}…` :
      'un relay con CA pública (sin hash)'}: el equipo no necesita más opciones. Después, un administrador aprueba su huella.`}</p> :
      <p className="warning">Este servidor no anuncia conectividad en el código: añade <code>--stun-host HOST</code> (y <code>--relay-ca RUTA</code> si el relay usa una CA propia) o el equipo no tendrá camino directo.</p>}
  </div>;
}
const labels = { cerrada: 'Cerrada (solo con token)', con_solicitud: 'Con solicitud (un administrador aprueba)' };
export function JoinNetwork({ net, canEdit, policy }: { net: string; canEdit: boolean; policy: Model<'JoinPolicy'> }) {
  const cache = useQueryClient();
  const key = ['network', net, 'join-code'];
  const query = useQuery({ queryKey: key, enabled: canEdit, retry: false,
    queryFn: () => result(api.GET('/api/v1/networks/{net}/join-code', { params: { path: { net } } })) });
  const [selected, setSelected] = useState(policy);
  const current = query.data?.join_policy ?? policy;
  const id = useId();
  const policyInput = useRef<HTMLInputElement>(null);
  const [message, setMessage] = useState('');
  async function save() {
    const value = await result(api.PUT('/api/v1/networks/{net}/join-policy', {
      params: { path: { net } }, body: { policy: selected },
    }));
    cache.setQueryData<Model<'JoinCode'>>(key, old => old ? { ...old, join_policy: value.policy } : old);
    await cache.invalidateQueries({ queryKey: ['network', net] });
    setMessage(`Política actualizada: ${labels[value.policy]}.`);
    policyInput.current?.focus();
  }
  const explain = (e: Error | null) => {
    let message = e?.message;
    if (e instanceof ApiError && e.status === 409) {
      if (e.detail.error.code === 'invalid_network_name_for_code') {
        message = 'El nombre de la red no es compatible con el código. Renombra la red en DNS y certificado con un nombre más corto.';
      }
      if (e.detail.error.code === 'invalid_public_url_for_code') {
        message = 'Corrige la URL pública del servidor: debe usar HTTPS, tener como máximo 100 bytes y no incluir barra final, credenciales, consulta ni fragmento.';
      }
    }
    return message;
  };
  const error = explain(query.error);
  // Código sin bloque para equipos con un CLI anterior a la 0.1.9 (que rechazan el bloque de conectividad).
  const [legacy, setLegacy] = useState(false);
  const legacyQuery = useQuery({ queryKey: [...key, 'sin-conectividad'], enabled: canEdit && legacy, retry: false,
    queryFn: () => result(api.GET('/api/v1/networks/{net}/join-code', { params: { path: { net }, query: { conectividad: '0' } } })) });
  return <div className="incorporar-cards">
    <Card title="Código de la red">
      <p>Comparte este código sin token para identificar la red. El código no autentica.</p>
      {canEdit ? <>
        {query.isPending ? <p role="status">Cargando código de red…</p> : query.isError ?
          <p role="alert">{error}</p> : query.data && <>
            <CopyCode code={query.data.code} label="Código de red"/>
            <JoinCommand code={query.data.code} connectivity={query.data.connectivity}/>
            {query.data.connectivity && <div>
              <label><input type="checkbox" checked={legacy} onChange={event => setLegacy(event.target.checked)}/>
                Código para un equipo con un CLI anterior a la 0.1.9 (sin conectividad)</label>
              {legacy && (legacyQuery.isPending ? <p role="status">Cargando código sin conectividad…</p> : legacyQuery.isError ?
                <p role="alert">{explain(legacyQuery.error)}</p> : legacyQuery.data && <>
                  <CopyCode code={legacyQuery.data.code} label="Código sin conectividad"/>
                  <JoinCommand code={legacyQuery.data.code} connectivity={null} legacy stun={query.data.connectivity.stun_host} url={query.data.url}/>
                </>)}
            </div>}
            <p>Huella de la raíz: <code>{query.data.root_anchor_fingerprint}</code>
              <CopyButton value={query.data.root_anchor_fingerprint} label="Copiar huella de la raíz"/></p>
            <p className="warning">Compara esta huella por otro canal de confianza antes de usar el código.</p>
          </>}
      </> : <p>Solo administradores y propietarios pueden ver el código y cambiar la política de unión.</p>}
    </Card>
    <Card title="Política de unión">
      <p>Política actual: <strong>{labels[current]}</strong></p>
      {canEdit && <div className="incorporar-form">
        <fieldset><legend>Política de unión</legend>
          {(Object.keys(labels) as Model<'JoinPolicy'>[]).map(value => <div className="incorporar-option" key={value}>
            <label htmlFor={`${id}-${value}`}><input id={`${id}-${value}`} type="radio" name={`${id}-policy`}
              ref={selected === value ? policyInput : undefined} checked={selected === value}
              aria-describedby={`${id}-${value}-help`} onChange={() => { setSelected(value); setMessage(''); }}/>
              {labels[value]}</label>
            <p className="muted" id={`${id}-${value}-help`}>{value === 'cerrada' ?
              'Solo se puede incorporar un equipo con un token de inscripción válido.' :
              'Cualquiera con el código puede solicitar acceso. Un administrador debe comprobar su huella y aprobarlo.'}</p>
          </div>)}
        </fieldset>
        {selected !== current && <Dialog
          trigger={selected === 'con_solicitud' ? 'Permitir solicitudes' : 'Cerrar unión: solo con token'}
          title={selected === 'con_solicitud' ? 'Permitir solicitudes de unión' : 'Cerrar la unión'}
          description={selected === 'con_solicitud' ?
            'Cualquiera con el código puede enviar una solicitud; hay un tope de pendientes y un administrador aprueba comparando la huella del dispositivo.' :
            'Se dejarán de admitir nuevas solicitudes con el código. Los equipos nuevos necesitarán un token.'}>
          <RunButton variant="primary" run={save}>{selected === 'con_solicitud' ?
            'Confirmar política con solicitud' : 'Confirmar política cerrada'}</RunButton>
        </Dialog>}
        <p role="status">{message}</p>
      </div>}
    </Card>
  </div>;
}
