// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { axe } from 'vitest-axe';
import { expect, it, vi, afterEach } from 'vitest';
import { api, ApiError } from '@atenea/api-client';
import { JoinNetwork } from './join-network';
import { Enrollment } from './enrollment';
import { Devices } from './devices';
import { device } from './device.fixture';

const info = { code:'ATENEA1-CODIGO-SINTETICO',root_anchor_fingerprint:'aaaa-bbbb-cccc-dddd',join_policy:'cerrada' as const,name:'Red sintética',url:'https://control.example.test' };
const wrap = (child:React.ReactNode) => render(<QueryClientProvider client={new QueryClient({defaultOptions:{queries:{retry:false}}})}>{child}</QueryClientProvider>);
afterEach(()=>vi.restoreAllMocks());
it('muestra código y huella, copia con confirmación y es accesible',async()=>{
 vi.spyOn(api,'GET').mockResolvedValue({data:info,response:new Response()});
 const user=userEvent.setup(); const copy=vi.spyOn(navigator.clipboard,'writeText').mockResolvedValue();
 const {container}=wrap(<JoinNetwork net="red" canEdit policy="cerrada"/>);
 expect(await screen.findByLabelText('Código de red')).toHaveValue(info.code);
 expect(screen.getByText(info.root_anchor_fingerprint)).toBeVisible();
 await user.click(screen.getByRole('button',{name:'Copiar código de red'}));
 expect(copy).toHaveBeenCalledWith(info.code);
 expect(await screen.findByText('Código copiado.')).toHaveAttribute('role','status');
 expect((await axe(container,{rules:{region:{enabled:false},'color-contrast':{enabled:false}}})).violations).toEqual([]);
});
it.each([
 ['invalid_network_name_for_code',/Renombra la red en DNS y certificado/],
 ['invalid_public_url_for_code',/Corrige la URL pública del servidor/],
])('explica el conflicto %s',async(code,message)=>{
 vi.spyOn(api,'GET').mockRejectedValue(new ApiError(409,{error:{code,message:code}}));
 wrap(<JoinNetwork net="red" canEdit policy="cerrada"/>);
 expect(await screen.findByText(message)).toBeVisible();
});
it('no consulta ni muestra código ni controles para miembros',()=>{
 const get=vi.spyOn(api,'GET');
 wrap(<JoinNetwork net="red" canEdit={false} policy="con_solicitud"/>);
 expect(screen.getByText('Con solicitud (un administrador aprueba)')).toBeVisible();
 expect(screen.queryByRole('button')).not.toBeInTheDocument();
 expect(screen.queryByLabelText('Código de red')).not.toBeInTheDocument();
 expect(get).not.toHaveBeenCalled();
});
it('requiere confirmación para solicitudes y cancelar no modifica la política',async()=>{
 vi.spyOn(api,'GET').mockResolvedValue({data:info,response:new Response()});
 const put=vi.spyOn(api,'PUT').mockImplementation(async()=>{vi.mocked(api.GET).mockResolvedValue({data:{...info,join_policy:'con_solicitud'},response:new Response()});return {data:{policy:'con_solicitud'},response:new Response()};});
 const user=userEvent.setup();wrap(<JoinNetwork net="red" canEdit policy="cerrada"/>);
 await screen.findByLabelText('Código de red');
 await user.click(screen.getByRole('radio',{name:'Con solicitud (un administrador aprueba)'}));
 await user.click(screen.getByRole('button',{name:'Permitir solicitudes'}));
 expect(screen.getByRole('dialog')).toHaveTextContent(/Cualquiera con el código/);
 await user.click(screen.getByRole('button',{name:'Cerrar'}));
 expect(put).not.toHaveBeenCalled();
 expect(screen.getByRole('button',{name:'Permitir solicitudes'})).toHaveFocus();
 await user.click(screen.getByRole('button',{name:'Permitir solicitudes'}));
 await user.click(screen.getByRole('button',{name:'Confirmar política con solicitud'}));
 await waitFor(()=>expect(put).toHaveBeenCalledWith('/api/v1/networks/{net}/join-policy',{params:{path:{net:'red'}},body:{policy:'con_solicitud'}}));
 expect(await screen.findByText('Política actualizada: Con solicitud (un administrador aprueba).')).toHaveAttribute('role','status');
 expect(screen.getByRole('radio',{name:'Con solicitud (un administrador aprueba)'})).toHaveFocus();
});
it.each(['ATENEA1-TOKEN-SINTETICO',null])('token conserva join y explica el código %s',async(code)=>{
 vi.spyOn(api,'POST').mockResolvedValue({data:{token:'secreto-sintetico',expires_at:'2026-01-01T01:00:00Z',join:{url:info.url,token:'secreto-sintetico',network_id:device.id,root_anchor:'aa'.repeat(32)},code,root_anchor_fingerprint:info.root_anchor_fingerprint},response:new Response()});
 const user=userEvent.setup();wrap(<Enrollment net="red"/>);
 await user.click(screen.getByRole('button',{name:'Crear token'}));
 await screen.findByRole('button',{name:'Copiar join'});
 if(code){expect(screen.getByLabelText('Código con token')).toHaveValue(code);expect(screen.getByText(/Este código contiene el token y se muestra una sola vez/)).toBeVisible();}
 else expect(screen.getByText(/El nombre de la red o la URL pública no caben/)).toBeVisible();
 expect(screen.getByText(info.root_anchor_fingerprint)).toBeVisible();
 if(code){
   const copy=vi.spyOn(navigator.clipboard,'writeText').mockResolvedValue();
   await user.click(screen.getByRole('button',{name:'Copiar código con token'}));
   expect(copy).toHaveBeenCalledWith(code);
 }
 await user.click(screen.getByRole('button',{name:/Ya lo guardé/}));
 expect(screen.queryByLabelText('Código con token')).not.toBeInTheDocument();
 expect(screen.queryByText(/secreto-sintetico/)).not.toBeInTheDocument();
});
it('marca solo solicitudes pendientes',()=>{
 const {rerender}=render(<Devices devices={[{...device,join_origin:'request'}]} accessRole="member" net="red" elapsed={0} refresh={async()=>{}}/>);
 expect(screen.getByText('Solicitud de unión')).toBeVisible();
 rerender(<Devices devices={[{...device,join_origin:'request',status:'approved'}]} accessRole="member" net="red" elapsed={0} refresh={async()=>{}}/>);
 expect(screen.queryByText('Solicitud de unión')).not.toBeInTheDocument();
});
it('cerrar la política actualiza el estado y devuelve el foco al selector',async()=>{
 vi.spyOn(api,'GET').mockResolvedValue({data:{...info,join_policy:'con_solicitud'},response:new Response()});
 const put=vi.spyOn(api,'PUT').mockImplementation(async()=>{vi.mocked(api.GET).mockResolvedValue({data:info,response:new Response()});return {data:{policy:'cerrada'},response:new Response()};});
 const user=userEvent.setup();wrap(<JoinNetwork net="red" canEdit policy="con_solicitud"/>);
 await screen.findByLabelText('Código de red');
 await user.click(screen.getByRole('radio',{name:'Cerrada (solo con token)'}));
 await user.click(screen.getByRole('button',{name:'Cerrar unión: solo con token'}));
 expect(put).not.toHaveBeenCalled();
 await user.click(screen.getByRole('button',{name:'Confirmar política cerrada'}));
 expect(await screen.findByText('Política actualizada: Cerrada (solo con token).')).toHaveAttribute('role','status');
 expect(screen.getByRole('radio',{name:'Cerrada (solo con token)'})).toHaveFocus();
 expect(put).toHaveBeenCalledWith('/api/v1/networks/{net}/join-policy',{params:{path:{net:'red'}},body:{policy:'cerrada'}});
});
it('un rechazo de permisos no rompe la sección',async()=>{
 vi.spyOn(api,'GET').mockRejectedValue(new ApiError(403,{error:{code:'forbidden',message:'forbidden'}}));
 wrap(<JoinNetwork net="red" canEdit policy="cerrada"/>);
 expect(await screen.findByRole('alert')).toHaveTextContent('No tienes permiso para esta acción.');
 expect(screen.getByRole('heading',{name:'Código de la red'})).toBeVisible();
 expect(screen.queryByLabelText('Código de red')).not.toBeInTheDocument();
});
it('si el portapapeles falla permite copiar manualmente',async()=>{
 vi.spyOn(api,'GET').mockResolvedValue({data:info,response:new Response()});
 const user=userEvent.setup();vi.spyOn(navigator.clipboard,'writeText').mockRejectedValue(new Error('denegado'));
 wrap(<JoinNetwork net="red" canEdit policy="cerrada"/>);
 await screen.findByLabelText('Código de red');
 await user.click(screen.getByRole('button',{name:'Copiar código de red'}));
 expect(await screen.findByText(/No se pudo copiar/)).toHaveAttribute('role','status');
 expect(screen.getByLabelText('Código de red')).toHaveValue(info.code);
});

it('asocia el error de duración al campo y explica la autoaprobación', async () => {
 const post = vi.spyOn(api, 'POST');
 const user = userEvent.setup();
 const { container } = wrap(<Enrollment net="red"/>);
 const ttl = screen.getByLabelText('Validez (segundos, 60–86400)');
 await user.clear(ttl); await user.type(ttl, '59');
 await user.click(screen.getByRole('button', { name: 'Crear token' }));
 expect(ttl).toHaveAttribute('aria-invalid', 'true');
 expect(ttl).toHaveAccessibleDescription(/número entero entre 60 y 86400/);
 expect(post).not.toHaveBeenCalled();
 expect(screen.getByRole('checkbox')).toHaveAccessibleDescription(/entrará sin aprobación manual/);
 expect((await axe(container, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
});
