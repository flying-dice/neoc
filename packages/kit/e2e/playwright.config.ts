/**
 * Only the kit's own specs, with no host and no Storybook: `bun run test:e2e` in this folder (the kit must be built:
 * `bun run build`). The repository's `bun run test:e2e` runs the same specs in its `journeys` project.
 */
import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.ts',
  reporter: [['list']],
  use: {
    colorScheme: 'light',
    reducedMotion: 'reduce',
    locale: 'en-GB',
    timezoneId: 'Europe/London'
  }
})
