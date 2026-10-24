import { defineConfig } from '@playwright/test';

// E2E del panel (ver tests/e2e-panel/run.sh): un único escenario largo y ordenado contra el
// control plane real, Dex y agentes Linux; no se ejecuta en CI (necesita contenedores privilegiados).
export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.ts',
  timeout: 15 * 60_000,
  expect: { timeout: 10_000 },
  workers: 1,
  fullyParallel: false,
  reporter: [['list']],
  outputDir: '../test-results',
  use: {
    baseURL: 'http://198.51.100.254:8080',
    launchOptions: { args: ['--no-sandbox'] },
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    locale: 'es-ES',
  },
});
