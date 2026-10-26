// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { startTransition, StrictMode } from 'react';
import { hydrateRoot } from 'react-dom/client';
import { HydratedRouter } from 'react-router/dom';

const container = document.getElementById('app');
if (!container) throw new Error('No se encuentra la frontera de hidratación del panel.');

startTransition(() => {
  hydrateRoot(container, <StrictMode><HydratedRouter/></StrictMode>);
});
