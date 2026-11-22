/**
 * Desktop shell journeys (client/journeys/desktop.md): the Home Screen, the Dock and what is pinned to it, windows,
 * the recent-apps strip, Split View and full screen, on the real host. (System search has its own:
 * spotlight.journeys.spec.ts.) Nothing here presses a Calculator key or installs anything, so these can run
 * beside the specs that do (see the `verify-change` skill, "The end-to-end host").
 */
import type { Page } from '@playwright/test'
import {
  chooseFromAppMenu,
  chooseLayout,
  expect,
  frameOf,
  openDesktop,
  openFromHome,
  resizeWindow,
  settle,
  test,
  windowOf
} from './fixtures'

/** The apps of the Dock in order, with `|` where the divider between the pinned and the recent ones is. */
const dockOrder = (page: Page): Promise<string[]> =>
  page.getByTestId('dock').evaluate((tray) =>
    [...tray.children].map((el) =>
      el.getAttribute('data-testid') === 'dock-divider'
        ? '|'
        : (el.querySelector('button')?.getAttribute('data-testid')?.replace('dock-', '') ?? '?')
    )
  )

/**
 * Click an item of the recent-apps strip, as a person would. The strip auto-hides while a window covers it (its items then ignore
 * the pointer), and whether it does is settled by the layout a moment after the windows are placed: the edge that wakes it
 * exists only once it does. So look again until the item can really be clicked (the edge, if there is one, is rested on first),
 * rather than check once and act: a click that goes to a hidden strip finds nothing.
 */
async function clickStripItem(page: Page, testid: string): Promise<void> {
  const edge = page.getByTestId('stage-strip-edge')
  const item = page.getByTestId(testid)
  await expect(async () => {
    if (await edge.count()) await edge.hover()
    await item.click({ trial: true, timeout: 1000 })
  }).toPass()
  await item.click()
}

/**
 * A window wholly on a screen `width` wide, stopping above the Dock, on a page that does not scroll sideways: nothing of
 * it is cut off.
 */
async function expectWithin(page: Page, appId: string, width: number): Promise<void> {
  await settle(page)
  const box = (await windowOf(page, appId).boundingBox())!
  const dock = (await page.getByTestId('dock').boundingBox())!
  expect(box.x, 'not off the left edge').toBeGreaterThanOrEqual(0)
  expect(box.x + box.width, 'not off the right edge').toBeLessThanOrEqual(width)
  expect(box.y + box.height, 'clear of the Dock').toBeLessThanOrEqual(dock.y)
  const scrollWidth = await page.evaluate(() => document.scrollingElement?.scrollWidth ?? 0)
  expect(scrollWidth, 'the page does not scroll sideways').toBeLessThanOrEqual(width)
}

