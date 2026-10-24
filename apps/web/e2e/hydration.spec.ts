import { expect, test } from '@playwright/test';

test('el build estático se hidrata con CSP y sin errores de React', async ({ page }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  // Registramos llamadas reales a console.error; un HTTP 401 esperado no es una llamada JS.
  await page.addInitScript(() => {
    const state = window as typeof window & { hydrationConsoleErrors: string[] };
    state.hydrationConsoleErrors = [];
    document.addEventListener('securitypolicyviolation', event => {
      state.hydrationConsoleErrors.push(`CSP: ${event.violatedDirective} (${event.blockedURI})`);
    });
    const original = console.error.bind(console);
    console.error = (...args: unknown[]) => {
      state.hydrationConsoleErrors.push(args.map(value => String(value)).join(' '));
      original(...args);
    };
  });

  const session = page.waitForResponse(response => new URL(response.url()).pathname === '/api/v1/session');
  const documentResponse = await page.goto('/');
  expect(documentResponse?.status()).toBe(200);
  expect(documentResponse?.headers()['content-security-policy']).toContain("default-src 'self'");
  expect((await session).status()).toBe(401);
  await expect(page.getByRole('link', { name: 'Iniciar sesión con OIDC' })).toBeVisible();
  await expect(page.locator('script:not([src])')).toHaveCount(0);
  await page.waitForLoadState('networkidle');

  const consoleErrors = await page.evaluate(() => (window as typeof window & { hydrationConsoleErrors: string[] }).hydrationConsoleErrors);
  expect(pageErrors, 'Errores de página, incluidos React 418/422/423/425').toEqual([]);
  expect(consoleErrors, 'Ninguna llamada a console.error durante el arranque').toEqual([]);
});
