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
import { Login } from './panel';
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
it('suma al último RX la edad del informe y el tiempo desde la consulta',()=>{
 const reported={...device,report:{reported_at:'2026-01-01T00:00:00Z',age_secs:20,nat_mapping:null,paths:[{peer:device.id,path:'direct' as const,last_rx_secs:3}]}};
 render(<Devices devices={[reported]} accessRole="member" net="red" elapsed={5} refresh={async()=>{}}/>);
 expect(screen.getByText(/último RX 28 s/)).toBeInTheDocument();
});
it('solo muestra join en memoria tras crear y lo elimina al ocultar o desmontar',async()=>{
 const post=vi.spyOn(api,'POST').mockResolvedValue({data:{token:'token-sintetico',expires_at:'2026-01-01T01:00:00Z',join:{url:'https://example.invalid',token:'token-sintetico',network_id:device.id,root_anchor:'aa'.repeat(32)}},response:new Response()});
 const user=userEvent.setup();const {unmount}=wrap(<Enrollment net="red"/>);expect(screen.queryByText(/token-sintetico/)).not.toBeInTheDocument();await user.click(screen.getByRole('button',{name:'Crear token'}));await screen.findByRole('button',{name:'Copiar join'});expect(screen.getByText(/"token": "token-sintetico"/)).toBeVisible();await user.click(screen.getByRole('button',{name:/Ya lo guardé/}));expect(screen.queryByText(/"token": "token-sintetico"/)).not.toBeInTheDocument();unmount();wrap(<Enrollment net="red"/>);expect(screen.queryByRole('button',{name:'Copiar join'})).not.toBeInTheDocument();post.mockRestore();
});
it('pantallas principales sin infracciones básicas de accesibilidad',async()=>{
 const get=vi.spyOn(api,'GET').mockResolvedValue({data:[],response:new Response()});
 const cases=[<Login/>,<Organizations org={{id:device.id,name:'Organización de prueba',role:'owner'}} choose={()=>{}}/>,<Devices devices={[device]} accessRole="member" net="red" elapsed={0} refresh={async()=>{}} />,<Enrollment net="red"/>,<Relays devices={[]} elapsed={0}/>,<Topology devices={[device]} elapsed={0}/>,<Policies net="red" canEdit/>,<Metrics net="red"/>,<Audit net="red"/>];
 // jsdom no implementa canvas/layout; el contraste necesita comprobación en navegador real.
 for(const component of cases){const {container,unmount}=wrap(component);expect((await axe(container,{rules:{region:{enabled:false},'color-contrast':{enabled:false}}})).violations).toEqual([]);unmount();}
 get.mockRestore();
});
