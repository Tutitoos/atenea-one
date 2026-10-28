// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router';
import { axe } from 'vitest-axe';
import { expect, it, vi } from 'vitest';
import { api } from '@atenea/api-client';
import { device } from './device.fixture';
import { Devices } from './devices';
import { Enrollment } from './enrollment';
import { Organizations } from './organizations';
import Panel, { Login } from './panel';
import { Relays, Topology } from './observability';
import { Policies } from './policies';
import { Audit, Metrics } from './observability';
const wrap=(children:React.ReactNode)=>render(<MemoryRouter><QueryClientProvider client={new QueryClient({defaultOptions:{queries:{retry:false}}})}>{children}</QueryClientProvider></MemoryRouter>);
it('no ofrece acciones de escritura a member y compara huella al confirmar',async()=>{
 const refresh=vi.fn(async()=>{});const {unmount}=wrap(<Devices devices={[device]} accessRole="member" net="red" elapsed={0} refresh={refresh} />);expect(screen.queryByRole('button',{name:'Aprobar'})).not.toBeInTheDocument();unmount();
 const user=userEvent.setup();wrap(<Devices devices={[device]} accessRole="admin" net="red" elapsed={0} refresh={refresh} />);await user.click(screen.getByRole('button',{name:'Aprobar'}));expect(screen.getByRole('dialog')).toHaveTextContent(device.fingerprint);await waitFor(()=>expect(screen.getByRole('button',{name:'Confirmar aprobación'})).toBeVisible());await user.keyboard('{Escape}');expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});
