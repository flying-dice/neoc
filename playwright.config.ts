/**
 * Headless UI tests for every AgentOS app (see docs/TESTING.md).
 *
 *   stories  every story of every Storybook: renders, axe, snapshots, resize
 *   desktop  the real desktop renderer in web mode: every app, every layout and size
 *   apps     each app's key user journeys (apps/<app>/e2e/*.spec.ts)
 *
 * No Electron and no main process: the renderer runs on the in-memory host.
 */
import { defineConfig } from '@playwright/test'

const STORYBOOKS = 'http://localhost:6100'
const DESKTOP = 'http://localhost:6200'

export default defineConfig({
  testDir: '.',
  // Agent worktrees (.claude/worktrees) carry their own node_modules and copies of every spec.
  testIgnore: ['**/node_modules/**', '.claude/**'],
  snapshotPathTemplate: '{testFileDir}/__screenshots__/{testFileName}/{arg}-{platform}{ext}',
  outputDir: 'test-results/output',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never', outputFolder: 'test-results/report' }]],
  use: {
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    colorScheme: 'light',
    reducedMotion: 'reduce',
    locale: 'en-GB',
    timezoneId: 'Europe/London',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure'
  },
  expect: {
    timeout: 10_000,
    toHaveScreenshot: { maxDiffPixelRatio: 0.002, animations: 'disabled', caret: 'hide', scale: 'css' }
  },
  projects: [
    { name: 'stories', testMatch: 'e2e/stories.spec.ts', use: { baseURL: STORYBOOKS } },
    { name: 'desktop', testMatch: 'e2e/desktop.spec.ts', use: { baseURL: DESKTOP } },
    { name: 'apps', testMatch: ['{apps,packages}/*/e2e/**/*.spec.ts', 'client/e2e/**/*.spec.ts'], use: { baseURL: DESKTOP } }
  ],
  webServer: [
    {
      command: 'bun scripts/storybooks.ts serve',
      url: `${STORYBOOKS}/storybooks.json`,
      reuseExistingServer: !process.env.CI,
      timeout: 600_000
    },
    {
      command: 'bun run --filter @agentos/client build:web && bun run --filter @agentos/client preview:web',
      url: `${DESKTOP}/web.html`,
      reuseExistingServer: !process.env.CI,
      timeout: 180_000
    }
  ]
})
