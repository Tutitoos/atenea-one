// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Links, Meta, Outlet, Scripts } from 'react-router';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import './styles.css';

export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="es">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>Atenea One</title>
        <link rel="icon" type="image/svg+xml" href="/icon.svg" />
        <Meta />
        <Links />
      </head>
      <body>
        <div id="root">{children}</div>
        <Scripts />
      </body>
    </html>
  );
}

export function HydrateFallback() {
  return <p role="status" className="p-6 text-mut">Cargando…</p>;
}

export default function App() {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  return <QueryClientProvider client={client}><Outlet /></QueryClientProvider>;
}
