import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, result, type Model } from '@atenea/api-client';
import { ActionForm, Button, Dialog, RunButton, Status } from './ui';
import { Organizations } from './organizations';
import { Devices } from './devices';
import { Enrollment } from './enrollment';
import { Policies } from './policies';
import { Audit, Metrics, Relays, Topology } from './observability';
import { useClock, useNetworkEvents } from './live';
import { secondsSince } from './domain';
const views=['Dispositivos','Incorporar','Políticas','Relays','Topología','Métricas','Auditoría'];
function Theme() { const [theme,setTheme]=useState('sistema'); useEffect(()=>{setTheme(localStorage.getItem('atenea-theme')??'sistema');},[]); useEffect(()=>{const media=window.matchMedia('(prefers-color-scheme: dark)');const apply=()=>{document.documentElement.dataset.theme=theme==='sistema'?(media.matches?'oscuro':'claro'):theme;};apply();media.addEventListener('change',apply);return()=>media.removeEventListener('change',apply);},[theme]);return <label>Tema<select value={theme} onChange={e=>{setTheme(e.target.value);localStorage.setItem('atenea-theme',e.target.value);}}>{['sistema','claro','oscuro'].map(t=><option key={t}>{t}</option>)}</select></label>; }
export function Login({ expired=false }: { expired?:boolean }) { return <main className="login"><div className="card"><p className="eyebrow">ATENEA ONE</p><h1>Tu red, bajo tu control.</h1><p>{expired?'Tu sesión ha caducado o no has iniciado sesión. Tus cambios sin guardar no se enviarán.':'Identifícate con el proveedor de tu organización para operar la red privada.'}</p><a className="button primary" href="/auth/login">Iniciar sesión con OIDC</a><p className="muted">Las claves privadas permanecen fuera del panel.</p></div></main>; }
export default function Panel() {
 const [params,setParams]=useSearchParams();const cache=useQueryClient();
 const session=useQuery({queryKey:['session'],queryFn:()=>result(api.GET('/api/v1/session')),refetchInterval:30_000});
 const orgs=useQuery({queryKey:['organizations'],queryFn:()=>result(api.GET('/api/v1/orgs')),enabled:!!session.data});
 const selected=orgs.data?.find(o=>o.id===params.get('org')); const net=params.get('net')??''; const view=params.get('view')??views[0];
 const navigate=(org:string,network='',page=views[0])=>setParams({org,...(network?{net:network,view:page}:{})});
 const expired=session.error instanceof ApiError&&session.error.status===401;
 if(expired)return <Login expired/>;
 if(!session.data)return <main><h1>Atenea One</h1><Status query={session}/>{session.isError&&<RunButton run={()=>session.refetch()}>Reintentar</RunButton>}</main>;
 return <div className="shell">
   <a className="skip" href="#contenido">Saltar al contenido</a>
   <aside>
     <header><a href="/" className="brand">△ Atenea One</a><p className="eyebrow">PLANO DE CONTROL</p></header>
     <label>Organización<select value={selected?.id??''} onChange={e=>navigate(e.target.value)}><option value="">Selecciona organización</option>{orgs.data?.map(o=><option key={o.id} value={o.id}>{o.name} · {o.role}</option>)}</select></label>
     <Status query={orgs}/>
     <Button onClick={()=>navigate(selected?.id??'')}>Organizaciones y redes</Button>
     {net&&selected&&<details className="network-menu" open><summary>Menú de red</summary><nav aria-label="Secciones de red">{views.map(v=><Button key={v} aria-current={view===v?'page':undefined} onClick={()=>navigate(selected.id,net,v)}>{v}</Button>)}</nav></details>}
     <footer><Theme/><p className="muted">{session.data.user.display_name??session.data.user.email??'Usuario autenticado'}</p><RunButton run={async()=>{await api.POST('/auth/logout');cache.clear();window.location.assign('/');}}>Cerrar sesión</RunButton></footer>
   </aside>
   <main id="contenido">{net&&selected?<Network key={net} net={net} view={view} memberships={session.data.memberships}/>:<Organizations org={selected} choose={n=>navigate(selected!.id,n)}/>}</main>
 </div>;
}
function Network({net,view,memberships}:{net:string;view:string;memberships:Model<'Membership'>[]}) {
 const cache=useQueryClient();const connected=useNetworkEvents(net);const now=useClock();
 const query=useQuery({queryKey:['network',net,'detail'],queryFn:()=>result(api.GET('/api/v1/networks/{net}',{params:{path:{net}}}))});
 const devices=useQuery({queryKey:['network',net,'devices'],queryFn:()=>result(api.GET('/api/v1/networks/{net}/devices',{params:{path:{net}}}))});
 const membership=memberships.find(m=>m.org_id===query.data?.org_id)?.role??'member';
 const [certificate,setCertificate]=useState<File|null>(null);const canEdit=membership!=='member';const elapsed=secondsSince(devices.dataUpdatedAt,now);
 const refresh=()=>cache.invalidateQueries({queryKey:['network',net]});
 return <><div className="page-heading"><div><p className="eyebrow">RED PRIVADA</p><h1>{query.data?.name??'Red'}</h1><p className="muted">{query.data?.ipv4_cidr} · {query.data?.ipv6_cidr}</p></div>{canEdit&&<Dialog trigger="Certificado de emisor" title="Instalar certificado offline" description="Solo se sube issuer.cert público (178 bytes). Nunca subas claves privadas. Raíz y emisor se preparan offline (ADR-005)."><p>{query.data?.has_issuer_cert?'Hay un certificado instalado. Subir uno lo reemplaza.':'Falta el certificado de emisor.'}</p><pre>{`atenea issuer-cert --root raiz-offline --issuer-key emisor-offline --network-id ${net} --serial 1 --valid-days 30 --out issuer.cert`}</pre><label>Certificado público<input type="file" onChange={e=>setCertificate(e.target.files?.[0]??null)}/></label><RunButton run={async()=>{if(!certificate||certificate.size!==178)throw new Error('Selecciona un certificado binario de exactamente 178 bytes.');await api.PUT('/api/v1/networks/{net}/issuer-cert',{params:{path:{net}},body:'',bodySerializer:()=>certificate,headers:{'Content-Type':'application/octet-stream'}});setCertificate(null);await refresh();}}>Subir issuer.cert</RunButton></Dialog>}</div><Status query={query}/>{!connected&&<p className="warning" role="status">Desconectado: los datos pueden estar desactualizados. Sondeo REST de respaldo cada 30 s.</p>}<p className="muted">{connected?'Eventos en vivo conectados. ':''}Los informes son declaraciones autenticadas de los agentes, no verificaciones independientes.</p>{['Dispositivos','Relays','Topología'].includes(view)&&<Status query={devices}/>} {view==='Dispositivos'&&devices.data&&<Devices devices={devices.data} accessRole={membership} net={net} elapsed={elapsed} refresh={refresh} />} {view==='Incorporar'&&(canEdit?<Enrollment net={net}/>:<p>Solo owner o admin puede crear tokens.</p>)} {view==='Políticas'&&<Policies net={net} canEdit={canEdit}/>} {view==='Relays'&&devices.data&&<Relays devices={devices.data} elapsed={elapsed}/>} {view==='Topología'&&devices.data&&<Topology devices={devices.data} elapsed={elapsed}/>} {view==='Métricas'&&<Metrics net={net}/>} {view==='Auditoría'&&<Audit net={net}/>} {view==='Dispositivos'&&canEdit&&<details><summary>Zona de peligro</summary><Dialog trigger="Eliminar red" title="Eliminar red definitivamente" description="Se eliminarán la red y sus dispositivos. Esta acción no se puede deshacer."><ActionForm label="Eliminar definitivamente" fields={[{name:'confirmation',label:'Escribe el UUID de la red para confirmar'}]} submit={async v=>{if(v.confirmation!==net)throw new Error('El UUID no coincide.');await api.DELETE('/api/v1/networks/{net}',{params:{path:{net}}});window.location.assign('/');}}/></Dialog></details>}</>;
}
