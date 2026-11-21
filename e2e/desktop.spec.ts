/**
 * Every app, in the real shell on the real host (e2e/host/start.ts):
 *  • opens from the Dock,
 *  • floating at its preferred size, light and dark: axe + layout + snapshot,
 *  • resized to its minimum and to a large window,
 *  • full screen, Split View and Slide Over,
 *  • on an iPad-sized screen and a large one.
 *
 * The built-in pages come from the shell's app registry and the installed apps from `apps/<id>/app.json`, so a new
 * app is covered with no extra code. The Calculator's display is "0" in every shot: nothing here presses a key,
 * and the journeys (which do) run after this project.
 */
import { readdirSync, readFileSync, existsSync } from 'node:fs'
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
  const source = readFileSync(join(ROOT, 'client/src/lib/apps.svelte.ts'), 'utf8')
  const block = source.slice(source.indexOf('const builtinApps'), source.indexOf('const isBuiltin'))
  return [...block.matchAll(/^ {2}([a-z][a-z0-9-]*): \{/gm)].map((m) => m[1])
}

/** Lua app packages (apps/<dir>/app.json): the host installs the ones in the test registry at start. */
function installedAppIds(): string[] {
  return readdirSync(join(ROOT, 'apps'))
    .map((dir) => join(ROOT, 'apps', dir, 'app.json'))
    .filter((file) => existsSync(file))
    .map((file) => (JSON.parse(readFileSync(file, 'utf8')) as { id: string }).id)
}

/** Layout is checked inside the window's content: the resize corner deliberately overhangs the frame. */
const contentOf = (page: Page, id: string) => page.getByTestId(`window-${id}-content`)

const APPS = [...builtinAppIds(), ...installedAppIds()]

test('the desktop renders with every app in the Dock', async ({ page }, testInfo) => {
  await openDesktop(page)
  for (const id of APPS) await expect(page.getByTestId(`dock-${id}`)).toBeVisible()
  await expectAccessible(page, testInfo)
  await expect(page).toHaveScreenshot('desktop-landing.png')
})

// With Reduce Motion (on in every spec) a window is drawn at its new frame as soon as the shell sets it: no transition
// holds the old size until a later frame, which made the Slide Over and full screen journeys (V5, D6) flaky (NEOC-146).
for (const layout of ['slideover', 'fullscreen'] as const) {
  test(`Reduce Motion: a window is drawn at its ${layout} frame at once`, async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    const win = await windowOf(page, 'discover').elementHandle()
    await page.getByTestId('window-discover-multitasking').click()
    const frame = await page.getByTestId(`window-discover-menu-${layout}`).evaluate(
      (item, el) =>
        new Promise<{ before: number; set: number; drawn: number }>((resolve) => {
          const before = el.getBoundingClientRect().width
          // read right after the shell writes the frame, in the same task, before any frame is drawn
          new MutationObserver((_, observer) => {
            observer.disconnect()
            resolve({ before, set: parseFloat(el.style.width), drawn: el.getBoundingClientRect().width })
          }).observe(el, { attributes: true, attributeFilter: ['style'] })
          item.dispatchEvent(new MouseEvent('click', { bubbles: true }))
        }),
      win
    )
    expect(frame.set, 'the layout gave the window another width').not.toBe(frame.before)
    expect(frame.drawn, 'the window is drawn at the width its layout gave it').toBe(frame.set)
  })
}

// With Reduce Motion nothing decorative keeps moving: Discover's floating icons stand still (only an indicator that says
// the app is busy keeps turning, and the landing page shows none once it has loaded).
test('Reduce Motion: nothing in Discover keeps moving', async ({ page }) => {
  await openDesktop(page, { open: ['discover'] })
  const moving = await page.getByTestId('window-discover-content').evaluate((el) =>
    el
      .getAnimations({ subtree: true })
      .filter((a) => a.playState === 'running')
      .map((a) => (a.effect instanceof KeyframeEffect && a.effect.target?.getAttribute('class')) || 'an animation')
  )
  expect(moving, 'what still moves').toEqual([])
})

for (const id of APPS) {
  test.describe(`app: ${id}`, () => {
    test('opens from the Dock', async ({ page }) => {
      await openDesktop(page)
      await page.getByTestId(`dock-${id}`).click()
      await settle(page)
      await expect(windowOf(page, id)).toBeVisible()
      await expect(page.getByTestId(`window-${id}-content`)).not.toBeEmpty()
      // an installed app's window is a view the host rendered: wait for it
      if (installedAppIds().includes(id)) await expect(page.getByTestId(`${id}-view`)).toBeVisible()
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
      const { minWidth, minHeight } = await page.evaluate((appId) => window.__vos.apps[appId], id)

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
