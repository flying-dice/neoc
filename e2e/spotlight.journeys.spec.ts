/**
 * System search journeys (Spotlight), on the real host: the three ways to open it (the status bar's magnifier, the
 * "Search" capsule of the Home Screen, Cmd+K / Ctrl+K), finding an app and a Settings pane, and where the keyboard is
 * afterwards. Nothing here presses a Calculator key or installs anything.
 *
 * Not covered, because the shell cannot do it today: the shortcut while the keyboard is inside an app's frame (S7).
 */
import type { Page } from '@playwright/test'
import { chooseLayout, expect, expectAccessible, frameOf, openDesktop, showStatusBar, test, windowOf } from './fixtures'

const field = (page: Page) => page.getByTestId('spotlight-field')
const result = (page: Page, id: string) => page.getByTestId(`spotlight-result-${id}`)
/** The ids of the results listed, top first. */
const results = (page: Page): Promise<string[]> =>
  page
    .getByTestId('spotlight-results')
    .locator('[role="option"]')
    .evaluateAll((options) => options.map((o) => o.id.replace('spotlight-result-', '')))
/** The test id of the window the keyboard is in, or of the focused element when it is in none. */
const keyboardIn = (page: Page): Promise<string | null> =>
  page.evaluate(() => {
    const active = document.activeElement
    return (
      active?.closest('[data-testid^="window-"][role="dialog"]')?.getAttribute('data-testid') ??
      active?.getAttribute('data-testid') ??
      null
    )
  })

