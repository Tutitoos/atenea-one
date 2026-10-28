// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Link, useSearchParams } from 'react-router';
import type { UseQueryResult } from '@tanstack/react-query';
import type { Model } from '@atenea/api-client';
import { Devices } from './devices';
import { Enrollment } from './enrollment';
import { JoinNetwork } from './join-network';
import { Policies } from './policies';
import { Subnets } from './subnets';
import { NetworkSettings } from './network-settings';
import { Audit, Metrics, Relays, Topology } from './observability';
import { Overview } from './overview';
import { Status } from './ui';

// Contrato del registro: las vistas nuevas reciben contexto; las existentes conservan sus props.
export type SectionProps = {
  net: string;
  network: UseQueryResult<Model<'Network'>>;
  devices: UseQueryResult<Model<'Device'>[]>;
  routes: UseQueryResult<Model<'Route'>[]>;
  role: Model<'Role'>;
  canEdit: boolean;
  elapsed: number;
  refresh: () => Promise<unknown>;
};
function DeviceSection(props: SectionProps) {
  const [params] = useSearchParams();
  const pendingOnly = params.get('status') === 'pending';
  return <><Status query={props.devices}/><Status query={props.routes}/>
    {pendingOnly && <p>Solicitudes pendientes · <Link to={`/?org=${props.network.data?.org_id ?? ''}&net=${props.net}&view=Dispositivos`}>Ver todos los dispositivos</Link></p>}
    {props.routes.isError && <p className="warning">No se han podido consultar las rutas aprobadas; no se puede evaluar la advertencia de forwarding del router.</p>}
    {props.devices.data && <Devices key={pendingOnly ? 'pending' : 'all'}
      devices={props.devices.data} status={pendingOnly ? 'pending' : 'all'}
      accessRole={props.role} net={props.net} elapsed={props.elapsed}
      refresh={props.refresh} routes={props.routes.data}/>}</>;
}
function EnrollmentSection({ network, net, canEdit }: SectionProps) {
  return <>{network.data && <JoinNetwork net={net} canEdit={canEdit} policy={network.data.join_policy}/>}
    {canEdit ? <Enrollment net={net}/> : <p>Solo administradores y propietarios pueden crear tokens.</p>}</>;
}
function ObservabilitySection({ net, devices, elapsed }: SectionProps) {
  return <><Status query={devices}/>{devices.data && <>
    <Topology devices={devices.data} elapsed={elapsed}/><Relays devices={devices.data} elapsed={elapsed}/>
  </>}<Metrics net={net}/><Audit net={net} devices={devices.data ?? []}/></>;
}
// Añade una entrada con id URL estable, etiqueta, grupo y Component; el shell consume este único registro.
export const sections: { id: string; label: string; group: string; Component: React.ComponentType<SectionProps> }[] = [
  { id: 'Resumen', label: 'Resumen', group: 'Inicio', Component: Overview },
  { id: 'Dispositivos', label: 'Dispositivos', group: 'Inicio', Component: DeviceSection },
  { id: 'Incorporar', label: 'Incorporar', group: 'Inicio', Component: EnrollmentSection },
  { id: 'Red', label: 'DNS y certificado', group: 'Red', Component: NetworkSettings },
  {
    id: 'Subredes', label: 'Subredes', group: 'Red',
    Component: props => <Subnets net={props.net} canEdit={props.canEdit} devices={props.devices.data ?? []}/>,
  },
  {
    id: 'Políticas', label: 'Políticas', group: 'Red',
    Component: props => <Policies net={props.net} canEdit={props.canEdit} devices={props.devices.data ?? []}/>,
  },
  { id: 'Observabilidad', label: 'Observabilidad', group: 'Observabilidad', Component: ObservabilitySection },
  {
    id: 'Topología', label: 'Topología', group: 'Observabilidad',
    Component: props => <><Status query={props.devices}/>{props.devices.data &&
      <Topology devices={props.devices.data} elapsed={props.elapsed}/>}</>,
  },
  { id: 'Métricas', label: 'Métricas', group: 'Observabilidad', Component: props => <Metrics net={props.net}/> },
  {
    id: 'Relays', label: 'Relays', group: 'Observabilidad',
    Component: props => <><Status query={props.devices}/>{props.devices.data &&
      <Relays devices={props.devices.data} elapsed={props.elapsed}/>}</>,
  },
  { id: 'Auditoría', label: 'Auditoría', group: 'Observabilidad',
    Component: props => <Audit net={props.net} devices={props.devices.data ?? []}/> },
];
