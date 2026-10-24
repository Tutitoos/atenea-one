import { Links, Meta, Outlet, Scripts } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import './styles.css';
export function Layout({ children }: { children: React.ReactNode }) {
  // El documento y el arranque externo quedan fuera de la frontera hidratada.
  if (typeof document !== 'undefined') return children;
  return <html lang="es"><head><meta charSet="utf-8"/><meta name="viewport" content="width=device-width, initial-scale=1"/><title>Atenea One · Red privada</title><Meta/><Links/></head><body><div id="app">{children}</div><Scripts/></body></html>;
}
export function HydrateFallback() { return <main><h1>Atenea One</h1><p role="status">Cargando el panel…</p></main>; }
export default function App() { const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false, refetchInterval: 30_000 }, mutations: { retry: false } } })); return <QueryClientProvider client={client}><Outlet/></QueryClientProvider>; }
