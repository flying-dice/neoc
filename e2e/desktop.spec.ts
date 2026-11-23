/**
 * Every app, in the real shell on the real host (e2e/host/start.ts):
 *  • is on the Home Screen and opens from it (the Dock holds only what is pinned, and what is open),
 *  • floating at its preferred size, light and dark: axe + layout + snapshot,
 *  • resized to its minimum and to a large window,
 *  • full screen, Split View and Slide Over,
 *  • on an iPad-sized screen and a large one.
 *
 * The built-in pages come from the shell's app registry and the installed apps from their contracts (`openrpc.json`),
 * so a new app is covered with no extra code. The Calculator's display is "0" in every shot: nothing here presses a key,
 * and the journeys (which do) run after this project.
 */
import { readdirSync, readFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import {
  chooseLayout,
  expect,
  expectAccessible,
  expectNoOverflow,
  frameOf,
  openDesktop,
  openFromHome,
  resizeWindow,
  settle,
  test,
  windowOf
} from './fixtures'
import { FIXTURE_APPS } from './host/config'
import type { Page } from '@playwright/test'

const ROOT = join(import.meta.dirname, '..')

/** Built-in app ids, read from the `builtinApps` registry in apps.svelte.ts. */
function builtinAppIds(): string[] {
  const source = readFileSync(join(ROOT, 'client/src/lib/apps.svelte.ts'), 'utf8')
  const block = source.slice(source.indexOf('const builtinApps'), source.indexOf('const isBuiltin'))
  return [...block.matchAll(/^ {2}([a-z][a-z0-9-]*): \{/gm)].map((m) => m[1])
}

/**
 * The installed apps, each a page in a frame of its own (a contract, `openrpc.json`), that are installed at start: the framework
 * fixtures (`FIXTURE_APPS`; the other apps of e2e/apps are installed only by their journeys) and the repo's Vite projects
 * (apps/<dir>).
 */
function installedAppIds(): string[] {
  return [
    ...FIXTURE_APPS.map((dir) => join(ROOT, 'e2e/apps', dir, 'openrpc.json')),
    ...readdirSync(join(ROOT, 'apps')).map((dir) => join(ROOT, 'apps', dir, 'openrpc.json'))
  ]
    .filter((file) => existsSync(file))
    .map((file) => (JSON.parse(readFileSync(file, 'utf8')) as { 'x-app': { id: string } })['x-app'].id)
}

/** Layout is checked inside the window's content: the resize corner deliberately overhangs the frame. */
const contentOf = (page: Page, id: string) => page.getByTestId(`window-${id}-content`)

const APPS = [...builtinAppIds(), ...installedAppIds()]

test('the desktop lands on Discover, with the pinned apps and the open ones in the Dock', async ({ page }, testInfo) => {
  await openDesktop(page)
  // someone who never chose has Discover, Files and Settings in the Dock, and no other app
  for (const id of ['discover', 'files', 'settings']) await expect(page.getByTestId(`dock-${id}`)).toBeVisible()
  for (const id of APPS.filter((app) => !['discover', 'files', 'settings'].includes(app)))
    await expect(page.getByTestId(`dock-${id}`)).toHaveCount(0)
  // the Home Screen is not under a window
  await expect(page.getByTestId('home')).toHaveCount(0)
  // the status bar is a feature that is off until chosen (Settings > Features)
  await expect(page.getByTestId('status-bar-search')).toHaveCount(0)
  await expectAccessible(page, testInfo)
  await expect(page).toHaveScreenshot('desktop-landing.png')
})

test('the Home Screen shows every app', async ({ page }, testInfo) => {
  await openDesktop(page, { open: [] })
  const home = page.getByTestId('home')
  await expect(home).toBeVisible()
  // the built-in pages first, in their fixed order; then the installed apps by name
  const order = await home.locator('li').evaluateAll((items) => items.map((li) => (li as HTMLElement).dataset.app))
  expect(order.slice(0, 3)).toEqual(['discover', 'files', 'settings'])
  expect([...order].sort()).toEqual([...APPS].sort())
  const names = await home.locator('li:nth-child(n+4) [data-testid$="-name"]').allTextContents()
  expect(names, 'the installed apps are in the order of their names').toEqual(
    [...names].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }))
  )
  for (const id of APPS) {
    const icon = page.getByTestId(`home-${id}`)
    await icon.scrollIntoViewIfNeeded()
    await expect(icon).toBeVisible()
  }
  await expect(page.getByTestId('home-search')).toBeVisible()
  await expectNoOverflow(page.getByTestId('desktop-stage'))
  await expectAccessible(page, testInfo)
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
    test('opens from the Home Screen', async ({ page }) => {
      await openDesktop(page, { open: [] })
      await openFromHome(page, id)
      await settle(page)
      await expect(windowOf(page, id)).toBeVisible()
      await expect(page.getByTestId('home'), 'no icon is left under the window').toHaveCount(0)
      await expect(page.getByTestId(`dock-${id}`), 'an open app is in the Dock, pinned or not').toBeVisible()
      // an installed app's window is a frame, which has no text of its own: wait for its page to draw
      if (installedAppIds().includes(id)) await expect(frameOf(page, id).getByTestId(`${id}-app`)).toBeVisible()
      else await expect(page.getByTestId(`window-${id}-content`)).not.toBeEmpty()
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
