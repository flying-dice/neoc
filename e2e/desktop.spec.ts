/**
 * Every app, in the real desktop renderer (web mode, in-memory host):
 *  • opens from the Dock,
 *  • floating at its preferred size, light and dark: axe + layout + snapshot,
 *  • resized to its minimum and to a large window,
 *  • full screen, Split View and Slide Over,
 *  • on an iPad-sized screen and a large one.
 *
 * The app list comes from the desktop's registries, so a newly installed app
 * is covered with no extra code.
 */
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import {
  chooseLayout,
  expect,
  expectAccessible,
  expectNoOverflow,
  openDesktop,
  resizeWindow,
  settle,
  test,
  windowOf
} from './fixtures'
import type { Page } from '@playwright/test'

const ROOT = join(import.meta.dirname, '..')

/** Built-in app ids, read from the `builtinApps` registry in apps.svelte.ts. */
function builtinAppIds(): string[] {
  const source = readFileSync(join(ROOT, 'apps/desktop/src/renderer/src/lib/apps.svelte.ts'), 'utf8')
  const block = source.slice(source.indexOf('const builtinApps'), source.indexOf('const toDefinition'))
  return [...block.matchAll(/^ {2}([a-z][a-z0-9-]*): \{/gm)].map((m) => m[1])
}

/** Repo app modules (apps/<dir>/src/manifest.ts): web mode installs every one of them. */
function repoAppIds(): string[] {
  return readdirSync(join(ROOT, 'apps'))
    .map((dir) => join(ROOT, 'apps', dir, 'src/manifest.ts'))
    .filter((file) => existsSync(file))
    .map((file) => readFileSync(file, 'utf8').match(/\bid:\s*'([a-z][a-z0-9-]*)'/)![1])
}

/** Layout is checked inside the window's content: the resize corner deliberately overhangs the frame. */
const contentOf = (page: Page, id: string) => page.getByTestId(`window-${id}-content`)

const APPS = [...builtinAppIds(), ...repoAppIds()]

test('the desktop renders with every app in the Dock', async ({ page }, testInfo) => {
  await openDesktop(page)
  for (const id of APPS) await expect(page.getByTestId(`dock-${id}`)).toBeVisible()
  await expectAccessible(page, testInfo)
  await expect(page).toHaveScreenshot('desktop-landing.png')
})

for (const id of APPS) {
  test.describe(`app: ${id}`, () => {
    test('opens from the Dock', async ({ page }) => {
      await openDesktop(page)
      await page.getByTestId(`dock-${id}`).click()
      await settle(page)
      await expect(windowOf(page, id)).toBeVisible()
      await expect(page.getByTestId(`window-${id}-content`)).not.toBeEmpty()
    })

    for (const theme of ['light', 'dark'] as const) {
      test(`floating, ${theme}`, async ({ page }, testInfo) => {
        await openDesktop(page, { open: [id], theme })
        const win = windowOf(page, id)
        await expect(win).toBeVisible()
        await expectNoOverflow(contentOf(page, id))
        await expectAccessible(page, testInfo, { include: `[data-testid="window-${id}"]` })
        await expect(page).toHaveScreenshot(`${id}-floating-${theme}.png`)
      })
    }

    test('resizes', async ({ page }) => {
      await openDesktop(page, { open: [id] })
      const win = windowOf(page, id)
      const { minWidth, minHeight } = await page.evaluate((appId) => window.__agentos.apps[appId], id)

      await resizeWindow(page, id, 100, 100)
      const small = (await win.boundingBox())!
      expect(Math.round(small.width), 'clamped to the minimum width').toBe(minWidth)
      expect(Math.round(small.height), 'clamped to the minimum height').toBe(minHeight)
      await expectNoOverflow(contentOf(page, id))
      await expect(win).toHaveScreenshot(`${id}-min.png`)

      await resizeWindow(page, id, 1000, 680)
      const large = (await win.boundingBox())!
      const dock = (await page.getByTestId('dock').boundingBox())!
      expect(large.y + large.height, 'a resized window stays clear of the Dock').toBeLessThanOrEqual(dock.y)
      await expectNoOverflow(contentOf(page, id))
      await expect(win).toHaveScreenshot(`${id}-large.png`)
    })

    for (const layout of ['fullscreen', 'split-left', 'slideover'] as const) {
      test(`layout: ${layout}`, async ({ page }) => {
        await openDesktop(page, { open: [id] })
        await chooseLayout(page, id, layout)
        const win = windowOf(page, id)
        await expect(win).toHaveClass(new RegExp(`\\b${layout}\\b`))
        await expectNoOverflow(contentOf(page, id))
        await expect(page).toHaveScreenshot(`${id}-${layout}.png`)
      })
    }

    for (const screen of [
      { name: 'ipad', width: 1024, height: 768 },
      { name: 'large', width: 1600, height: 1000 }
    ]) {
      test(`screen: ${screen.name} (${screen.width}x${screen.height})`, async ({ page }) => {
        await page.setViewportSize({ width: screen.width, height: screen.height })
        await openDesktop(page, { open: [id] })
        const win = windowOf(page, id)
        await expect(win).toBeVisible()
        const box = (await win.boundingBox())!
        expect(box.x + box.width, 'window fits on screen').toBeLessThanOrEqual(screen.width)
        expect(box.y + box.height, 'window fits on screen').toBeLessThanOrEqual(screen.height)
        await expectNoOverflow(contentOf(page, id))
        await expect(page).toHaveScreenshot(`${id}-screen-${screen.name}.png`)
      })
    }
  })
}
