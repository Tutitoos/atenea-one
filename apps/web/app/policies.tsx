// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useQuery } from '@tanstack/react-query';
import { api, result, type Model } from '@atenea/api-client';
import { ActionForm, Dialog, Empty, RunButton, Status } from './ui';
import { policySchema } from './domain';
const policyNotice = 'Las políticas son efectivamente bidireccionales: la regla inversa mantiene el protocolo, pero no restringe puertos. Una política A→B tcp/22 permite también a B iniciar tráfico TCP a A en cualquier puerto; no se limita a respuestas de conexiones iniciadas por A.';
export function Policies({ net, canEdit }: { net: string; canEdit: boolean }) {
  const params = { path: { net } };
  const query = useQuery({ queryKey: ['network', net, 'policies'], queryFn: () => result(api.GET('/api/v1/networks/{net}/policies', { params })) });
  const select = (kind: string, value: string) => kind === 'device' ? { kind, device_id: value } : kind === 'prefix' ? { kind, prefix: value } : { kind: 'any' };
  const describe = (s: Model<'Selector'>) => s.kind === 'any' ? 'Cualquiera' : s.kind === 'device' ? s.device_id : s.prefix;
  return <section><h2>Políticas</h2><p>Sin políticas no hay pares. Los prefijos deben estar dentro de los CIDR de la red; el servidor comprueba pertenencia y límites.</p>
    <aside className="card" role="note" aria-label="Bidireccionalidad de las políticas"><strong>ACL sin estado</strong><p>{policyNotice}</p></aside>
    <Status query={query}/>{canEdit && <Dialog trigger="Crear política" title="Crear política" description={`${policyNotice} Los puertos solo se aplican a TCP/UDP. Usa 0–0 para no restringirlos.`}><ActionForm label="Guardar política" fields={[{name:'srcKind',label:'Selector origen',value:'any',options:['any','device','prefix']},{name:'srcValue',label:'UUID o prefijo de origen',required:false},{name:'dstKind',label:'Selector destino',value:'any',options:['any','device','prefix']},{name:'dstValue',label:'UUID o prefijo de destino',required:false},{name:'proto',label:'Protocolo (0 cualquiera, 1 ICMP, 6 TCP, 17 UDP, 58 ICMPv6)',value:'0',options:['0','1','6','17','58']},{name:'min',label:'Puerto mínimo',value:'0',type:'number'},{name:'max',label:'Puerto máximo',value:'0',type:'number'}]} submit={async v => { const body = policySchema.parse({ src: select(v.srcKind,v.srcValue), dst: select(v.dstKind,v.dstValue), proto: Number(v.proto), port_min: Number(v.min), port_max: Number(v.max) }); await api.POST('/api/v1/networks/{net}/policies', { params, body }); await query.refetch(); }}/></Dialog>}{query.data?.length ? <ul className="cards">{query.data.map(p => <li className="card" key={p.id}><code>{describe(p.src)} → {describe(p.dst)}</code><p>Protocolo {p.proto} · Puertos {p.port_min}–{p.port_max}</p>{canEdit && <Dialog trigger="Borrar" title="Borrar política" description="Los pares afectados dejarán de comunicarse al renovar la configuración."><RunButton run={async () => { await api.DELETE('/api/v1/networks/{net}/policies/{id}', {params:{path:{net,id:p.id}}}); await query.refetch(); }}>Confirmar borrado</RunButton></Dialog>}</li>)}</ul> : !query.isPending && !query.isError && <Empty>No hay políticas.</Empty>}</section>;
}
