// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Link } from 'react-router';
import { useQuery } from '@tanstack/react-query';
import { api, result } from '@atenea/api-client';
import { Card, Status } from './ui';
import { connection, relays } from './domain';
import type { SectionProps } from './sections';
import { auditAction, AuditChange, DeviceIdentity } from './observability';
import { certificateExpiry, CertificateStatus, ServerInfo } from './server-status';

// Reutiliza las claves del shell; cada tarjeta mantiene su propio estado de consulta.
export function Overview({ net, network, devices, elapsed }: SectionProps) {
  const policies = useQuery({ queryKey: ['network', net, 'policies'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/policies', { params: { path: { net } } })) });
  const audit = useQuery({ queryKey: ['network', net, 'audit', 'overview'],
    queryFn: () => result(api.GET('/api/v1/networks/{net}/audit', { params: { path: { net }, query: { limit: 5 } } })) });
  const rows = devices.data;
  const pending = rows?.filter(device => device.status === 'pending').length;
  const relayCount = rows ? relays(rows, elapsed).length : 0;
  const expiry = network.data?.has_issuer_cert ? certificateExpiry(network.data.issuer_cert_not_after) : null;
  return <div className="overview-grid">
    {expiry && expiry.days <= 14 && <section role="note" aria-label="Caducidad del certificado"
      className={`card overview-audit certificate-notice badge-${expiry.tone}`}>
      <h2>{expiry.days <= 0 ? 'Certificado de emisor caducado' : 'El certificado de emisor caduca pronto'}</h2>
      <p>Renueva el certificado con la raíz offline y súbelo en DNS y certificado.</p>
    </section>}
    <Card title="Dispositivos"><Status query={devices}/>{rows && <dl className="counts">
      <div><dt>Aprobados</dt><dd>{rows.filter(device => device.status === 'approved').length}</dd></div>
      <div><dt>Pendientes</dt><dd>{pending}</dd></div>
      <div><dt>Revocados</dt><dd>{rows.filter(device => device.status === 'revoked').length}</dd></div>
      <div><dt>Conectados</dt><dd>{rows.filter(device => connection(device.report, elapsed) === 'online').length}</dd></div>
    </dl>}</Card>
    <Card title="Solicitudes pendientes"><Status query={devices}/>{rows && (pending ? <>
      <p>{pending === 1 ? '1 solicitud requiere revisión.' : `${pending} solicitudes requieren revisión.`}</p>
      <Link className="button primary" to={`/?org=${network.data?.org_id ?? ''}&net=${net}&view=Dispositivos&status=pending`}>Revisar pendientes</Link>
    </> : <p>No hay solicitudes pendientes.</p>)}</Card>
    <Card title="Política de unión"><Status query={network}/>{network.data &&
      <p>{network.data.join_policy === 'con_solicitud' ? 'Con solicitud' : 'Cerrada (solo con token)'}</p>}</Card>
    <Card title="Políticas"><Status query={policies}/>{policies.data &&
      <p>{policies.data.length} {policies.data.length === 1 ? 'política' : 'políticas'}</p>}</Card>
    <Card title="Certificado de emisor"><Status query={network}/>{network.data &&
      <CertificateStatus installed={network.data.has_issuer_cert} expires={network.data.issuer_cert_not_after}/>}</Card>
    <Card title="Relays declarados"><Status query={devices}/>{rows &&
      <p>{relayCount} {relayCount === 1 ? 'relay observado' : 'relays observados'} por los agentes.</p>}</Card>
    <ServerInfo/>
    <Card title="Últimos eventos de auditoría" className="overview-audit"><Status query={audit}/>
      {audit.data && (audit.data.length ? <ol>{audit.data.slice(0, 5).map(event => <li key={event.id}>
        <div className="overview-event">
          <time dateTime={event.at}>{new Date(event.at).toLocaleString('es')}</time>
          <span>{auditAction(event.action)}</span>
          <span>Destino: <DeviceIdentity id={event.target} devices={rows}/></span>
          <span>{event.actor_kind === 'user' ? 'Usuario' : event.actor_kind === 'device' ? 'Dispositivo' : event.actor_kind}:
            <DeviceIdentity id={event.actor_id} devices={rows}/></span>
        </div>
        <AuditChange action={event.action} detail={event.detail}/>
        <details><summary>Ver detalle JSON</summary><pre>{JSON.stringify(event.detail, null, 2)}</pre></details>
      </li>)}</ol> : <p>No hay eventos de auditoría.</p>)}
    </Card>
  </div>;
}
