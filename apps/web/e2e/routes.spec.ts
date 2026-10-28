// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const LAB = `${process.env.LAB_ROOT ?? '/src'}/tests/e2e-panel/lab.sh`;
const lab = (...args: string[]) => execFileSync('bash', [LAB, ...args], { encoding: 'utf8', env: process.env }).trim();
const labOk = (...args: string[]) => { try { lab(...args); return true; } catch { return false; } };
const device = (page: Page, name: string) => page.getByRole('row').filter({
  has: page.getByRole('button', { name: `Ver detalle de host-${name}`, exact: true }),
});
const detail = (page: Page, name: string) => page.getByRole('region', { name: `Detalle de host-${name}`, exact: true });
const open = (page: Page, view: string) => page.getByRole('navigation', { name: 'Secciones de red' })
  .getByRole('link', { name: view === 'Dispositivos' ? /^Dispositivos/ : view, exact: view !== 'Dispositivos' }).click();
async function dismiss(page: Page) {
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
}
async function axeClean(page: Page, label: string) {
  // Axe calcula colores compuestos: espera a que termine el fade del diálogo.
  const dialog = page.getByRole('dialog');
  if (await dialog.count()) await expect(dialog).toHaveCSS('opacity', '1');
  const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(result.violations.map(v => `${v.id}: ${v.nodes.length} nodos`), `accesibilidad en ${label}`).toEqual([]);
}

