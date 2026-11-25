/**
 * Settings journeys. S8 and S9 (the Devices pane, revoking a device) live in client/e2e/devices.spec.ts: they pair a
 * second device and run one after another there. Turning banners off is shown against a real notification in
 * e2e/notifications.journeys.spec.ts (NO10), which has an app that sends one.
 *
 * Everything in General and Appearance belongs to the browser: the shell applies it to the document and keeps it in
 * `localStorage`. Each test starts in a fresh browser context, so nothing chosen here is seen by another spec.
 */
import type { Page } from '@playwright/test'
import { expect, expectNoOverflow, openDesktop, resizeWindow, settle, test } from './fixtures'

/** What the shell applied to the document root (`settings.apply`). */
const appliedTheme = (page: Page): Promise<string | null> =>
  page.evaluate(() => document.documentElement.dataset.theme ?? null)
const appliedTint = (page: Page): Promise<string> =>
  page.evaluate(() => document.documentElement.style.getPropertyValue('--tint'))
const reducedTransparency = (page: Page): Promise<boolean> =>
  page.evaluate(() => document.documentElement.classList.contains('reduce-transparency'))
/** The size a part's text is drawn at, in px. */
const fontSizeOf = (page: Page, testid: string): Promise<number> =>
  page.getByTestId(testid).evaluate((el) => parseFloat(getComputedStyle(el).fontSize))
const stored = (page: Page): Promise<Record<string, unknown> | null> =>
  page.evaluate(() => JSON.parse(localStorage.getItem('vos.settings.v1') ?? 'null') as Record<string, unknown> | null)

async function openSettings(page: Page, pane: 'general' | 'appearance' | 'devices'): Promise<void> {
  await openDesktop(page, { open: ['settings'] })
  await page.getByTestId(`settings-pane-${pane}`).click()
  await expect(page.getByTestId(`settings-pane-${pane}`)).toHaveAttribute('aria-current', 'true')
}

