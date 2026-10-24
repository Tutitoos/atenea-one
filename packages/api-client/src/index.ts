import createClient from 'openapi-fetch';
import type { paths, components } from './schema.js';
export type { paths, components } from './schema.js';
export type Model<K extends keyof components['schemas']> = components['schemas'][K];
export class ApiError extends Error {
  constructor(public status: number, public detail: Model<'Error'>) {
    super(status === 401 ? 'Tu sesión ha caducado. Inicia sesión de nuevo.' : status === 403 ? 'No tienes permiso para esta acción.' : status === 404 ? 'El recurso no existe o no tienes acceso.' : detail.error.message);
  }
}
export function createApi(baseUrl = '', fetcher: typeof fetch = fetch) {
  let csrf: string | undefined;
  const client = createClient<paths>({ baseUrl, credentials: 'same-origin', fetch: fetcher });
  client.use({
    async onRequest({ request }) {
      if (!['GET', 'HEAD', 'OPTIONS'].includes(request.method)) {
        if (!csrf) {
          const { data } = await client.GET('/api/v1/session');
          csrf = data?.csrf_token;
        }
        if (!csrf) throw new Error('No se pudo obtener la sesión.');
        request.headers.set('X-CSRF-Token', csrf);
      }
      return request;
    },
    async onResponse({ response, schemaPath }) {
      if (response.status === 401) csrf = undefined;
      if (!response.ok) {
        const detail = await response.clone().json() as Model<'Error'>;
        throw new ApiError(response.status, detail);
      }
      if (schemaPath === '/api/v1/session') {
        const session = await response.clone().json() as Model<'Session'>;
        csrf = session.csrf_token;
      }
      return response;
    },
  });
  return client;
}
export const api = createApi();
export async function result<T>(request: Promise<{ data?: T }>): Promise<T> {
  const { data } = await request;
  if (data === undefined) throw new Error('Respuesta vacía inesperada.');
  return data;
}
