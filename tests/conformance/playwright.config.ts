import { defineConfig, devices } from '@playwright/test';

/**
 * BASE_URL=http://localhost:8002 pnpm test:e2e   -> test a running example
 * pnpm test:e2e                                   -> starts the Node reference server on :8000
 */
const external = process.env.BASE_URL;

export default defineConfig({
  testDir: 'e2e',
  fullyParallel: true,
  reporter: process.env.CI ? 'github' : 'list',
  use: { baseURL: external ?? 'http://localhost:8000' },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    ...(process.env.ALL_BROWSERS
      ? [
          { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
          { name: 'webkit', use: { ...devices['Desktop Safari'] } },
        ]
      : []),
  ],
  webServer: external
    ? undefined
    : { command: 'node reference-server.mjs', url: 'http://localhost:8000/health', reuseExistingServer: true },
});