it('conserva la confirmación y la huella cuando avanza el reloj',async()=>{
 const props={devices:[device],accessRole:'admin' as const,net:'red',elapsed:0,refresh:async()=>{}};
 const user=userEvent.setup();const {rerender}=render(<Devices {...props}/>);
 await user.click(screen.getByRole('button',{name:'Aprobar'}));
 rerender(<Devices {...props} elapsed={5}/>);
 expect(screen.getByRole('dialog')).toHaveTextContent(device.fingerprint);
});
it('suma al último RX la edad del informe y el tiempo desde la consulta',async()=>{
 const reported={...device,report:{reported_at:'2026-01-01T00:00:00Z',age_secs:20,nat_mapping:null,paths:[{peer:device.id,path:'direct' as const,last_rx_secs:3}],forwarding_ok:null,dns:null,routes:null}};
 render(<Devices devices={[reported]} accessRole="member" net="red" elapsed={5} refresh={async()=>{}}/>);
 await userEvent.setup().click(screen.getByRole('button',{name:`Ver detalle de ${device.name}`}));
 expect(screen.getByText(/último RX 28 s/)).toBeInTheDocument();
});
it('solo muestra join en memoria tras crear y lo elimina al ocultar o desmontar',async()=>{
 const post=vi.spyOn(api,'POST').mockResolvedValue({data:{token:'token-sintetico',expires_at:'2026-01-01T01:00:00Z',join:{url:'https://example.invalid',token:'token-sintetico',network_id:device.id,root_anchor:'aa'.repeat(32)}},response:new Response()});
 const user=userEvent.setup();const {unmount}=wrap(<Enrollment net="red"/>);expect(screen.queryByText(/token-sintetico/)).not.toBeInTheDocument();await user.click(screen.getByRole('button',{name:'Crear token'}));await screen.findByRole('button',{name:'Copiar join'});expect(screen.getByText(/"token": "token-sintetico"/)).toBeVisible();await user.click(screen.getByRole('button',{name:/Ya lo guardé/}));expect(screen.queryByText(/"token": "token-sintetico"/)).not.toBeInTheDocument();unmount();wrap(<Enrollment net="red"/>);expect(screen.queryByRole('button',{name:'Copiar join'})).not.toBeInTheDocument();post.mockRestore();
});
it.each(['Dispositivo','Servidor'])('envía el tipo de token elegido: %s',async type=>{
 const post=vi.spyOn(api,'POST').mockResolvedValue({data:{token:'token-sintetico',expires_at:'2026-01-01T01:00:00Z',join:{url:'https://example.invalid',token:'token-sintetico',network_id:device.id,root_anchor:'aa'.repeat(32)}},response:new Response()});
 const user=userEvent.setup();wrap(<Enrollment net="red"/>);
 expect(screen.getByRole('combobox',{name:'Tipo'})).toHaveValue('Dispositivo');
 await user.selectOptions(screen.getByRole('combobox',{name:'Tipo'}),type);
 await user.click(screen.getByRole('button',{name:'Crear token'}));
 await screen.findByRole('button',{name:'Copiar join'});
 expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/enrollment-tokens',{params:{path:{net:'red'}},body:{ttl_secs:3600,auto_approve:false,role:type==='Servidor'?'server':'device'}});
 post.mockRestore();
});
it.each(['admin','owner'] as const)('permite a %s cambiar el tipo solo tras confirmar',async accessRole=>{
 const patch=vi.spyOn(api,'PATCH').mockResolvedValue({data:{device_id:device.id},response:new Response()});
 const refresh=vi.fn(async()=>{});const user=userEvent.setup();
 wrap(<Devices devices={[device]} accessRole={accessRole} net="red" elapsed={0} refresh={refresh}/>);
 expect(screen.getByText('Dispositivo',{selector:'.badge'})).toBeVisible();
 await user.click(screen.getByRole('button',{name:`Ver detalle de ${device.name}`}));
 await user.click(screen.getByRole('button',{name:'Cambiar tipo'}));
 expect(screen.getByRole('dialog')).toHaveTextContent('Dispositivo → Servidor');
 expect(patch).not.toHaveBeenCalled();
 await user.click(screen.getByRole('button',{name:'Confirmar cambio de tipo'}));
 await waitFor(()=>expect(refresh).toHaveBeenCalledOnce());
 expect(patch).toHaveBeenCalledWith('/api/v1/networks/{net}/devices/{dev}',{params:{path:{net:'red',dev:device.id}},body:{role:'server'}});
 patch.mockRestore();
});
it('muestra servidores a member sin permitir cambiar su tipo',()=>{
 wrap(<Devices devices={[{...device,role:'server'}]} accessRole="member" net="red" elapsed={0} refresh={async()=>{}}/>);
 expect(screen.getByText('Servidor',{selector:'.badge'})).toBeVisible();
 expect(screen.queryByRole('button',{name:'Cambiar tipo'})).not.toBeInTheDocument();
});
it('permite devolver un servidor a dispositivo',async()=>{
 const patch=vi.spyOn(api,'PATCH').mockResolvedValue({data:{device_id:device.id},response:new Response()});
 const refresh=vi.fn(async()=>{});const user=userEvent.setup();
 wrap(<Devices devices={[{...device,role:'server'}]} accessRole="admin" net="red" elapsed={0} refresh={refresh}/>);
 await user.click(screen.getByRole('button',{name:`Ver detalle de ${device.name}`}));
 await user.click(screen.getByRole('button',{name:'Cambiar tipo'}));
 expect(screen.getByRole('dialog')).toHaveTextContent('Servidor → Dispositivo');
 await user.click(screen.getByRole('button',{name:'Confirmar cambio de tipo'}));
 await waitFor(()=>expect(refresh).toHaveBeenCalledOnce());
 expect(patch).toHaveBeenCalledWith('/api/v1/networks/{net}/devices/{dev}',{params:{path:{net:'red',dev:device.id}},body:{role:'device'}});
 patch.mockRestore();
});
it.each([false,true])('explica la bidireccionalidad antes de crear y en la lista (edición: %s)',async canEdit=>{
 const get=vi.spyOn(api,'GET').mockResolvedValue({data:[{id:device.id,src:{kind:'any'},dst:{kind:'any'},proto:6,port_min:22,port_max:22}],response:new Response()});
 const user=userEvent.setup();wrap(<Policies net="red" canEdit={canEdit}/>);
 await screen.findByText('Cualquiera → Cualquiera');
 const notice=screen.getByRole('note',{name:'Bidireccionalidad de las políticas'});
 expect(notice).toBeVisible();
 expect(notice).toHaveTextContent('Una política A→B tcp/22 permite también a B iniciar tráfico TCP a A en cualquier puerto');
 expect(notice).toHaveTextContent('no se limita a respuestas');
 expect(screen.getAllByRole('note')).toHaveLength(1);
 if(canEdit){
  await user.click(screen.getByRole('button',{name:'Crear política'}));
  const dialog=screen.getByRole('dialog',{name:'Crear política'});
  expect(dialog).toHaveAccessibleDescription(/Una política A→B tcp\/22 permite también a B iniciar tráfico TCP a A en cualquier puerto/);
  expect((await axe(dialog,{rules:{region:{enabled:false},'color-contrast':{enabled:false}}})).violations).toEqual([]);
 }else{
  expect(screen.queryByRole('button',{name:'Crear política'})).not.toBeInTheDocument();
 }
 get.mockRestore();
});
it('pantallas principales sin infracciones básicas de accesibilidad',async()=>{
 const get=vi.spyOn(api,'GET').mockResolvedValue({data:[],response:new Response()});
 const cases=[<Login/>,<Organizations org={{id:device.id,name:'Organización de prueba',role:'owner'}} choose={()=>{}}/>,<Devices devices={[device]} accessRole="member" net="red" elapsed={0} refresh={async()=>{}} />,<Enrollment net="red"/>,<Relays devices={[]} elapsed={0}/>,<Topology devices={[device]} elapsed={0}/>,<Policies net="red" canEdit/>,<Metrics net="red"/>,<Audit net="red"/>];
 // jsdom no implementa canvas/layout; el contraste necesita comprobación en navegador real.
 for(const component of cases){const {container,unmount}=wrap(component);expect((await axe(container,{rules:{region:{enabled:false},'color-contrast':{enabled:false}}})).violations).toEqual([]);unmount();}
 get.mockRestore();
});
it('un enlace con solo la red (desde la aplicación de escritorio) selecciona su organización',async()=>{
  const org='0199ab12-3456-7def-8abc-0123456789aa',net='0199ab12-3456-7def-8abc-0123456789bb';
  vi.spyOn(api,'GET').mockImplementation((async(path:string)=>{
    if(path==='/api/v1/session')return {data:{user:{display_name:'Prueba'},memberships:[{org_id:org,role:'admin'}],csrf_token:'x'}};
    if(path==='/api/v1/orgs')return {data:[{id:org,name:'org-de-prueba',role:'admin'}]};
    if(path==='/api/v1/networks/{net}')return {data:{id:net,org_id:org,name:'red',ipv4_cidr:'10.0.0.0/24',ipv6_cidr:'fd00::/64',root_anchor:'00',has_issuer_cert:true,join_policy:'cerrada'}};
    return {data:[]};
  }) as never);
  render(<MemoryRouter initialEntries={[`/?net=${net}&view=Dispositivos`]}><QueryClientProvider client={new QueryClient({defaultOptions:{queries:{retry:false}}})}><Panel/></QueryClientProvider></MemoryRouter>);
  await waitFor(()=>expect(screen.getByRole('combobox',{name:/Organización/})).toHaveValue(org));
  expect(screen.getByRole('navigation',{name:'Secciones de red'})).toBeInTheDocument();
});

