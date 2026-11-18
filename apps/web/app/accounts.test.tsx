// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
// ADR-028, fase 3: cuentas con correo y contraseña en la pantalla de acceso.
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { axe } from 'vitest-axe';
import { afterEach, expect, it, vi } from 'vitest';
import { LocalAccess, RecoveryCodes } from './accounts';

afterEach(() => vi.restoreAllMocks());

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const fail = (code: string, status: number) => json({ error: { code, message: code } }, status);

it('entrar envía el correo y la contraseña sin pasar por el cliente con sesión', async () => {
  const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(null, { status: 204 }));
  const done = vi.fn();
  const user = userEvent.setup();
  render(<LocalAccess onDone={done}/>);
  await user.type(screen.getByLabelText('Correo'), 'ana@example.test');
  await user.type(screen.getByLabelText('Contraseña'), 'una contraseña larga');
  await user.click(screen.getByRole('button', { name: 'Entrar' }));
  expect(fetchMock).toHaveBeenCalledWith('/auth/local/login', expect.objectContaining({
    method: 'POST', body: JSON.stringify({ login: 'ana@example.test', password: 'una contraseña larga' }),
  }));
  expect(done).toHaveBeenCalled();
});

it.each([
  ['invalid_credentials', 401, 'Correo o contraseña incorrectos.'],
  ['rate_limited', 429, 'Demasiados intentos. Espera unos minutos y vuelve a probar.'],
])('un error %s se explica sin mostrar el código', async (code, status, message) => {
  vi.spyOn(globalThis, 'fetch').mockResolvedValue(fail(code, status));
  const done = vi.fn();
  const user = userEvent.setup();
  render(<LocalAccess onDone={done}/>);
  await user.type(screen.getByLabelText('Correo'), 'ana@example.test');
  await user.type(screen.getByLabelText('Contraseña'), 'una contraseña larga');
  await user.click(screen.getByRole('button', { name: 'Entrar' }));
  expect(await screen.findByText(message)).toBeVisible();
  expect(done).not.toHaveBeenCalled();
});

it('al crear la cuenta muestra los diez códigos y solo deja continuar cuando se confirman', async () => {
  const codes = Array.from({ length: 10 }, (_, index) => `codigo-sintetico-${index}`);
  vi.spyOn(globalThis, 'fetch').mockResolvedValue(json({ user_id: 'u', recovery_codes: codes }, 201));
  const done = vi.fn();
  const user = userEvent.setup();
  render(<LocalAccess onDone={done}/>);
  await user.click(screen.getByRole('button', { name: 'Crear una cuenta' }));
  await user.type(screen.getByLabelText('Correo'), 'ana@example.test');
  await user.type(screen.getByLabelText('Contraseña'), 'una contraseña larga');
  await user.click(screen.getByRole('button', { name: 'Crear cuenta' }));
  expect(await screen.findByText('codigo-sintetico-9')).toBeVisible();
  expect(screen.getAllByRole('listitem')).toHaveLength(10);
  expect(screen.getByText(/No se volverán a mostrar/)).toBeVisible();
  const next = screen.getByRole('button', { name: 'Continuar' });
  expect(next).toBeDisabled();
  await user.click(screen.getByRole('checkbox'));
  await user.click(next);
  expect(done).toHaveBeenCalledOnce();
});

it('registro cerrado y correo repetido tienen mensajes propios', async () => {
  const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(fail('registration_closed', 403));
  const user = userEvent.setup();
  render(<LocalAccess onDone={() => {}}/>);
  await user.click(screen.getByRole('button', { name: 'Crear una cuenta' }));
  await user.type(screen.getByLabelText('Correo'), 'ana@example.test');
  await user.type(screen.getByLabelText('Contraseña'), 'una contraseña larga');
  await user.click(screen.getByRole('button', { name: 'Crear cuenta' }));
  expect(await screen.findByText('El registro de cuentas está cerrado en este servidor.')).toBeVisible();
  fetchMock.mockResolvedValue(fail('conflict', 409));
  await user.click(screen.getByRole('button', { name: 'Crear cuenta' }));
  expect(await screen.findByText('Ya existe una cuenta con ese correo.')).toBeVisible();
});

it('recuperar cambia la contraseña con un código y vuelve a la pantalla de entrada', async () => {
  const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(null, { status: 204 }));
  const user = userEvent.setup();
  render(<LocalAccess onDone={() => {}}/>);
  await user.click(screen.getByRole('button', { name: 'He olvidado la contraseña' }));
  await user.type(screen.getByLabelText('Correo'), 'ana@example.test');
  await user.type(screen.getByLabelText('Código de recuperación'), 'codigo-sintetico-1');
  await user.type(screen.getByLabelText('Contraseña nueva'), 'contraseña nueva larga');
  await user.click(screen.getByRole('button', { name: 'Cambiar contraseña' }));
  expect(fetchMock).toHaveBeenCalledWith('/auth/local/recover', expect.objectContaining({
    body: JSON.stringify({ login: 'ana@example.test', recovery_code: 'codigo-sintetico-1', new_password: 'contraseña nueva larga' }),
  }));
  expect(await screen.findByText(/Se han cerrado las demás sesiones/)).toBeVisible();
  expect(screen.getByRole('button', { name: 'Entrar' })).toBeVisible();
});

it('los formularios y los códigos no tienen infracciones de accesibilidad', async () => {
  const { container, unmount } = render(<LocalAccess onDone={() => {}}/>);
  expect((await axe(container, { rules: { region: { enabled: false } } })).violations).toEqual([]);
  unmount();
  const codes = render(<RecoveryCodes codes={['a', 'b']} done={() => {}}/>);
  expect((await axe(codes.container, { rules: { region: { enabled: false } } })).violations).toEqual([]);
});
