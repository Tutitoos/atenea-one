// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useId, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { Button, Dialog, RunButton } from './ui';

export function CopyCode({ code, label }: { code: string; label: string }) {
  const id = useId();
  const [message, setMessage] = useState('');
  return <div><label htmlFor={id}>{label}</label><textarea className="join-code" id={id} readOnly value={code} rows={4}/><Button onClick={async () => {
    try { await navigator.clipboard.writeText(code); setMessage('Código copiado.'); }
    catch { setMessage('No se pudo copiar. Selecciona y copia el código del campo.'); }
  }}>Copiar {label.toLocaleLowerCase('es')}</Button><p role="status">{message}</p></div>;
}
const labels = { cerrada: 'Cerrada (solo con token)', con_solicitud: 'Con solicitud (un administrador aprueba)' };
export function JoinNetwork({ net, canEdit, policy }: { net: string; canEdit: boolean; policy: Model<'JoinPolicy'> }) {
  const cache = useQueryClient();
  const key = ['network', net, 'join-code'];
  const query = useQuery({ queryKey: key, enabled: canEdit, retry: false, queryFn: () => result(api.GET('/api/v1/networks/{net}/join-code', { params: { path: { net } } })) });
  const [selected, setSelected] = useState(policy);
  const current = query.data?.join_policy ?? policy;
  const id = useId();
  const policySelect = useRef<HTMLSelectElement>(null);
  const [message, setMessage] = useState('');
  async function save() {
    const value = await result(api.PUT('/api/v1/networks/{net}/join-policy', { params: { path: { net } }, body: { policy: selected } }));
    cache.setQueryData<Model<'JoinCode'>>(key, old => old ? { ...old, join_policy: value.policy } : old);
    await cache.invalidateQueries({ queryKey: ['network', net] });
    setMessage(`Política actualizada: ${labels[value.policy]}.`);
    policySelect.current?.focus();
  }
  let error = query.error?.message;
  if (query.error instanceof ApiError && query.error.status === 409) {
    if (query.error.detail.error.code === 'invalid_network_name_for_code') error = 'Corrige el nombre de la red: debe tener de 1 a 48 bytes UTF-8, sin espacios en los extremos, controles ni marcas bidireccionales.';
    if (query.error.detail.error.code === 'invalid_public_url_for_code') error = 'Corrige la URL pública del servidor: debe usar HTTPS, tener como máximo 100 bytes y no incluir barra final, credenciales, consulta ni fragmento.';
  }
  return <section aria-labelledby={`${id}-title`}><h2 id={`${id}-title`}>Unirse a esta red</h2>
    <p>Política actual: <strong>{labels[current]}</strong></p>
    {canEdit ? <>
      {query.isPending ? <p role="status">Cargando código de red…</p> : query.isError ? <p role="alert">{error}</p> : query.data && <>
        <CopyCode code={query.data.code} label="Código de red"/>
        <p>Huella de la raíz: <code>{query.data.root_anchor_fingerprint}</code></p>
        <p>Compara esta huella por otro canal de confianza antes de usar el código.</p>
      </>}
      <label htmlFor={`${id}-policy`}>Política de unión</label><select ref={policySelect} id={`${id}-policy`} value={selected} onChange={e => { setSelected(e.target.value as Model<'JoinPolicy'>); setMessage(''); }}>
        <option value="cerrada">{labels.cerrada}</option><option value="con_solicitud">{labels.con_solicitud}</option>
      </select>
      {selected !== current && (selected === 'con_solicitud' ? <Dialog trigger="Permitir solicitudes" title="Permitir solicitudes de unión" description="Cualquiera con el código puede enviar una solicitud; hay un tope de pendientes y un administrador aprueba comparando la huella del dispositivo."><RunButton run={save}>Confirmar política con solicitud</RunButton></Dialog> : <RunButton run={save}>Cerrar unión: solo con token</RunButton>)}
      <p role="status">{message}</p>
    </> : <p>Solo administradores y propietarios pueden ver el código y cambiar la política de unión.</p>}
  </section>;
}
