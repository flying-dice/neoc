/**
 * Headless UI tests for vos.
 *
 *   stories   every story of every Storybook: renders, axe, snapshots, resize
 *   desktop   the real shell on the real host: every app from the Dock, resize, every layout, axe, snapshots
 *   journeys  each app's and the shell's key user journeys (apps/<app>/e2e, client/e2e, e2e/*.journeys.spec.ts)
 *   notifications  the notification journeys (e2e/notifications.journeys.spec.ts): one app raises banners on every open page, so they run alone, last
 *   background  the background-work journeys (e2e/background.journeys.spec.ts): they install an app that every open page hears about, so they run alone, after the journeys
 *
 * The desktop and the journeys run against the real Rust host (`e2e/host/start.ts`: a cargo build, a fresh data
 * directory, a test registry, port 7410, or VOS_PORT_BASE + 10: see scripts/ports.ts). One browser is paired once (`e2e/host/global-setup.ts`) and every spec
 * shares it, except the pairing journeys, which start unpaired.
 */
import { defineConfig } from '@playwright/test'
import { ORIGIN, STATE } from './e2e/host/config'
import { STORYBOOKS_PORT } from './scripts/ports'

/** This checkout. Agent worktrees live under `<root>/.claude/` and carry copies of every spec: skip those. */
const ROOT = import.meta.dirname
const STORYBOOKS = `http://localhost:${STORYBOOKS_PORT}`
/**
 * Where a run keeps its traces and its report. A run on ports of its own (VOS_PORT_BASE) keeps them in folders of its
 * own, as it keeps its host (e2e/host/config.ts): Playwright empties the output folder when a run starts, so two runs
 * that shared one would delete each other's traces.
 */
const RUN = process.env.VOS_PORT_BASE ? `-${process.env.VOS_PORT_BASE}` : ''

export default defineConfig({
  testDir: '.',
  testIgnore: ['**/node_modules/**', new RegExp(`^${ROOT.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}/\\.claude/`)],
  snapshotPathTemplate: '{testFileDir}/__screenshots__/{testFileName}/{arg}-{platform}{ext}',
  outputDir: `test-results/output${RUN}`,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never', outputFolder: `test-results/report${RUN}` }]],
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
      testMatch: ['{apps,packages}/*/e2e/**/*.spec.ts', 'client/e2e/**/*.spec.ts', 'e2e/!(background|notifications).journeys.spec.ts'],
      dependencies: ['desktop'],
      fullyParallel: false,
      use: { baseURL: ORIGIN, storageState: STATE }
    },
    {
      // Installs and removes an app on the shared host, which every open page hears about (the Dock changes under
      // the other journeys): it runs alone, after them.
      name: 'background',
      testMatch: 'e2e/background.journeys.spec.ts',
      dependencies: ['journeys'],
      fullyParallel: false,
      use: { baseURL: ORIGIN, storageState: STATE }
    },
    {
      // A notification reaches every open page, and its sender is installed for the run: this runs alone, last.
      name: 'notifications',
      testMatch: 'e2e/notifications.journeys.spec.ts',
      dependencies: ['background'],
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
