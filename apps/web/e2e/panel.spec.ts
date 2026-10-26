// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { expect, test, type Locator, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Flujo completo (PLAN.md, fase 6): login → crear red → incorporar dos agentes Linux → transferencia →
// bloqueo UDP → relay TLS → revocación. Los pasos de laboratorio los ejecuta tests/e2e-panel/lab.sh.
const LAB = `${process.env.LAB_ROOT ?? '/src'}/tests/e2e-panel/lab.sh`;
const lab = (...args: string[]) =>
  execFileSync('bash', [LAB, ...args], { encoding: 'utf8', env: process.env }).trim();
const labOk = (...args: string[]) => {
  try { lab(...args); return true; } catch { return false; }
};

const row = (page: Page, name: string): Locator => page.getByRole('row', { name: new RegExp(name) });

async function open(page: Page, view: string) {
  await page.getByRole('navigation', { name: 'Secciones de red' }).getByRole('button', { name: view }).click();
}

async function dismiss(page: Page) {
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
}

async function axeClean(page: Page, label: string) {
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  const bad = result.violations.map(v => `${v.id}: ${v.nodes.length} nodos`);
  expect(bad, `accesibilidad en ${label}`).toEqual([]);
}

test('administrar dos agentes desde el panel: P2P, relay UDP, relay TLS y revocación', async ({ page }) => {
  const rootAnchor = readFileSync('/tmp/lab/root.pub', 'utf8').trim();
  expect(rootAnchor, 'ancla raíz generada offline').toMatch(/^[0-9a-f]{64}$/);

  // --- Inicio de sesión OIDC real (Dex)
  await page.goto('/');
  await expect(page.getByRole('link', { name: 'Iniciar sesión con OIDC' })).toBeVisible();
  await axeClean(page, 'login');
  await page.getByRole('link', { name: 'Iniciar sesión con OIDC' }).click();
  await page.locator('input[name="login"]').fill('admin@example.test');
  await page.locator('input[name="password"]').fill('password');
  await page.locator('button[type="submit"]').click();
  await expect(page.getByRole('button', { name: 'Crear organización' })).toBeVisible();

  // --- Organización y red (la raíz y el emisor son offline: el panel solo recibe el ancla pública)
  await page.getByRole('button', { name: 'Crear organización' }).click();
  await page.getByLabel('Nombre').fill('laboratorio');
  await page.getByRole('dialog').getByRole('button', { name: 'Crear organización' }).click();
  await expect(page.getByRole('dialog').getByRole('status')).toHaveText('Acción completada.');
  await dismiss(page);
  await page.getByRole('combobox', { name: /^Organización/ }).selectOption({ label: 'laboratorio · owner' });
  await page.getByRole('button', { name: 'Crear red' }).click();
  await page.getByLabel('Nombre').fill('red-lab');
  await page.getByLabel('CIDR IPv4').fill('10.77.0.0/24');
  await page.getByLabel('CIDR IPv6').fill('fd77::/64');
  await page.getByLabel('Ancla pública raíz (64 hex)').fill(rootAnchor);
  await page.getByRole('dialog').getByRole('button', { name: 'Crear red' }).click();
  await expect(page.getByRole('heading', { name: 'red-lab' })).toBeVisible();
  const net = new URL(page.url()).searchParams.get('net')!;
  expect(net).toMatch(/^[0-9a-f-]{36}$/);

  // --- Certificado del emisor, generado offline por el laboratorio
  lab('issuer-cert', net);
  await page.getByRole('button', { name: 'Certificado de emisor' }).click();
  await page.getByLabel('Certificado público').setInputFiles('/tmp/lab/issuer.cert');
  await page.getByRole('button', { name: 'Subir issuer.cert' }).click();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Certificado de emisor' })).toBeVisible();

  // --- Incorporar dos dispositivos: token único, enrolamiento por CLI, aprobación comparando huellas
  const joins: Record<string, string> = {};
  const fingerprints: Record<string, string> = {};
  for (const n of ['a', 'b']) {
    fingerprints[n] = /fingerprint=(\S+)/.exec(lab('device-init', n))![1];
    await open(page, 'Incorporar');
    await page.getByRole('button', { name: 'Crear token' }).click();
    const join = await page.locator('pre').filter({ hasText: '"root_anchor"' }).innerText();
    joins[n] = JSON.stringify(JSON.parse(join));
    const { url, token } = JSON.parse(join) as { url: string; token: string };
    lab('enroll', n, url, token);
    await page.getByRole('button', { name: /Ya lo guardé/ }).click();
  }
  await open(page, 'Dispositivos');
  await axeClean(page, 'dispositivos');
  for (const n of ['a', 'b']) {
    const r = row(page, `host-${n}`);
    await expect(r).toContainText('pending');
    await r.getByRole('button', { name: 'Aprobar' }).click();
    await expect(page.getByRole('dialog')).toContainText(fingerprints[n]);
    await page.getByRole('button', { name: 'Confirmar aprobación' }).click();
    await dismiss(page);
    await expect(row(page, `host-${n}`)).toContainText('approved');
  }

  // --- Política: cualquier tráfico entre dispositivos de la red
  await open(page, 'Políticas');
  await page.getByRole('button', { name: 'Crear política' }).click();
  await page.getByRole('button', { name: 'Guardar política' }).click();
  await expect(page.getByRole('dialog').getByRole('status')).toHaveText('Acción completada.');
  await dismiss(page);
  await expect(page.getByText('Cualquiera → Cualquiera')).toBeVisible();

  // --- Agentes: se configuran con el `join` del panel y informan su estado
  lab('start-agent', 'a', joins.a);
  lab('start-agent', 'b', joins.b);
  await open(page, 'Dispositivos');
  // Sin tráfico no hay handshake: se genera un ping por consulta, como haría un usuario.
  await expect(row(page, 'host-b')).toContainText(/10\.77\.0\.\d+/);
  const ipB = (await row(page, 'host-b').innerText()).match(/10\.77\.0\.\d+/)![0];
  const pathOf = async (n: string) => {
    labOk('ping', 'a', ipB);
    return row(page, `host-${n}`).innerText();
  };
  await expect.poll(async () => pathOf('a'), { timeout: 180_000, intervals: [3000] }).toMatch(/direct/);
  await expect.poll(async () => pathOf('b'), { timeout: 60_000, intervals: [3000] }).toMatch(/direct/);
  await expect.poll(() => labOk('ping', 'a', ipB), { timeout: 60_000 }).toBe(true);
  expect(lab('transfer', ipB)).toBe('ok');

  // --- Bloqueo del P2P: relay UDP; bloqueo de todo UDP: relay TLS/TCP 443
  lab('block', 'p2p');
  await expect.poll(async () => pathOf('a'), { timeout: 120_000, intervals: [3000] }).toMatch(/relay-udp/);
  expect(lab('transfer', ipB)).toBe('ok');
  lab('block', 'udp');
  await expect.poll(async () => pathOf('a'), { timeout: 120_000, intervals: [3000] }).toMatch(/relay-tls/);
  expect(lab('transfer', ipB)).toBe('ok');

  // --- Vistas derivadas con datos reales
  await open(page, 'Relays');
  await expect(page.getByRole('heading', { level: 3, name: 'lab', exact: true })).toBeVisible();
  await axeClean(page, 'relays');
  await open(page, 'Topología');
  await axeClean(page, 'topología');
  await open(page, 'Auditoría');
  for (const action of ['device.enroll', 'device.approve', 'policy.create']) {
    await expect(page.getByText(action).first()).toBeVisible();
  }

  // --- Temas y móvil
  await page.getByLabel('Tema').selectOption('oscuro');
  await axeClean(page, 'tema oscuro');
  await page.setViewportSize({ width: 390, height: 800 });
  await open(page, 'Dispositivos');
  await axeClean(page, 'móvil');
  await page.screenshot({ path: '../test-results/movil-dispositivos.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 800 });
  lab('block', 'none');

  // --- Revocación: B deja de comunicarse (lease ≤ 60 s + tolerancia 30 s)
  await open(page, 'Dispositivos');
  await row(page, 'host-b').getByRole('button', { name: 'Revocar' }).click();
  await expect(page.getByRole('dialog')).toContainText(fingerprints.b);
  await page.getByRole('button', { name: 'Confirmar revocación' }).click();
  await dismiss(page);
  await expect(row(page, 'host-b')).toContainText('revoked');
  const t0 = Date.now();
  await expect.poll(() => labOk('ping', 'a', ipB), { timeout: 150_000, intervals: [2000] }).toBe(false);
  console.log(`revocación efectiva en ${Math.round((Date.now() - t0) / 1000)} s`);
});
