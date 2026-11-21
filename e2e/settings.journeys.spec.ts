/**
 * Settings journeys S1 to S7 (client/journeys/settings.md). S8 and S9 (the Devices pane, revoking a device) live in
 * client/e2e/devices.spec.ts: they pair a second device and run one after another there.
 *
 * The appearance settings live in the browser tab (the shell applies them to the document), so these never change
 * anything another spec can see. The notification switches are local to the Settings window.
 */
import type { Page } from '@playwright/test'
import { expect, expectNoOverflow, openDesktop, test } from './fixtures'

/** What the shell applied to the document root (`settings.apply`). */
const appliedTheme = (page: Page): Promise<string | null> =>
  page.evaluate(() => document.documentElement.dataset.theme ?? null)
const appliedTint = (page: Page): Promise<string> =>
  page.evaluate(() => document.documentElement.style.getPropertyValue('--tint'))
const appliedFontSize = (page: Page): Promise<string> => page.evaluate(() => document.documentElement.style.fontSize)
const reducedTransparency = (page: Page): Promise<boolean> =>
  page.evaluate(() => document.documentElement.classList.contains('reduce-transparency'))

test.describe('Settings journeys', () => {
  test('S1: switch to dark mode, then back to automatic', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    expect(await appliedTheme(page)).toBeNull()
    await page.getByTestId('settings-theme-dark').click()
    await expect(page.getByTestId('settings-theme-dark')).toHaveAttribute('aria-checked', 'true')
    expect(await appliedTheme(page)).toBe('dark')
    await page.getByTestId('settings-theme-auto').click()
    await expect(page.getByTestId('settings-theme-auto')).toHaveAttribute('aria-checked', 'true')
    expect(await appliedTheme(page)).toBeNull()
  })

  test('S2: change the accent colour to green', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    await expect(page.getByTestId('settings-accent-blue')).toHaveAttribute('aria-pressed', 'true')
    await page.getByTestId('settings-accent-green').click()
    await expect(page.getByTestId('settings-accent-green')).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByTestId('settings-accent-blue')).toHaveAttribute('aria-pressed', 'false')
    // the tint every app reads is now green
    expect(await appliedTint(page)).toBe('var(--ios-green)')
  })

  test('S3: make text bigger and turn off transparency', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    expect(await appliedFontSize(page)).toBe('100%')
    await page.getByTestId('settings-text-size').focus()
    await page.keyboard.press('End')
    expect(await appliedFontSize(page)).toBe('125%')
    await page.getByTestId('settings-reduce-transparency').click()
    await expect(page.getByTestId('settings-reduce-transparency')).toHaveAttribute('aria-checked', 'true')
    expect(await reducedTransparency(page)).toBe(true)
    await expectNoOverflow(page.getByTestId('window-settings-content'))
  })

  test('S4: turn off notification sounds, then all notifications', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    await page.getByTestId('settings-pane-notifications').click()
    await expect(page.getByTestId('settings-sounds')).toHaveAttribute('aria-checked', 'true')
    await page.getByTestId('settings-sounds').click()
    await expect(page.getByTestId('settings-sounds')).toHaveAttribute('aria-checked', 'false')
    await expect(page.getByTestId('settings-badges')).toBeEnabled()
    await page.getByTestId('settings-allow-notifications').click()
    await expect(page.getByTestId('settings-allow-notifications')).toHaveAttribute('aria-checked', 'false')
    for (const id of ['settings-sounds', 'settings-badges', 'settings-scheduled-summary'])
      await expect(page.getByTestId(id)).toBeDisabled()
  })

  test('S5: see which agents are on this host', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    await page.getByTestId('settings-pane-agents').click()
    const row = page.getByTestId('settings-agent-scripted')
    await expect(row).toBeVisible()
    await expect(row).toContainText('Scripted agent')
    await expect(row).toContainText('Answers by script, for tests')
  })

  test('S6: find out which version of vos and the host this is', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    await page.getByTestId('settings-pane-general').click()
    await expect(page.getByTestId('settings-about-name')).toContainText('vos')
    await expect(page.getByTestId('settings-about-version')).toContainText('0.1.0')
    // the host answered with its own version, not the "—" placeholder
    await expect(page.getByTestId('settings-about-host')).toContainText(/\d+\.\d+\.\d+/)
    await expect(page.getByTestId('settings-about-host')).not.toContainText('—')
  })

  test('S7: find a settings page by searching, then clear the search', async ({ page }) => {
    await openDesktop(page, { open: ['settings'] })
    await page.getByTestId('settings-search').fill('dev')
    await expect(page.getByTestId('settings-pane-devices')).toBeVisible()
    await expect(page.getByTestId('settings-pane-appearance')).toHaveCount(0)
    await expect(page.getByTestId('settings-pane-notifications')).toHaveCount(0)
    await page.getByTestId('settings-pane-devices').click()
    await expect(page.getByTestId('settings-devices')).toBeVisible()
    await page.getByTestId('settings-search-clear').click()
    for (const id of ['general', 'appearance', 'devices', 'agents', 'notifications', 'privacy'])
      await expect(page.getByTestId(`settings-pane-${id}`)).toBeVisible()
  })
})
