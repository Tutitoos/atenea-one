// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./theme.css', import.meta.url), 'utf8');

function tokens(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/--color-([\w-]+):\s*(#[0-9a-fA-F]{6})/g)].map((m) => [m[1], m[2]]));
}
const dark = tokens(css.slice(0, css.indexOf(':root[data-theme=claro]')));
const light = { ...dark, ...tokens(css.slice(css.indexOf(':root[data-theme=claro]'))) };

const lum = (hex: string) => {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
};
const contrast = (a: string, b: string) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

describe.each([['oscuro', dark], ['claro', light]] as const)('pares de texto cumplen AA en %s', (_, t) => {
  const texts = ['gray-300', 'gray-400', 'accent-200', 'ok', 'no'];
  const grounds = ['shell', 'panel', 'raise'];
  it.each(texts.flatMap((f) => grounds.map((b) => [f, b])))('%s sobre %s', (f, b) => {
    expect(contrast(t[f], t[b])).toBeGreaterThanOrEqual(4.5);
  });
  it('texto del botón primario (shell) sobre accent-200', () => {
    expect(contrast(t.shell, t['accent-200'])).toBeGreaterThanOrEqual(4.5);
  });
});
