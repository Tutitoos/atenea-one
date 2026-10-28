// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useId, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, Card, CopyButton, Dialog, Field, RunButton } from './ui';

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
  let error = query.error?.message;
  if (query.error instanceof ApiError && query.error.status === 409) {
    if (query.error.detail.error.code === 'invalid_network_name_for_code') {
      error = 'El nombre de la red no es compatible con el código. Renombra la red en DNS y certificado con un nombre más corto.';
    }
    if (query.error.detail.error.code === 'invalid_public_url_for_code') {
      error = 'Corrige la URL pública del servidor: debe usar HTTPS, tener como máximo 100 bytes y no incluir barra final, credenciales, consulta ni fragmento.';
    }
  }
  return <div className="incorporar-cards">
    <Card title="Código de la red">
      <p>Comparte este código sin token para identificar la red. El código no autentica.</p>
      {canEdit ? <>
        {query.isPending ? <p role="status">Cargando código de red…</p> : query.isError ?
          <p role="alert">{error}</p> : query.data && <>
            <CopyCode code={query.data.code} label="Código de red"/>
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