test('aprobar con SNAT explícito, configurar DNS dividido y revocar una ruta declarada por Linux', async ({ page }) => {
  const started = Date.now();
  lab('routes-setup');
  await page.goto('/');
  await page.getByRole('link', { name: 'Iniciar sesión con OIDC' }).click();
  await page.locator('input[name="login"]').fill('admin@example.test');
  await page.locator('input[name="password"]').fill('password');
  await page.locator('button[type="submit"]').click();
  await page.getByRole('button', { name: 'Crear organización' }).click();
  await page.getByLabel('Nombre').fill('laboratorio-subredes');
  await page.getByRole('dialog').getByRole('button', { name: 'Crear organización' }).click();
  await expect(page.getByRole('dialog').getByRole('status')).toHaveText('Acción completada.');
  await dismiss(page);
  await page.getByRole('combobox', { name: /^Organización/ }).selectOption({ label: 'laboratorio-subredes · owner' });
  await page.getByRole('button', { name: 'Crear red' }).click();
  await page.getByLabel('Nombre').fill('red-subredes');
  await page.getByLabel('CIDR IPv4').fill('10.78.0.0/24');
  await page.getByLabel('CIDR IPv6').fill('fd78::/64');
  await page.getByLabel('Ancla pública raíz (64 hex)').fill(readFileSync('/tmp/lab/root.pub', 'utf8').trim());
  await page.getByRole('dialog').getByRole('button', { name: 'Crear red' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'red-subredes · Resumen' })).toBeVisible();
  const net = new URL(page.url()).searchParams.get('net')!;
  lab('issuer-cert', net);
  await open(page, 'DNS y certificado');
  await page.getByRole('button', { name: 'Subir certificado', exact: true }).click();
  await page.getByLabel('Certificado público').setInputFiles('/tmp/lab/issuer.cert');
  await page.getByRole('button', { name: 'Subir issuer.cert' }).click();
  await expect(page.getByRole('dialog').getByRole('status')).toHaveText('Certificado instalado.');
  await dismiss(page);

  const joins: Record<string, string> = {};
  for (const name of ['r', 'c']) {
    const fingerprint = /fingerprint=(\S+)/.exec(lab('device-init', name))![1];
    await open(page, 'Incorporar');
    await page.getByRole('button', { name: 'Crear token' }).click();
    const join = await page.locator('pre').filter({ hasText: '"root_anchor"' }).innerText();
    joins[name] = JSON.stringify(JSON.parse(join));
    const { url, token } = JSON.parse(join) as { url: string; token: string };
    lab('enroll', name, url, token);
    await page.getByRole('button', { name: /Ya lo guardé/ }).click();
    await open(page, 'Dispositivos');
    await expect(device(page, name)).toContainText('Pendiente');
    await page.getByRole('listitem').filter({ has: page.getByRole('heading', { name: `host-${name}`, exact: true }) })
      .getByRole('button', { name: 'Aprobar', exact: true }).click();
    await expect(page.getByRole('dialog')).toContainText(fingerprint);
    await page.getByRole('button', { name: 'Confirmar aprobación' }).click();
    await dismiss(page);
    await expect(device(page, name)).toContainText('Aprobado');
  }
  lab('start-agent', 'r', joins.r);
  lab('start-agent', 'c', joins.c);
  await open(page, 'Subredes');
  const route = page.getByRole('row', { name: /192\.168\.50\.0\/24/ });
  await expect(route).toContainText('Pendiente', { timeout: 60_000 });
  await expect(route).toContainText('Pendiente de decidir');
  console.log('Panel: ruta declarada por host-r pendiente, SNAT sin decidir.');
  await axeClean(page, 'Subredes');
  await route.getByRole('button', { name: 'Aprobar', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Aprobar ruta', exact: true })).toBeDisabled();
  await expect(page.getByRole('radio', { name: /^Con SNAT/ })).not.toBeChecked();
  await expect(page.getByRole('radio', { name: /^Sin SNAT/ })).not.toBeChecked();
  await axeClean(page, 'diálogo de aprobación');
  await page.getByRole('radio', { name: /^Con SNAT/ }).check();
  await page.getByRole('button', { name: 'Aprobar ruta', exact: true }).click();
  await expect(page.getByText('Ruta aprobada.')).toBeVisible();
  await dismiss(page);
  await expect(route).toContainText('Aprobada');
  await expect(route).toContainText('Aplicado');

  // Los prefijos solo pueden seleccionarse una vez aprobados; la ACL no es implícita.
  await open(page, 'Políticas');
  for (const proto of ['1', '6']) {
    for (const direction of ['destino', 'origen']) {
      await page.getByRole('button', { name: 'Crear política' }).click();
      await page.getByLabel(`Selector ${direction}`).selectOption('prefix');
      await page.getByRole('textbox', { name: `Prefijo de ${direction}`, exact: true }).fill('192.168.50.0/24');
      await page.getByRole('combobox', { name: 'Protocolo', exact: true }).selectOption(proto);
      if (proto === '6' && direction === 'destino') {
        await page.getByLabel('Puerto mínimo').fill('9000');
        await page.getByLabel('Puerto máximo').fill('9100');
      }
      await page.getByRole('button', { name: 'Revisar política' }).click();
      await page.getByRole('button', { name: 'Confirmar creación' }).click();
      await expect(page.getByRole('dialog').getByRole('status')).toHaveText('Política creada.');
      await dismiss(page);
    }
  }
  await open(page, 'Dispositivos');
  for (const name of ['r', 'c']) {
    await device(page, name).getByRole('button', { name: `Ver detalle de host-${name}` }).click();
  }
  await expect(detail(page, 'r')).toContainText('Forwarding: correcto', { timeout: 90_000 });
  await expect(detail(page, 'c')).toContainText('192.168.50.0/24: instalada', { timeout: 90_000 });
  expect(lab('subnet-route')).toContain('dev atenea0');
  await expect.poll(() => labOk('ping', 'c', '192.168.50.20'), { timeout: 120_000, intervals: [2000] }).toBe(true);
  const transfer = JSON.parse(lab('subnet-transfer')) as { bytes: number; source: string; sha256: string };
  expect(transfer.bytes).toBe(1_000_000);
  expect(transfer.source).toBe('192.168.50.1');
  console.log(`Panel: router forwarding correcto y cliente instalada; ping y TCP ${transfer.bytes} bytes íntegros, origen en h=${transfer.source}.`);

  await open(page, 'DNS y certificado');
  await page.getByLabel('Servidores DNS (separados por espacios)').fill('192.168.50.1');
  await page.getByLabel('Dominios de enrutamiento (separados por espacios)').fill('lab.internal');
  await page.getByRole('button', { name: 'Guardar DNS' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'DNS actualizado.' })).toHaveText('DNS actualizado.');
  await expect.poll(() => labOk('dns-log', 'c') ? lab('dns-log', 'c') : '', { timeout: 90_000 }).toContain('domain atenea0 ~lab.internal');
  const dns = lab('dns-log', 'c');
  expect(dns).toContain('dns atenea0 192.168.50.1');
  expect(dns).not.toMatch(/~\.(?:\s|$)/);
  await open(page, 'Dispositivos');
  await device(page, 'c').getByRole('button', { name: 'Ver detalle de host-c' }).click();
  await expect(detail(page, 'c')).toContainText('DNS: applied', { timeout: 60_000 });
  console.log(`Panel: DNS aplicado; resolvectl cliente:\n${dns}`);

  await open(page, 'Subredes');
  await route.getByRole('button', { name: 'Revocar', exact: true }).click();
  await page.getByRole('button', { name: 'Confirmar revocación' }).click();
  await dismiss(page);
  await expect(route).toContainText('Rechazada');
  const revoked = Date.now();
  await expect.poll(() => lab('subnet-route'), { timeout: 90_000, intervals: [2000] }).toBe('');
  await expect.poll(() => labOk('ping', 'c', '192.168.50.20'), { timeout: 30_000 }).toBe(false);
  await open(page, 'Dispositivos');
  await device(page, 'c').getByRole('button', { name: 'Ver detalle de host-c' }).click();
  await expect(detail(page, 'c')).not.toContainText('192.168.50.0/24', { timeout: 60_000 });
  console.log(`Panel: ruta rechazada y retirada del informe cliente; sin ping, revocación ${Math.round((Date.now() - revoked) / 1000)} s; escenario ${Math.round((Date.now() - started) / 1000)} s.`);
});
