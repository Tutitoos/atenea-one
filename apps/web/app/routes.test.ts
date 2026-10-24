import { expect, it } from 'vitest';
import { matchRoutes } from 'react-router';
import routeConfig from './routes';

it('carga el panel en / y en las rutas de fallback SPA', () => {
  const routes = routeConfig.map(route => route.index
    ? { id: route.id, index: true as const }
    : { id: route.id, path: route.path });
  expect(matchRoutes(routes, '/')?.at(-1)?.route.id).toBe('panel-index');
  expect(matchRoutes(routes, '/red-de-prueba')?.at(-1)?.route.id).toBe('panel');
});
