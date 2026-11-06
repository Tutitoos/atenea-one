// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Component, type ReactNode } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { ErrorBoundary } from './root';

// El ErrorBoundary de React Router es una función sin estado; esta clase hace de la frontera que lo pinta al lanzar.
class Frontier extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() { return this.state.failed ? <ErrorBoundary /> : this.props.children; }
}

const Boom = (): ReactNode => { throw new Error('fallo de prueba'); };
const original = window.location;
afterEach(() => { Object.defineProperty(window, 'location', { configurable: true, value: original }); vi.restoreAllMocks(); });

it('un fallo al pintar muestra el mensaje en español y «Reintentar» recarga la ventana', async () => {
  vi.spyOn(console, 'error').mockImplementation(() => {});
  const reload = vi.fn();
  // jsdom no deja espiar `location.reload`: se sustituye `location` entero por un objeto con el espía.
  Object.defineProperty(window, 'location', { configurable: true, value: { ...original, reload } });
  render(<Frontier><Boom /></Frontier>);
  expect(screen.getByRole('alert')).toHaveTextContent('Algo ha fallado al mostrar esta pantalla.');
  expect(reload).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole('button', { name: 'Reintentar' }));
  expect(reload).toHaveBeenCalledOnce();
});