it('crea políticas por nombre y prefijo con protocolo legible y validación', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [], response: new Response() });
  const post = vi.spyOn(api, 'POST').mockResolvedValue({ response: new Response() });
  const user = userEvent.setup();
  wrap(<Policies net="red" canEdit devices={[device]}/>);
  await user.click(screen.getByRole('button', { name: 'Crear política' }));
  await user.selectOptions(screen.getByLabelText('Selector origen'), 'device');
  await user.selectOptions(screen.getByLabelText('Dispositivo de origen'), device.id);
  expect(screen.getByRole('option', { name: device.name })).toBeInTheDocument();
  await user.selectOptions(screen.getByLabelText('Selector destino'), 'prefix');
  await user.type(screen.getByLabelText('Prefijo de destino'), 'incorrecto');
  await user.selectOptions(screen.getByLabelText('Protocolo'), '6');
  await user.click(screen.getByRole('button', { name: 'Revisar política' }));
  expect(post).not.toHaveBeenCalled();
  expect(screen.getByLabelText('Prefijo de destino')).toHaveAttribute('aria-invalid', 'true');
  expect((await axe(screen.getByRole('dialog'), {
    rules: { region: { enabled: false }, 'color-contrast': { enabled: false } },
  })).violations).toEqual([]);
  await user.clear(screen.getByLabelText('Prefijo de destino'));
  await user.type(screen.getByLabelText('Prefijo de destino'), '10.0.0.0/24');
  await user.click(screen.getByRole('button', { name: 'Revisar política' }));
  expect(post).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar creación' }));
  await waitFor(() => expect(post).toHaveBeenCalledWith('/api/v1/networks/{net}/policies', {
    params: { path: { net: 'red' } },
    body: { src: { kind: 'device', device_id: device.id },
      dst: { kind: 'prefix', prefix: '10.0.0.0/24' }, proto: 6, port_min: 0, port_max: 0 },
  }));
});