test.describe('Spotlight journeys', () => {
  test('S1: from the status bar, over a full screen app: find an app by name and open it', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    await showStatusBar(page)
    await chooseLayout(page, 'files', 'fullscreen') // the Dock is hidden and no icon of the Home Screen is on show
    await page.mouse.move(640, 300)
    const button = page.getByTestId('status-bar-search')
    await expect(button).toBeVisible()
    await expect(button).toHaveAccessibleName('Search')
    await button.click()

    await expect(page.getByTestId('spotlight')).toBeVisible()
    await expect(field(page)).toBeFocused()
    await expect(field(page)).toHaveValue('')
    await page.keyboard.type('calc')
    expect(await results(page)).toEqual(['app-calculator'])
    await expect(result(page, 'app-calculator')).toContainText('Calculator')
    await expect(result(page, 'app-calculator')).toContainText('App')
    await expect(field(page)).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-calculator')
    await page.keyboard.press('Enter')

    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await expect(frameOf(page, 'calculator').getByTestId('calculator-app')).toBeVisible()
    // the keyboard followed: it is the app's
    await expect(page.getByTestId('calculator-frame')).toBeFocused()
    expect(await page.evaluate(() => window.__vos.wm.focused?.appId)).toBe('calculator')
  })

  test('S2: from the Home Screen’s Search capsule: find a Settings pane and land on it', async ({ page }) => {
    await openDesktop(page, { open: [] })
    const pill = page.getByTestId('home-search')
    await expect(pill).toHaveAccessibleName('Search')
    const dock = (await page.getByTestId('dock').boundingBox())!
    const box = (await pill.boundingBox())!
    expect(box.y + box.height, 'the capsule is just above the Dock').toBeLessThanOrEqual(dock.y)
    await pill.click()
    await expect(field(page)).toBeFocused()

    await page.keyboard.type('appear')
    expect(await results(page)).toEqual(['settings-appearance'])
    await expect(result(page, 'settings-appearance')).toContainText('Settings')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(windowOf(page, 'settings')).toBeVisible()
    await expect(page.getByTestId('settings-theme'), 'Settings is on the Appearance pane').toBeVisible()
    expect(await keyboardIn(page)).toBe('window-settings')

    // Settings is open already: another pane is found and shown in the same window
    await showStatusBar(page)
    await page.getByTestId('status-bar-search').click()
    await expect(field(page)).toBeFocused()
    await page.keyboard.type('DEVI') // case does not matter
    expect(await results(page)).toEqual(['settings-devices'])
    await result(page, 'settings-devices').click() // a click opens a result too
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(page.getByTestId('settings-devices')).toBeVisible()
    await expect(page.getByTestId('settings-theme')).toHaveCount(0)
    expect(await keyboardIn(page)).toBe('window-settings')
  })

  test('S3: by the keyboard shortcut: open, move through the results, Escape twice, and focus is back where it was', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    const row = page.getByTestId('settings-pane-appearance')
    await row.focus()
    await page.keyboard.press('ControlOrMeta+K')
    await expect(field(page)).toBeFocused()
    // before anything is typed: suggestions, the app in front first
    await expect(page.getByTestId('spotlight-results')).toHaveAccessibleName('Suggestions')
    expect((await results(page))[0]).toBe('app-settings')
    await expect(page.getByTestId('spotlight-status')).toHaveText(/^\d suggestions$/)

    await page.keyboard.type('s')
    const listed = await results(page)
    expect(listed[0], 'a name that starts with what was typed comes first').toBe('app-settings')
    expect(listed.length).toBeGreaterThan(1)
    await expect(page.getByTestId('spotlight-status')).toHaveText(`${listed.length} results`)
    await expect(field(page)).toHaveAttribute('aria-activedescendant', `spotlight-result-${listed[0]}`)
    await page.keyboard.press('ArrowDown')
    await expect(field(page)).toHaveAttribute('aria-activedescendant', `spotlight-result-${listed[1]}`)
    await expect(result(page, listed[1])).toHaveAttribute('aria-selected', 'true')
    await expect(result(page, listed[0])).toHaveAttribute('aria-selected', 'false')
    await page.keyboard.press('ArrowUp')
    await page.keyboard.press('ArrowUp') // the top: stays
    await expect(field(page)).toHaveAttribute('aria-activedescendant', `spotlight-result-${listed[0]}`)
    await expect(field(page)).toBeFocused()

    // Escape clears what was typed; Escape again closes, and the row that had focus has it again
    await page.keyboard.press('Escape')
    await expect(field(page)).toHaveValue('')
    await expect(page.getByTestId('spotlight')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(row).toBeFocused()
    expect(await page.evaluate(() => window.__vos.wm.windows.length), 'nothing was opened').toBe(1)

    // the shortcut closes it as well, and so does a click on the dimmed desktop
    await page.keyboard.press('ControlOrMeta+K')
    await expect(field(page)).toBeFocused()
    await page.keyboard.press('ControlOrMeta+K')
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(row).toBeFocused()
    await page.keyboard.press('ControlOrMeta+K')
    await expect(field(page)).toBeFocused()
    await page.mouse.click(20, 700)
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(row).toBeFocused()
  })

  test('S3b: the shortcut works while the keys are in an app', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    await frameOf(page, 'calculator').getByTestId('calculator-key-7').click() // the keys are in the app's frame now
    await page.keyboard.press('ControlOrMeta+K')
    await expect(field(page)).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
  })

  test('S4: nothing found, and what a screen reader is given', async ({ page }, testInfo) => {
    await openDesktop(page)
    await page.keyboard.press('ControlOrMeta+K')
    const dialog = page.getByTestId('spotlight')
    await expect(dialog).toHaveAttribute('role', 'dialog')
    await expect(dialog).toHaveAttribute('aria-modal', 'true')
    await expect(field(page)).toHaveAttribute('role', 'combobox')
    await expect(field(page)).toHaveAttribute('aria-controls', 'spotlight-results')
    await expect(page.getByTestId('spotlight-results')).toHaveAttribute('role', 'listbox')
    await expectAccessible(page, testInfo, { include: '[data-testid="spotlight"]' })

    await page.keyboard.type('zebra crossing')
    await expect(page.getByTestId('spotlight-empty')).toHaveText('No Results')
    await expect(page.getByTestId('spotlight-status')).toHaveText('No results')
    await expect(field(page)).toHaveAttribute('aria-expanded', 'false')
    await expectAccessible(page, testInfo, { include: '[data-testid="spotlight"]' })
    await page.keyboard.press('Enter') // opens nothing
    await expect(dialog).toBeVisible()
    expect(await page.evaluate(() => window.__vos.wm.windows.length)).toBe(1)

    // accents do not matter
    await page.getByTestId('spotlight-clear').click()
    await expect(field(page)).toBeFocused()
    await page.keyboard.type('cálc')
    expect(await results(page)).toEqual(['app-calculator'])
  })

  test('S5: Show Home Screen leaves the windows where they are and shows every app', async ({ page }) => {
    await openDesktop(page, { open: ['files', 'settings'] })
    await page.keyboard.press('ControlOrMeta+K')
    // search has the keyboard a moment after it opens: what is typed before that goes to the window behind
    await expect(field(page)).toBeFocused()
    await page.keyboard.type('show home')
    await expect.poll(async () => (await results(page))[0]).toBe('action-home')
    await expect(result(page, 'action-home')).toContainText('Show Home Screen')
    await expect(result(page, 'action-home')).toContainText('Action')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(page.getByTestId('home')).toBeVisible()
    await expect(windowOf(page, 'settings')).toBeHidden()
    expect(await page.evaluate(() => window.__vos.wm.windows.length), 'no window was closed').toBe(2)
    await expect(page.getByTestId('stage-strip-settings')).toBeAttached()
    await expect(page.getByTestId('stage-strip-files')).toBeAttached()
    // the keyboard is on the Home Screen
    expect(await page.evaluate(() => !!document.activeElement?.closest('[data-testid="home"]'))).toBe(true)
    // on the Home Screen the action is not offered
    await page.getByTestId('home-search').click()
    await expect(field(page)).toBeFocused()
    await page.keyboard.type('show home')
    await expect(page.getByTestId('spotlight-status')).toHaveText(/results$/i)
    expect(await results(page)).not.toContain('action-home')
  })

  test('S6: while search is open, what is typed goes to search and not to the app behind', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    const display = frameOf(page, 'calculator').getByTestId('calculator-display')
    await expect(display).toBeVisible()
    await expect(page.getByTestId('calculator-frame')).toBeFocused() // the app has the keyboard
    const before = await display.textContent()
    // the status bar's button is the way in while an app has the keyboard
    await showStatusBar(page)
    await page.getByTestId('status-bar-search').click()
    await expect(field(page)).toBeFocused()
    await page.keyboard.type('files 123')
    await expect(field(page)).toHaveValue('files 123')
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('spotlight')).toHaveCount(0)
    await expect(display).toHaveText(before ?? '')
    await expect(page.getByTestId('status-bar-search'), 'focus is back on what opened search').toBeFocused()
  })

  // The gap: a key pressed inside an app's frame never reaches the shell. The frame is sandboxed (`allow-scripts
  // allow-forms`, a different origin from the shell) and the app bridge (crates/vos-http/src/apps/bridge.js) forwards no
  // key to its parent, so the shell cannot hear Cmd+K / Ctrl+K while an app has the keyboard. It needs the bridge to
  // post the chord (a notification the shell's hostBridge listens for); until then the status bar's button (S6) is the
  // way in.
  test.fixme('S7: the shortcut opens search while the keyboard is inside an app’s frame', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    await expect(page.getByTestId('calculator-frame')).toBeFocused()
    await page.keyboard.press('ControlOrMeta+K')
    await expect(field(page)).toBeFocused()
  })
})
