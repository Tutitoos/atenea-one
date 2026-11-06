// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { axe } from 'vitest-axe';
import { expect, it, vi } from 'vitest';
import { useShortcuts, type ShortcutActions } from './lib/shortcuts';
import { ShortcutsHelp } from './shortcuts-help';

const actions = (): ShortcutActions => ({ search: vi.fn(), network: vi.fn(), refresh: vi.fn(), help: vi.fn() });
function Host({ a, isMac = true }: { a: ShortcutActions; isMac?: boolean }) {
  useShortcuts(a, isMac);
  return <input aria-label="Buscar" />;
}
const press = (init: KeyboardEventInit, target: Element | Window = window) => fireEvent.keyDown(target, init);

it('⌘F, ⌘N y ⌘R llaman a su acción y evitan el comportamiento por defecto', () => {
  const a = actions();
  render(<Host a={a} />);
  expect(fireEvent.keyDown(window, { key: 'f', metaKey: true })).toBe(false);
  press({ key: '3', metaKey: true });
  press({ key: 'r', metaKey: true });
  expect(a.search).toHaveBeenCalledOnce();
  expect(a.network).toHaveBeenCalledWith(3);
  expect(a.refresh).toHaveBeenCalledOnce();
});

it('fuera de macOS el modificador es Ctrl y ⌘ no vale', () => {
  const a = actions();
  render(<Host a={a} isMac={false} />);
  press({ key: 'r', metaKey: true });
  expect(a.refresh).not.toHaveBeenCalled();
  press({ key: 'r', ctrlKey: true });
  expect(a.refresh).toHaveBeenCalledOnce();
});

it('con el foco en un campo solo se atiende buscar y no se bloquea la tecla', () => {
  const a = actions();
  render(<Host a={a} />);
  const input = screen.getByLabelText('Buscar');
  expect(fireEvent.keyDown(input, { key: '?' })).toBe(true);
  expect(fireEvent.keyDown(input, { key: '2', metaKey: true })).toBe(true);
  press({ key: 'r', metaKey: true }, input);
  expect(a.help).not.toHaveBeenCalled();
  expect(a.network).not.toHaveBeenCalled();
  expect(a.refresh).not.toHaveBeenCalled();
  press({ key: 'f', metaKey: true }, input);
  expect(a.search).toHaveBeenCalledOnce();
});

it('un atajo no atendido no se bloquea', () => {
  render(<Host a={actions()} />);
  expect(fireEvent.keyDown(window, { key: 'q', metaKey: true })).toBe(true);
});

function Help() {
  const [open, setOpen] = useState(false);
  useShortcuts({ ...actions(), help: () => setOpen(true) }, true);
  return <><button>Antes</button>{open && <ShortcutsHelp isMac onClose={() => setOpen(false)} />}</>;
}

it('? abre la ayuda, el foco queda dentro y Escape la cierra devolviéndolo', async () => {
  render(<Help />);
  const before = screen.getByRole('button', { name: 'Antes' });
  before.focus();
  press({ key: '?' }, before);
  const dialog = await screen.findByRole('dialog', { name: 'Atajos de teclado' });
  expect(dialog).toHaveTextContent('⌘F');
  expect(dialog).toContainElement(document.activeElement as HTMLElement);
  expect((await axe(document.body, { rules: { region: { enabled: false }, 'color-contrast': { enabled: false } } })).violations).toEqual([]);
  await userEvent.tab();
  expect(dialog).toContainElement(document.activeElement as HTMLElement);
  await userEvent.keyboard('{Escape}');
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(before).toHaveFocus();
});
