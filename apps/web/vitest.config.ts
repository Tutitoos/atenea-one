import { defineConfig } from 'vitest/config';
export default defineConfig({ test: { environment: 'jsdom', setupFiles: ['./app/test-setup.ts'], include: ['app/**/*.test.{ts,tsx}'] } });
