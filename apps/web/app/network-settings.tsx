// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result } from '@atenea/api-client';
import type { SectionProps } from './sections';
import { Dns } from './dns';
import { Button, Card, ConfirmDialog, CopyButton, Dialog, Field, RunButton, Status } from './ui';
import { CertificateStatus } from './server-status';
import { RenameDialog } from './rename';

export function NetworkSettings({ net, network, canEdit, refresh }: SectionProps) {
  const cache = useQueryClient();
  const [certificate, setCertificate] = useState<File | null>(null);
  const [certificateError, setCertificateError] = useState('');
  const [message, setMessage] = useState('');
  const [pending, setPending] = useState(false);
  const [autoConnectDraft, setAutoConnectDraft] = useState<boolean>();
  const issuer = useQuery({ queryKey: ['issuer'], enabled: canEdit, retry: false,
    queryFn: () => result(api.GET('/api/v1/issuer')) });
  const join = useQuery({ queryKey: ['network', net, 'join-code'], enabled: canEdit, retry: false,
    queryFn: () => result(api.GET('/api/v1/networks/{net}/join-code', { params: { path: { net } } })) });
  const data = network.data;
  return <div className="incorporar-cards">
    <Card title="Datos de la red">
      <Status query={network}/>
      {data && <dl className="incorporar-network-data">
        <dt>Nombre</dt><dd>{data.name}</dd>
        <dt>CIDR IPv4</dt><dd><code>{data.ipv4_cidr}</code></dd>
        {data.ipv4_pool && <><dt>Rango de reparto IPv4</dt><dd><code>{data.ipv4_pool.start} – {data.ipv4_pool.end}</code></dd></>}
        <dt>CIDR IPv6</dt><dd>{data.ipv6_cidr ? <code>{data.ipv6_cidr}</code> : 'Sin IPv6'}</dd>
        <dt>Conexiones automáticas</dt><dd>{data.auto_connect ? 'Activadas' : 'Desactivadas'}</dd>
        <dt>Ancla de la raíz</dt><dd><code>{data.root_anchor}</code>
          <CopyButton value={data.root_anchor} label="Copiar ancla de la raíz"/></dd>
        {join.data && <><dt>Huella de la raíz</dt><dd><code>{join.data.root_anchor_fingerprint}</code>
          <CopyButton value={join.data.root_anchor_fingerprint} label="Copiar huella de la raíz"/></dd></>}
      </dl>}
      {data && canEdit && <RenameDialog kind="red" name={data.name} rename={async name => {
        await api.PATCH('/api/v1/networks/{net}', { params: { path: { net } }, body: { name } });
        await Promise.all([refresh(), cache.invalidateQueries({ queryKey: ['org', data.org_id, 'networks'] })]);
      }}/>}
      <p className="muted">Los códigos de unión ya emitidos conservan el nombre antiguo (solo informativo) y siguen siendo válidos.</p>
      {!canEdit && <p>Pide la huella de la raíz a un administrador y compárala por otro canal de confianza.</p>}
      {canEdit && join.isError && <p role="alert">No se pudo consultar la huella: {join.error.message}</p>}
    </Card>
    {data && canEdit && <Card title="Conexiones automáticas">
      <p>Dos equipos aprobados se conectan si el que se aprobó después tenía este ajuste activo, salvo exclusiones. Las políticas manuales no cambian.</p>
      <Field label="Conectar automáticamente los dispositivos aprobados"
        help="Solo afecta a las aprobaciones futuras: no cambia las conexiones existentes.">
        {props => <input {...props} type="checkbox" checked={autoConnectDraft ?? data.auto_connect}
          onChange={event => setAutoConnectDraft(event.target.checked)}/>}
      </Field>
      <RunButton variant="primary" run={async () => {
        await api.PATCH('/api/v1/networks/{net}', { params: { path: { net } },
          body: { auto_connect: autoConnectDraft ?? data.auto_connect } });
        setAutoConnectDraft(undefined);
        await refresh();
      }}>Guardar conexiones automáticas</RunButton>
    </Card>}
    <Dns net={net} canEdit={canEdit}/>
    <Card title="Certificado de emisor">
      {data && <CertificateStatus installed={data.has_issuer_cert} expires={data.issuer_cert_not_after}/>}
      {data?.root_mode === 'managed' ? <p>Este certificado lo firma y renueva el servidor solo (dura 30 días y se renueva al quedarle un tercio). No tienes que hacer nada.</p> :
        <p>Se firma fuera de línea con la raíz. Solo se sube el certificado público issuer.cert; nunca subas claves privadas.</p>}
      {data?.root_mode === 'managed' ? null : canEdit ? <Dialog trigger={data?.has_issuer_cert ? 'Reemplazar certificado' : 'Subir certificado'}
        title="Instalar certificado offline"
        description="Solo se sube issuer.cert público (178 bytes). Nunca subas claves privadas. Raíz y emisor se preparan offline (ADR-005).">
        <p>{data?.has_issuer_cert ? 'Hay un certificado instalado. Subir uno lo reemplaza.' : 'Falta el certificado de emisor.'}</p>
        {data?.has_issuer_cert && <p>El serial debe ser mayor que el del certificado instalado (Auditoría, último evento «network.issuer_cert»). El servidor rechaza uno menor porque los agentes lo descartarían y perderían su configuración.</p>}
        <p>La clave pública del emisor de este servidor es <code>{issuer.data?.issuer_pubkey ?? '(no disponible: consulta al operador)'}</code>
          {issuer.data && <CopyButton value={issuer.data.issuer_pubkey} label="Copiar clave pública del emisor"/>}</p>
        <pre>{`atenea issuer-cert --root raiz-offline --issuer-pub ${issuer.data?.issuer_pubkey ?? '<clave pública del emisor>'} --network-id ${net.replaceAll('-', '')} --serial ${data?.has_issuer_cert ? '<serial vigente + 1>' : 1} --valid-days ${data?.has_issuer_cert ? 90 : 30} --out issuer.cert`}</pre>
        <form className="incorporar-form" onSubmit={async event => {
          event.preventDefault(); setCertificateError(''); setMessage('');
          const form = event.currentTarget;
          if (!certificate || certificate.size !== 178) {
            setCertificateError('Selecciona un certificado binario de exactamente 178 bytes.'); return;
          }
          setPending(true);
          try {
            await api.PUT('/api/v1/networks/{net}/issuer-cert', {
              params: { path: { net } }, body: '', bodySerializer: () => certificate,
              headers: { 'Content-Type': 'application/octet-stream' },
            });
            await refresh(); setCertificate(null); form.reset(); setMessage('Certificado instalado.');
          } catch (cause) {
            setMessage(cause instanceof ApiError && cause.detail.error.code === 'serial_rollback'
              ? 'El certificado tiene un serial menor que el del certificado instalado: los agentes lo rechazarían. Emite uno con serial mayor.'
              : cause instanceof Error ? cause.message : 'No se pudo subir el certificado.');
          }
          finally { setPending(false); }
        }}>
          <Field label="Certificado público" help="Archivo binario issuer.cert de 178 bytes, firmado fuera de línea."
            error={certificateError}>{props => <input {...props} type="file"
              onChange={event => setCertificate(event.target.files?.[0] ?? null)}/>}</Field>
          <Button type="submit" variant="primary" loading={pending}>Subir issuer.cert</Button>
          <p role="status">{message}</p>
        </form>
      </Dialog> : <p>Solo lectura. Un administrador puede instalar o reemplazar el certificado.</p>}
    </Card>
    {canEdit && <Card title="Zona de peligro" className="danger-zone incorporar-danger">
      <p>Eliminar la red borra también sus dispositivos. Esta acción es irreversible.</p>
      <ConfirmDialog trigger="Eliminar red" title="Eliminar red definitivamente"
        description="Se eliminarán la red y sus dispositivos. Esta acción no se puede deshacer."
        confirmation={data?.name || net} label="Eliminar definitivamente" confirm={async () => {
          await api.DELETE('/api/v1/networks/{net}', { params: { path: { net } } });
          window.location.assign('/');
        }}/>
    </Card>}
  </div>;
}
