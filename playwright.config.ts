/**
 * Headless UI tests for AgentOS (see docs/TESTING.md).
 *
 *   stories   every story of every Storybook: renders, axe, snapshots, resize
 *   desktop   the real shell on the real host: every app from the Dock, resize, every layout, axe, snapshots
 *   journeys  each app's and the shell's key user journeys (apps/<app>/e2e, client/e2e, e2e/*.journeys.spec.ts)
 *
 * The desktop and the journeys run against the real Rust host (`e2e/host/start.ts`: a cargo build, a fresh data
 * directory, a test registry, port 7410). One browser is paired once (`e2e/host/global-setup.ts`) and every spec
 * shares it, except the pairing journeys, which start unpaired.
 */
import { defineConfig } from '@playwright/test'
import { ORIGIN, STATE } from './e2e/host/config'

/** This checkout. Agent worktrees live under `<root>/.claude/` and carry copies of every spec: skip those. */
const ROOT = import.meta.dirname
const STORYBOOKS = 'http://localhost:6100'

export default defineConfig({
  testDir: '.',
  testIgnore: ['**/node_modules/**', new RegExp(`^${ROOT.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}/\\.claude/`)],
  snapshotPathTemplate: '{testFileDir}/__screenshots__/{testFileName}/{arg}-{platform}{ext}',
  outputDir: 'test-results/output',
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never', outputFolder: 'test-results/report' }]],
  globalSetup: './e2e/host/global-setup.ts',
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
    {
      name: 'stories',
      testMatch: 'e2e/stories.spec.ts',
      fullyParallel: true,
      use: { baseURL: STORYBOOKS }
    },
    {
      // One host, one Calculator, one set of windows: these run one after another.
      name: 'desktop',
      testMatch: 'e2e/desktop.spec.ts',
      fullyParallel: false,
      use: { baseURL: ORIGIN, storageState: STATE }
    },
    {
      name: 'journeys',
      testMatch: ['{apps,packages}/*/e2e/**/*.spec.ts', 'client/e2e/**/*.spec.ts', 'e2e/*.journeys.spec.ts'],
      dependencies: ['desktop'],
      fullyParallel: false,
      use: { baseURL: ORIGIN, storageState: STATE }
    }
  ],
  webServer: [
    {
      command: 'bun scripts/storybooks.ts serve',
      url: `${STORYBOOKS}/storybooks.json`,
      reuseExistingServer: !process.env.CI,
      timeout: 600_000
    },
    {
      command: 'bun e2e/host/start.ts',
      url: `${ORIGIN}/session`,
      reuseExistingServer: false,
      timeout: 600_000,
      gracefulShutdown: { signal: 'SIGTERM', timeout: 10_000 }
    }
  ]
})