it('traduce auditoría, conserva desconocidas y permite filtrar la página', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [
    { id: 2, at: '2026-01-01T00:00:00Z', action: 'device.approve',
      actor_kind: 'user', actor_id: device.id, target: device.id, detail: { ejemplo: true } },
    { id: 1, at: '2026-01-01T00:00:00Z', action: 'future.action',
      actor_kind: 'device', actor_id: device.id, target: device.id, detail: {} },
  ], response: new Response() });
  const user = userEvent.setup();
  const { container } = wrap(<Audit net="red" devices={[device]}/>);
  expect(await screen.findByText('Dispositivo aprobado')).toBeVisible();
  expect(screen.getByText('future.action')).toBeVisible();
  expect(screen.getAllByText(device.name).length).toBeGreaterThan(0);
  expect((await axe(container, {
    rules: { region: { enabled: false }, 'color-contrast': { enabled: false } },
  })).violations).toEqual([]);
  await user.type(screen.getByLabelText('Filtrar eventos de esta página'), 'aprobado');
  expect(screen.queryByText('future.action')).not.toBeInTheDocument();
  await user.click(screen.getByText('Ver detalle JSON'));
  expect(screen.getByText(/"ejemplo": true/)).toBeVisible();
});

it('muestra nombres en topología y acorta pares desconocidos', () => {
  const peer = '22222222-2222-4222-8222-222222222222';
  wrap(<Topology devices={[{ ...device, report: {
    reported_at: '2026-01-01T00:00:00Z', age_secs: 1, nat_mapping: null,
    paths: [{ peer, path: 'direct', last_rx_secs: 0 }],
    forwarding_ok: null, dns: null, routes: null,
  } }]} elapsed={0}/>);
  expect(screen.getByRole('cell', { name: device.name })).toBeVisible();
  expect(screen.getByTitle(peer)).toHaveTextContent('22222222…');
});

it('encierra métricas en un contenedor desplazable', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [
    { at: '2026-01-01T00:00:00Z', total: 1, direct: 1, relay: 0, probing: 0, unknown: 0, offline: 0 },
  ], response: new Response() });
  wrap(<Metrics net="red"/>);
  const table = await screen.findByRole('table', { name: 'Muestras recibidas' });
  expect(table.parentElement).toHaveClass('table-scroll');
});

it('muestra nombres y protocolos en políticas y confirma el borrado', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [{
    id: device.id, src: { kind: 'device', device_id: device.id },
    dst: { kind: 'prefix', prefix: '10.0.0.0/24' }, proto: 17, port_min: 53, port_max: 53,
  }], response: new Response() });
  const remove = vi.spyOn(api, 'DELETE').mockResolvedValue({ response: new Response() });
  const user = userEvent.setup();
  wrap(<Policies net="red" canEdit devices={[device]}/>);
  expect(await screen.findByText(`${device.name} → 10.0.0.0/24`)).toBeVisible();
  expect(screen.getByText('UDP · Puerto 53')).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Borrar' }));
  expect(remove).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Confirmar borrado' }));
  await waitFor(() => expect(remove).toHaveBeenCalledOnce());
});