test.describe('Desktop journeys', () => {
  test('D1: first launch: see what vos is, then open an app from the Dock', async ({ page }) => {
    await openDesktop(page)
    // the landing window says what vos is
    await expect(windowOf(page, 'discover')).toBeVisible()
    await expect(page.getByTestId('window-discover-content')).toContainText('Find and install apps')
    await expect(page.getByTestId('window-discover-content')).toContainText('Watch them work')
    // the Dock opens another app, and Discover steps back into the recent-apps strip
    await page.getByTestId('dock-files').click()
    await expect(windowOf(page, 'files')).toBeVisible()
    await expect(page.getByTestId('stage-strip-discover')).toBeAttached()
  })

  test('D2: two apps side by side in Split View', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    await chooseLayout(page, 'files', 'split-left')
    await expect(windowOf(page, 'files')).toHaveClass(/\bsplit-left\b/)
    // the other half asks for an app, and shows them all: the Calculator is chosen there (it is not in the Dock)
    await expect(page.getByTestId('split-placeholder')).toBeVisible()
    await expect(page.getByTestId('split-placeholder')).toContainText('Choose an app')
    await expect(page.getByTestId('dock-calculator')).toHaveCount(0)
    await expect(page.getByTestId('split-placeholder').getByTestId('home-calculator')).toBeVisible()
    await openFromHome(page, 'calculator')
    await expect(page.getByTestId('split-placeholder')).toHaveCount(0)
    await expect(windowOf(page, 'calculator')).toHaveClass(/\bsplit-right\b/)
    await expect(page.getByTestId('split-divider')).toBeVisible()
    const left = (await windowOf(page, 'files').boundingBox())!
    const right = (await windowOf(page, 'calculator').boundingBox())!
    expect(left.x + left.width, 'Files is left of the Calculator').toBeLessThanOrEqual(right.x)
  })

  test('D3: make a window bigger, smaller, then move it', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    const win = windowOf(page, 'settings')
    const dock = (await page.getByTestId('dock').boundingBox())!
    const start = (await win.boundingBox())!

    await resizeWindow(page, 'settings', 1100, 700)
    const bigger = (await win.boundingBox())!
    expect(bigger.width).toBeGreaterThan(start.width)
    expect(bigger.y + bigger.height, 'a resized window stays clear of the Dock').toBeLessThanOrEqual(dock.y)

    await resizeWindow(page, 'settings', 600, 420)
    const smaller = (await win.boundingBox())!
    expect(smaller.width).toBeLessThan(bigger.width)
    expect(smaller.height).toBeLessThan(bigger.height)

    // move it by its title handle (the "···" dots in the middle are the menu, not a grip)
    const handle = (await page.getByTestId('window-settings-handle').boundingBox())!
    const grabX = handle.x + 24
    const grabY = handle.y + handle.height / 2
    await page.mouse.move(grabX, grabY)
    await page.mouse.down()
    await page.mouse.move(grabX + 60, grabY + 40, { steps: 6 })
    await page.mouse.up()
    await settle(page)
    const moved = (await win.boundingBox())!
    expect(Math.round(moved.x - smaller.x)).toBe(60)
    expect(Math.round(moved.y - smaller.y)).toBe(40)
    expect(moved.x + moved.width, 'nothing is cut off at the right').toBeLessThanOrEqual(1280)
    expect(moved.y + moved.height, 'nothing hides behind the Dock').toBeLessThanOrEqual(dock.y)
  })

  test('D4: switch back to an app from the recent-apps strip', async ({ page }) => {
    await openDesktop(page, { open: ['files', 'terminal'] })
    await expect(windowOf(page, 'terminal')).toBeVisible()
    await expect(windowOf(page, 'files')).toHaveCount(0)
    await clickStripItem(page, 'stage-strip-files')
    await settle(page)
    await expect(windowOf(page, 'files')).toBeVisible()
    // Terminal took Files' place in the strip
    await expect(windowOf(page, 'terminal')).toHaveCount(0)
    await expect(page.getByTestId('stage-strip-terminal')).toBeAttached()
  })

  test('D5: tidy up: minimise one window, close another', async ({ page }) => {
    await openDesktop(page, { open: ['files', 'terminal'] })
    // minimise the Terminal: it leaves the stage and waits in the strip
    await chooseLayout(page, 'terminal', 'minimize')
    await expect(windowOf(page, 'terminal')).toHaveCount(0)
    await expect(page.getByTestId('stage-strip-terminal')).toBeAttached()
    // bring Files back and close it: it is gone for good, the minimised Terminal is still in the strip
    await clickStripItem(page, 'stage-strip-files')
    await expect(windowOf(page, 'files')).toBeVisible()
    await chooseLayout(page, 'files', 'close')
    await expect(windowOf(page, 'files')).toHaveCount(0)
    await expect(page.getByTestId('stage-strip-files')).toHaveCount(0)
    await expect(page.getByTestId('stage-strip-terminal')).toBeAttached()
    // the Dock still opens Files fresh
    await page.getByTestId('dock-files').click()
    await expect(windowOf(page, 'files')).toBeVisible()
  })

  test('D6: take one app full screen, then go back to a floating window', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    const floating = (await windowOf(page, 'discover').boundingBox())!
    await chooseLayout(page, 'discover', 'fullscreen')
    await expect(windowOf(page, 'discover')).toHaveClass(/\bfullscreen\b/)
    const full = (await windowOf(page, 'discover').boundingBox())!
    expect(full.width).toBeGreaterThan(floating.width)
    expect(full.height).toBeGreaterThan(floating.height)

    await chooseLayout(page, 'discover', 'floating')
    await expect(windowOf(page, 'discover')).toHaveClass(/\bfloating\b/)
    const back = (await windowOf(page, 'discover').boundingBox())!
    expect(Math.round(back.width)).toBe(Math.round(floating.width))
    await expect(page.getByTestId('dock')).toBeVisible()
  })

  test('D7: tell the Dock icons apart by their names', async ({ page }) => {
    await openDesktop(page)
    const name = page.getByTestId('dock-settings-name')
    await expect(name).toHaveText('Settings')
    await expect(name, 'a name shows only for the icon being pointed at').toHaveCSS('opacity', '0')
    await page.getByTestId('dock-settings').hover()
    await expect(name).toHaveCSS('opacity', '1')
    await expect(page.getByTestId('dock-files-name')).toHaveCSS('opacity', '0')
    await page.mouse.move(1, 1)
    await expect(name).toHaveCSS('opacity', '0')

    // the keyboard gets the name too; a click that leaves focus on the icon does not keep it showing
    await page.getByTestId('dock-files').focus()
    await page.keyboard.press('Shift+Tab') // a key press makes the focus visible, as it is for a person who tabs
    await page.keyboard.press('Tab')
    await expect(page.getByTestId('dock-files')).toBeFocused()
    await expect(page.getByTestId('dock-files-name')).toHaveCSS('opacity', '1')
    await expect(page.getByTestId('dock-files')).toHaveAccessibleName('Files') // the capsule is for the eye only
    // the focus ring is the shared 3px blue, drawn round the icon's squircle rather than a box round the button
    const ring = await page.getByTestId('dock-files').evaluate((button) => {
      const plate = getComputedStyle(button.querySelector('.plate')!)
      return { width: plate.outlineWidth, style: plate.outlineStyle, radius: plate.borderTopLeftRadius }
    })
    expect(ring).toMatchObject({ width: '3px', style: 'solid' })
    expect(parseFloat(ring.radius)).toBeGreaterThan(10)
  })

  test('D8: Slide Over, then Full Screen: the window stays on screen', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    await chooseLayout(page, 'files', 'slideover')
    await expect(windowOf(page, 'files')).toHaveClass(/\bslideover\b/)
    await chooseLayout(page, 'files', 'fullscreen')
    await expect(windowOf(page, 'files')).toHaveClass(/\bfullscreen\b/)
    await expect(windowOf(page, 'files')).toBeVisible()
    await expect(page.getByTestId('stage-strip-files')).toHaveCount(0) // not sent to the recent-apps strip
  })

  test('D9: Split View on a screen too narrow for two apps is shown unavailable, not offered a half-sized window', async ({ page }) => {
    await page.setViewportSize({ width: 600, height: 800 })
    await openDesktop(page, { open: ['files'] })
    await page.getByTestId('window-files-multitasking').click()
    for (const side of ['split-left', 'split-right']) {
      const item = page.getByTestId(`window-files-menu-${side}`)
      await expect(item).toHaveAttribute('aria-disabled', 'true') // two 320 px minimums do not fit in 600
      await expect(item).toHaveAccessibleName(/unavailable/)
    }
    await page.getByTestId('window-files-menu-split-left').click({ force: true }) // does nothing (Playwright waits forever to click an aria-disabled control)
    await expect(page.getByTestId('window-files-menu')).toBeVisible()
    await expect(windowOf(page, 'files')).toHaveClass(/\bfloating\b/)
    await expect(page.getByTestId('split-placeholder')).toHaveCount(0)
    await expect(page.getByTestId('window-files-menu-fullscreen')).not.toHaveAttribute('aria-disabled')
  })

  test('D10: an app too wide for the waiting half opens on its own, and the person is told', async ({ page }) => {
    await page.setViewportSize({ width: 800, height: 800 }) // 774 to share: Files 320 + Agents 480 does not fit
    await openDesktop(page, { open: ['files'] })
    await chooseLayout(page, 'files', 'split-left')
    await expect(page.getByTestId('split-placeholder')).toBeVisible()
    await openFromHome(page, 'agents') // from the waiting half's own icons
    await expect(windowOf(page, 'agents')).toBeVisible()
    await expect(page.getByTestId('desktop-notice')).toContainText('opened on its own')
    await expect(page.getByTestId('desktop-notice')).toContainText('Files is in your recent apps') // where it actually is
    // the always-mounted live region carries the same words, so a screen reader hears them
    await expect(page.getByTestId('desktop-notice-live')).toContainText('opened on its own')
    await expect(page.getByTestId('split-placeholder')).toHaveCount(0)
    await expect(page.getByTestId('stage-strip-files')).toBeAttached() // Files is a normal window again, in the strip
    await page.getByTestId('desktop-notice-dismiss').click()
    await expect(page.getByTestId('desktop-notice')).toHaveCount(0)
    await expect(page.getByTestId('desktop-notice-live')).toHaveText('') // emptied, ready for the next
  })

  test('D11: on a narrow screen the app has the whole screen and the other layouts are shown unavailable', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 }) // a compact screen (under 560): Agents needs 480, it has 464
    await openDesktop(page, { open: ['agents'] })
    await expect(windowOf(page, 'agents')).toHaveClass(/\bfullscreen\b/)
    await expectWithin(page, 'agents', 480)
    await page.getByTestId('window-agents-multitasking').click()
    for (const layout of ['split-left', 'split-right', 'slideover', 'floating'])
      await expect(page.getByTestId(`window-agents-menu-${layout}`)).toHaveAttribute('aria-disabled', 'true')
    await expect(page.getByTestId('window-agents-menu-fullscreen')).not.toHaveAttribute('aria-disabled')
  })

  test('D12: an app in Slide Over has the whole screen on a phone, and is in Slide Over again on a wide screen', async ({ page }) => {
    await openDesktop(page, { open: ['agents'] })
    await chooseLayout(page, 'agents', 'slideover')
    await expect(windowOf(page, 'agents')).toHaveClass(/\bslideover\b/)
    await page.setViewportSize({ width: 390, height: 844 }) // a phone: Agents needs 480, the screen has 374
    await expect(windowOf(page, 'agents')).toHaveClass(/\bfullscreen\b/)
    await expectWithin(page, 'agents', 390)
    await page.getByTestId('window-agents-multitasking').click()
    await expect(page.getByTestId('window-agents-menu-slideover')).toHaveAttribute('aria-disabled', 'true')
    await expect(page.getByTestId('window-agents-menu-floating')).toHaveAttribute('aria-disabled', 'true')
    await page.keyboard.press('Escape')
    await page.setViewportSize({ width: 1280, height: 800 })
    await expect(windowOf(page, 'agents')).toHaveClass(/\bslideover\b/) // remembered, not dropped for the phone
  })

  test('D13: on a phone, open an app from the Dock, use it, go back home and open another', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await openDesktop(page)
    // the home screen: Discover, the first window, has the screen to itself above the Dock
    await expect(windowOf(page, 'discover')).toHaveClass(/\bfullscreen\b/)
    await expectWithin(page, 'discover', 390)
    // an app from the Dock takes the screen, and can be used
    await page.getByTestId('dock-files').click()
    await expect(windowOf(page, 'files')).toHaveClass(/\bfullscreen\b/)
    await expectWithin(page, 'files', 390)
    await page.getByTestId('files-search').fill('report')
    await expect(page.getByTestId('files-search')).toHaveValue('report')
    // back to the home screen with the window's ··· menu
    await chooseLayout(page, 'files', 'minimize')
    await expect(windowOf(page, 'files')).toHaveCount(0)
    await expect(page.getByTestId('dock')).toBeVisible()
    // and another app
    await page.getByTestId('dock-settings').click()
    await expect(windowOf(page, 'settings')).toHaveClass(/\bfullscreen\b/)
    await expectWithin(page, 'settings', 390)
  })

  test('D14: with no window open the Home Screen shows every app, and one opens from it', async ({ page }) => {
    await openDesktop(page, { open: [] })
    const home = page.getByTestId('home')
    await expect(home).toBeVisible()
    await expect(home).toHaveAccessibleName('Apps')
    // the Calculator is installed, and nobody pinned it: it is here, not in the Dock
    await expect(page.getByTestId('home-calculator')).toHaveAccessibleName('Calculator')
    await expect(page.getByTestId('home-calculator-name')).toHaveText('Calculator')
    await expect(page.getByTestId('dock-calculator')).toHaveCount(0)
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings'])

    await page.getByTestId('home-calculator').click()
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await expect(frameOf(page, 'calculator').getByTestId('calculator-app')).toBeVisible()
    // its icon is gone from under the window, and the keyboard is the app's
    await expect(home).toHaveCount(0)
    await expect(page.getByTestId('calculator-frame')).toBeFocused()
    // closing the last window brings the Home Screen back
    await chooseLayout(page, 'calculator', 'close')
    await expect(home).toBeVisible()
  })

  test('D15: the Home Screen by keyboard: arrows across the icons, Enter opens', async ({ page }) => {
    await openDesktop(page, { open: [] })
    await page.getByTestId('home-discover').focus()
    await page.keyboard.press('ArrowRight')
    await expect(page.getByTestId('home-files')).toBeFocused()
    await page.keyboard.press('ArrowRight')
    await expect(page.getByTestId('home-settings')).toBeFocused()
    await page.keyboard.press('ArrowLeft')
    await expect(page.getByTestId('home-files')).toBeFocused()
    // one stop in the tab order: Tab leaves the grid, Shift+Tab comes back to the same icon
    await page.keyboard.press('Tab')
    await expect(page.getByTestId('home-search')).toBeFocused()
    await page.keyboard.press('Shift+Tab')
    await expect(page.getByTestId('home-files')).toBeFocused()
    await page.keyboard.press('Enter')
    await expect(windowOf(page, 'files')).toBeVisible()
    expect(await page.evaluate(() => !!document.activeElement?.closest('[data-testid="window-files"]')), 'the keyboard is in the window').toBe(true)
  })

  test('D16: pin an app to the Dock, find it there after a reload, and unpin it', async ({ page }) => {
    await openDesktop(page, { open: [] })
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings'])
    // a secondary click on its icon: Open, and Add to Dock
    await page.getByTestId('home-calculator').click({ button: 'right' })
    await expect(page.getByTestId('app-menu')).toHaveAccessibleName('Calculator')
    await expect(page.getByTestId('app-menu-open')).toBeFocused()
    await expect(page.getByTestId('app-menu-unpin')).toHaveCount(0)
    await page.getByTestId('app-menu-pin').click()
    await expect(page.getByTestId('app-menu')).toHaveCount(0)
    await expect(page.getByTestId('dock-calculator')).toBeVisible()
    expect(await dockOrder(page), 'at the end: in the order they were added').toEqual(['discover', 'files', 'settings', 'calculator'])

    // it is this browser's: still there after a reload
    await openDesktop(page, { open: [] })
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings', 'calculator'])
    expect(await page.evaluate(() => localStorage.getItem('vos.dock.v1'))).toBe(
      JSON.stringify({ pinned: ['discover', 'files', 'settings', 'calculator'] })
    )
    // and it opens from the Dock now
    await page.getByTestId('dock-calculator').click()
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await chooseLayout(page, 'calculator', 'close')

    // unpin it from its icon in the Dock, with the keyboard: Shift+F10, the last item, Enter
    await page.getByTestId('dock-calculator').focus()
    await page.keyboard.press('Shift+F10')
    await expect(page.getByTestId('app-menu-open')).toBeFocused()
    await page.keyboard.press('ArrowDown')
    await expect(page.getByTestId('app-menu-unpin')).toBeFocused()
    await expect(page.getByTestId('app-menu-unpin')).toHaveText('Remove from Dock')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('dock-calculator')).toHaveCount(0)
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings'])
    // the icon the menu was opened on is gone: focus is on the app's icon on the Home Screen
    await expect(page.getByTestId('home-calculator')).toBeFocused()
    await openDesktop(page, { open: [] })
    expect(await dockOrder(page), 'and it stays out after a reload').toEqual(['discover', 'files', 'settings'])
  })

  test('D17: leaving an icon’s menu without choosing: Escape and a click outside give focus back, and open nothing', async ({ page }) => {
    await openDesktop(page, { open: [] })
    await page.getByTestId('home-calculator').focus()
    await page.keyboard.press('Shift+F10')
    await expect(page.getByTestId('app-menu-open')).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('app-menu')).toHaveCount(0)
    await expect(page.getByTestId('home-calculator')).toBeFocused()

    await page.getByTestId('dock-files').click({ button: 'right' })
    await expect(page.getByTestId('app-menu-unpin')).toBeVisible() // Files is pinned
    const menu = (await page.getByTestId('app-menu').boundingBox())!
    const icon = (await page.getByTestId('dock-files').boundingBox())!
    expect(menu.y + menu.height, 'the menu of a Dock icon opens above it').toBeLessThanOrEqual(icon.y)
    await page.mouse.click(300, 300) // on the wallpaper, where an icon of the Home Screen may be: the press only closes the menu
    await expect(page.getByTestId('app-menu')).toHaveCount(0)
    expect(await page.evaluate(() => window.__vos.wm.windows.length), 'nothing was opened').toBe(0)
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings'])
  })

  test('D18: an app that is open and not pinned is in the Dock’s recent apps, most recent first, three at most', async ({ page }) => {
    await openDesktop(page, { open: [] })
    await openFromHome(page, 'calculator')
    // after the divider, with the dot of a running app; the pinned apps are as they were
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings', '|', 'calculator'])
    await expect(page.getByTestId('dock-divider')).toHaveAccessibleName('Recent apps')
    await openFromHome(page, 'tasks-svelte')
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings', '|', 'tasks-svelte', 'calculator'])
    // the recent icon brings its app back to the front, and to the front of the recent apps
    await page.getByTestId('dock-calculator').click()
    await expect(windowOf(page, 'calculator')).toBeVisible()
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings', '|', 'calculator', 'tasks-svelte'])
    await openFromHome(page, 'tasks-react')
    await openFromHome(page, 'agents')
    expect(await dockOrder(page), 'the three used most recently').toEqual(['discover', 'files', 'settings', '|', 'agents', 'tasks-react', 'calculator'])
    // pinned from there, it moves across the divider and stays when it is closed
    await chooseFromAppMenu(page, 'dock-agents', 'pin')
    expect(await dockOrder(page)).toEqual(['discover', 'files', 'settings', 'agents', '|', 'tasks-react', 'calculator', 'tasks-svelte'])
    await chooseLayout(page, 'agents', 'close')
    await expect(page.getByTestId('dock-agents')).toBeVisible()
    // closed, an app that is not pinned leaves the Dock
    await page.getByTestId('dock-calculator').click()
    await chooseLayout(page, 'calculator', 'close')
    await expect(page.getByTestId('dock-calculator')).toHaveCount(0)
  })
})
