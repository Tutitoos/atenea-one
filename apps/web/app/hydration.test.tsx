// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { act } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { hydrateRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { HydrateFallback, Layout } from './root';

it('hidrata #app sin incluir el documento ni los scripts de arranque', async () => {
  const container = document.createElement('div');
  container.id = 'app';
  container.innerHTML = renderToString(<HydrateFallback/>);
  document.body.append(container);
  const errors: unknown[] = [];
  const consoleError = vi.spyOn(console, 'error').mockImplementation((...args: unknown[]) => errors.push(args));
  try {
    const root = hydrateRoot(container, <Layout><HydrateFallback/></Layout>, {
      onRecoverableError: error => errors.push(error),
      onUncaughtError: error => errors.push(error),
      onCaughtError: error => errors.push(error),
    });
    try {
      await act(async () => {});
      expect(container.querySelector('main h1')?.textContent).toBe('Atenea One');
      expect(container.querySelector('[role="status"]')?.textContent).toBe('Cargando el panel…');
      expect(container.querySelector('html, head, body, script')).toBeNull();
      expect(errors).toEqual([]);
    } finally {
      await act(async () => root.unmount());
    }
  } finally {
    consoleError.mockRestore();
    container.remove();
  }
});