it('valida el prefijo de origen y los puertos antes de confirmar', async () => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [], response: new Response() });
  const user = userEvent.setup();
  wrap(<Policies net="red" canEdit devices={[device]}/>);
  await user.click(screen.getByRole('button', { name: 'Crear política' }));
  await user.selectOptions(screen.getByLabelText('Selector origen'), 'prefix');
  await user.type(screen.getByLabelText('Prefijo de origen'), '10.0.0.1/24');
  await user.selectOptions(screen.getByLabelText('Selector destino'), 'device');
  await user.selectOptions(screen.getByLabelText('Dispositivo de destino'), device.id);
  await user.selectOptions(screen.getByLabelText('Protocolo'), '17');
  await user.click(screen.getByRole('button', { name: 'Revisar política' }));
  expect(screen.getByLabelText('Prefijo de origen')).toHaveAttribute('aria-invalid', 'true');
  await user.clear(screen.getByLabelText('Prefijo de origen'));
  await user.type(screen.getByLabelText('Prefijo de origen'), '10.0.0.0/24');
  await user.clear(screen.getByLabelText('Puerto mínimo'));
  await user.type(screen.getByLabelText('Puerto mínimo'), '100');
  await user.click(screen.getByRole('button', { name: 'Revisar política' }));
  expect(screen.getByLabelText('Puerto mínimo')).toHaveAttribute('aria-invalid', 'true');
  expect(screen.queryByRole('button', { name: 'Confirmar creación' })).not.toBeInTheDocument();
  await user.clear(screen.getByLabelText('Puerto máximo'));
  await user.type(screen.getByLabelText('Puerto máximo'), '100');
  await user.click(screen.getByRole('button', { name: 'Revisar política' }));
  expect(screen.getByRole('button', { name: 'Confirmar creación' })).toBeVisible();
});

it('conserva la paginación de auditoría de 50 eventos al filtrar', async () => {
  const get = vi.spyOn(api, 'GET').mockResolvedValue({ data: Array.from({ length: 50 }, (_, index) => ({
    id: 50 - index, at: '2026-01-01T00:00:00Z', action: 'device.approve',
    actor_kind: 'user', actor_id: device.id, target: device.id, detail: {},
  })), response: new Response() });
  const user = userEvent.setup();
  wrap(<Audit net="red" devices={[device]}/>);
  await screen.findAllByText('Dispositivo aprobado');
  await user.type(screen.getByLabelText('Filtrar eventos de esta página'), 'sin coincidencias');
  await user.click(screen.getByRole('button', { name: 'Más antiguos' }));
  await waitFor(() => expect(get).toHaveBeenCalledWith('/api/v1/networks/{net}/audit', {
    params: { path: { net: 'red' }, query: { limit: 50, before: 1 } },
  }));
  await user.click(screen.getByRole('button', { name: 'Más recientes' }));
  expect(screen.getByRole('button', { name: 'Más recientes' })).toBeDisabled();
});

it.each([
  [0, 0, 0, 'Cualquiera · Todos los puertos'],
  [6, 0, 0, 'TCP · Todos los puertos'],
  [6, 22, 22, 'TCP · Puerto 22'],
  [6, 80, 443, 'TCP · Puertos 80–443'],
])('describe los puertos de protocolo %s (%s–%s)', async (proto, min, max, label) => {
  vi.spyOn(api, 'GET').mockResolvedValue({ data: [{
    id: device.id, src: { kind: 'any' }, dst: { kind: 'any' },
    proto, port_min: min, port_max: max,
  }], response: new Response() });
  wrap(<Policies net="red" canEdit={false}/>);
  expect(await screen.findByText(String(label))).toBeVisible();
});
