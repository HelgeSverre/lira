import { defineConfig, devices } from '@playwright/test';

// Alt-port config so the fiber e2e can run without colliding with a dev server
// already on :3001. Backend -> :3099, frontend -> :5199.
export default defineConfig({
  testDir: './e2e',
  testMatch: 'playground-fibers.spec.ts',
  fullyParallel: false,
  reporter: [['list']],
  use: {
    baseURL: 'http://localhost:5199',
    trace: 'off',
    screenshot: 'only-on-failure',
  },
  // Use the system-installed Google Chrome (channel) to avoid a slow Playwright
  // browser download on this machine.
  projects: [{ name: 'chrome', use: { ...devices['Desktop Chrome'], channel: 'chrome' } }],
  webServer: [
    {
      command: 'VITE_API_URL=http://localhost:3099 npm run dev -- --port 5199 --strictPort',
      url: 'http://localhost:5199',
      reuseExistingServer: true,
      timeout: 120 * 1000,
    },
    {
      command: 'cd ../.. && PORT=3099 cargo run --package lira-playground --release',
      url: 'http://localhost:3099/health',
      reuseExistingServer: true,
      timeout: 180 * 1000,
      env: { PORT: '3099' },
    },
  ],
  timeout: 60 * 1000,
  expect: { timeout: 10 * 1000 },
});
