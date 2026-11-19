// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import '@testing-library/jest-dom/vitest';
import { afterEach } from 'vitest';
import { cleanup, screen } from '@testing-library/react';
import type { UserEvent } from '@testing-library/user-event';
afterEach(() => cleanup());
// jsdom no implementa lo que Radix Select usa al abrir la lista.
Element.prototype.hasPointerCapture = () => false;
Element.prototype.setPointerCapture = () => {};
Element.prototype.releasePointerCapture = () => {};
Element.prototype.scrollIntoView = () => {};
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} };
/** Elige por valor en el `Select` del panel, como `user.selectOptions` en un select nativo. */
export async function choose(user: UserEvent, trigger: HTMLElement, value: string) {
  await user.click(trigger);
  await user.click(screen.getAllByRole('option').find(option => option.dataset.value === value)!);
}
