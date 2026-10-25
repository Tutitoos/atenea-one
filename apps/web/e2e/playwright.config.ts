import { defineConfig } from '@playwright/test';

// E2E del panel: hidratación y dos escenarios contra servicios reales compartidos.
// Cada escenario crea su organización/red y usa agentes distintos; requiere contenedores privilegiados.
export default defineConfig({
  globalSetup: './global-setup.ts',
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
    actionTimeout: 15_000,
    navigationTimeout: 30_000,
    launchOptions: { args: ['--no-sandbox'] },
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    locale: 'es-ES',
  },
});
