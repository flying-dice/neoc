/**
 * Desktop shell journeys (client/journeys/desktop.md): the Dock, windows, the recent-apps strip, Split View and
 * full screen, on the real host. Nothing here presses a Calculator key or installs anything, so these can run
 * beside the specs that do (docs/TESTING.md, "The end-to-end host").
 */
import type { Page } from '@playwright/test'
import { chooseLayout, expect, openDesktop, resizeWindow, settle, test, windowOf } from './fixtures'

/** The strip auto-hides while a window covers it: rest the pointer on the left edge, as a person would. */
async function peekStrip(page: Page): Promise<void> {
  const edge = page.getByTestId('stage-strip-edge')
  if (await edge.count()) await edge.hover()
}

test.describe('Desktop journeys', () => {
  test('D1: first launch: see what AgentOS is, then open an app from the Dock', async ({ page }) => {
    await openDesktop(page)
    // the landing window says what AgentOS is
    await expect(windowOf(page, 'discover')).toBeVisible()
    await expect(page.getByTestId('window-discover-content')).toContainText('Your agents, on one desktop.')
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
    // the other half asks for an app; the Dock is still there to answer
    await expect(page.getByTestId('split-placeholder')).toBeVisible()
    await page.getByTestId('dock-calculator').click()
    await settle(page)
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
    await peekStrip(page)
    await page.getByTestId('stage-strip-files').click()
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
    await peekStrip(page)
    await page.getByTestId('stage-strip-files').click()
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
})