test.describe('Settings journeys', () => {
  test('S1: switch to dark mode, then back to automatic', async ({ page }) => {
    await openSettings(page, 'appearance')
    expect(await appliedTheme(page)).toBeNull()
    await page.getByTestId('settings-theme-dark').click()
    await expect(page.getByTestId('settings-theme-dark')).toHaveAttribute('aria-checked', 'true')
    expect(await appliedTheme(page)).toBe('dark')
    await page.getByTestId('settings-theme-auto').click()
    await expect(page.getByTestId('settings-theme-auto')).toHaveAttribute('aria-checked', 'true')
    expect(await appliedTheme(page)).toBeNull()
  })

  test('S2: change the accent colour to green', async ({ page }) => {
    await openSettings(page, 'appearance')
    await expect(page.getByTestId('settings-accent-blue')).toHaveAttribute('aria-pressed', 'true')
    await page.getByTestId('settings-accent-green').click()
    await expect(page.getByTestId('settings-accent-green')).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByTestId('settings-accent-blue')).toHaveAttribute('aria-pressed', 'false')
    // the tint every part of the shell reads is now green
    expect(await appliedTint(page)).toBe('light-dark(#1f7a36, #30d158)')
  })

  test('S3: make text bigger with the keyboard, and turn off transparency', async ({ page }) => {
    await openSettings(page, 'appearance')
    const title = await fontSizeOf(page, 'settings-text-size-value')
    await page.getByTestId('settings-text-size').focus()
    await page.keyboard.press('ArrowRight')
    await expect(page.getByTestId('settings-text-size-value')).toContainText('105%')
    await page.keyboard.press('End')
    await expect(page.getByTestId('settings-text-size-value')).toContainText('125%')
    // the text itself is bigger, not only the number
    await expect.poll(() => fontSizeOf(page, 'settings-text-size-value')).toBeCloseTo(title * 1.25, 2)
    await page.getByTestId('settings-reduce-transparency').click()
    await expect(page.getByTestId('settings-reduce-transparency')).toHaveAttribute('aria-checked', 'true')
    expect(await reducedTransparency(page)).toBe(true)
    await settle(page)
    await expectNoOverflow(page.getByTestId('window-settings-content'))
  })

  test('S4: turn banners off; the choice is kept', async ({ page }) => {
    await openSettings(page, 'general')
    const toggle = page.getByTestId('settings-banners')
    await expect(toggle).toHaveAttribute('aria-checked', 'true')
    await toggle.focus()
    await page.keyboard.press('Space')
    await expect(toggle).toHaveAttribute('aria-checked', 'false')
    expect(await stored(page)).toMatchObject({ banners: false })
    await openSettings(page, 'general') // loads the shell again
    await expect(page.getByTestId('settings-banners')).toHaveAttribute('aria-checked', 'false')
  })

  test('S5: what was chosen survives a reload, and a bad stored value does not break the shell', async ({ page }) => {
    await openSettings(page, 'appearance')
    await page.getByTestId('settings-theme-dark').click()
    await page.getByTestId('settings-accent-green').click()
    await page.getByTestId('settings-text-size').focus()
    await page.keyboard.press('Home')
    await page.getByTestId('settings-reduce-transparency').click()
    expect(await stored(page)).toEqual({
      theme: 'dark',
      accent: 'green',
      textScale: 85,
      reduceTransparency: true,
      banners: true,
      statusBar: false
    })

    // a reload: the document has them before any window is open, and the controls show them
    await openDesktop(page)
    expect(await appliedTheme(page)).toBe('dark')
    expect(await appliedTint(page)).toBe('light-dark(#1f7a36, #30d158)')
    expect(await reducedTransparency(page)).toBe(true)
    expect(await page.evaluate(() => document.documentElement.style.getPropertyValue('--text-scale'))).toBe('0.85')
    await openSettings(page, 'appearance')
    await expect(page.getByTestId('settings-theme-dark')).toHaveAttribute('aria-checked', 'true')
    await expect(page.getByTestId('settings-accent-green')).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByTestId('settings-text-size')).toHaveValue('85')
    await expect(page.getByTestId('settings-reduce-transparency')).toHaveAttribute('aria-checked', 'true')

    // the standalone page of an app reads the same settings: its app is loaded in the stored theme
    await page.goto('/apps/calculator')
    await expect(page.locator('iframe#app')).toHaveAttribute('src', /[?&]theme=dark\b/)

    // something else under the key (an older shell, a hand edit): the defaults, and the next choice is stored whole
    await page.evaluate(() => localStorage.setItem('vos.settings.v1', '{"theme":"sepia","textScale":4000,"accent":7}'))
    await openSettings(page, 'appearance')
    expect(await appliedTheme(page)).toBeNull()
    await expect(page.getByTestId('settings-theme-auto')).toHaveAttribute('aria-checked', 'true')
    await expect(page.getByTestId('settings-accent-blue')).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByTestId('settings-text-size')).toHaveValue('100')
    await page.getByTestId('settings-theme-light').click()
    expect(await stored(page)).toEqual({
      theme: 'light',
      accent: 'blue',
      textScale: 100,
      reduceTransparency: false,
      banners: true,
      statusBar: false
    })
  })

  test('S6: find out which version of the host this is', async ({ page }) => {
    await openSettings(page, 'general')
    // the host answered with its own version
    await expect(page.getByTestId('settings-about-host')).toContainText(/Host Version\s*\d+\.\d+\.\d+/)
  })

  test('S7: in a window at its smallest, the list and a pane are one at a time, by keyboard too', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    await resizeWindow(page, 'settings', 320, 240)
    const content = page.getByTestId('window-settings-content')
    // the list alone: no pane, nothing cut off
    for (const id of ['general', 'appearance', 'devices']) await expect(page.getByTestId(`settings-pane-${id}`)).toBeVisible()
    await expect(page.getByTestId('settings-nav')).toHaveCount(0)
    await expectNoOverflow(content)

    await page.getByTestId('settings-pane-appearance').focus()
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('settings-theme')).toBeVisible()
    await expect(page.getByTestId('settings-pane-appearance')).toHaveCount(0)
    await expect(page.getByTestId('settings-nav-back')).toBeFocused()
    await expectNoOverflow(content)
    // every control of the pane is inside the window's width and can be used
    await page.getByTestId('settings-theme-dark').click()
    expect(await appliedTheme(page)).toBe('dark')
    await page.getByTestId('settings-accent-graphite').click()
    expect(await appliedTint(page)).toBe('light-dark(#636366, #aeaeb2)')
    await page.getByTestId('settings-reduce-transparency').click()
    expect(await reducedTransparency(page)).toBe(true)
    await expectNoOverflow(content)

    await page.getByTestId('settings-nav-back').focus()
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('settings-pane-appearance')).toBeFocused()
    await expect(page.getByTestId('settings-theme')).toHaveCount(0)

    // wide again: both at once, on the pane that was open
    await resizeWindow(page, 'settings', 860, 580)
    await expect(page.getByTestId('settings-pane-appearance')).toHaveAttribute('aria-current', 'true')
    await expect(page.getByTestId('settings-theme')).toBeVisible()
    await expect(page.getByTestId('settings-nav-back')).toHaveCount(0)
  })
})
