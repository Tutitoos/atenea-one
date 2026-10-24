import { expect, it, vi } from 'vitest';
import { ApiError, createApi } from './index';
it('obtiene CSRF en memoria y envía cookies same-origin al escribir',async()=>{
 const seen:Request[]=[];const fetcher=vi.fn(async(request:Request)=>{seen.push(request);return request.url.endsWith('/session')?Response.json({csrf_token:'csrf-sintetico',user:{id:'u',email:null,display_name:null},memberships:[]}):Response.json({id:'org',name:'Organización'});});
 const client=createApi('http://localhost',fetcher as typeof fetch);await client.POST('/api/v1/orgs',{body:{name:'Organización'}});await client.POST('/api/v1/orgs',{body:{name:'Otra'}});
 expect(seen).toHaveLength(3);expect(seen[0].url).toContain('/session');expect(seen[1].headers.get('X-CSRF-Token')).toBe('csrf-sintetico');expect(seen[1].credentials).toBe('same-origin');
});
it('expone el error tipado y caducidad sin ocultar rechazo backend',async()=>{const client=createApi('http://localhost',vi.fn(async()=>Response.json({error:{code:'unauthorized',message:'Caducada'}},{status:401})));await expect(client.GET('/api/v1/session')).rejects.toMatchObject({status:401,detail:{error:{code:'unauthorized'}}});try{await client.GET('/api/v1/session');}catch(e){expect(e).toBeInstanceOf(ApiError);expect((e as Error).message).toContain('caducado');}});
it('traduce autorización y aislamiento sin filtrar recursos',async()=>{for(const status of [403,404]){const client=createApi('http://localhost',vi.fn(async()=>Response.json({error:{code:'denied',message:'Detalle'}},{status})));await expect(client.GET('/api/v1/session')).rejects.toMatchObject({status});}});
