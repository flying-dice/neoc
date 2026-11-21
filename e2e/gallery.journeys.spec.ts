/**
 * UI Kit journeys (client/journeys/gallery.md): the catalogue of `@vos/ui` that app builders read. Nothing
 * here touches the host's state.
 */
import type { Page } from '@playwright/test'
import { expect, openDesktop, test } from './fixtures'

/** The appearance the catalogue forces on itself (its wrapper's `data-theme`), or null for "System". */
const themeOf = (page: Page): Promise<string | null> =>
  page.getByTestId('gallery-nav').evaluate((el) => el.closest('[data-theme]')?.getAttribute('data-theme') ?? null)

test.describe('UI Kit journeys', () => {
  test('G1: find which button style to use for a secondary action', async ({ page }) => {
    await openDesktop(page, { open: ['gallery'] })
    for (const variant of ['filled', 'tinted', 'gray', 'plain']) {
      await page.getByTestId(`gallery-button-${variant}`).scrollIntoViewIfNeeded()
      await expect(page.getByTestId(`gallery-button-${variant}`)).toBeVisible()
    }
    // the specimen card names the variants
    await expect(page.getByTestId('window-gallery-content')).toContainText('filled · tinted · gray · plain')
  })

  test('G2: check how the components look in dark mode, then back', async ({ page }) => {
    await openDesktop(page, { open: ['gallery'] })
    expect(await themeOf(page)).toBeNull()
    await page.getByTestId('gallery-appearance-dark').click()
    await expect(page.getByTestId('gallery-appearance-dark')).toHaveAttribute('aria-checked', 'true')
    expect(await themeOf(page)).toBe('dark')
    // only the catalogue changed, not the desktop around it
    expect(await page.evaluate(() => document.documentElement.dataset.theme ?? null)).toBeNull()
    await page.getByTestId('gallery-appearance-light').click()
    expect(await themeOf(page)).toBe('light')
    await page.getByTestId('gallery-appearance-system').click()
    expect(await themeOf(page)).toBeNull()
  })

  test('G3: try a sheet and an alert, and dismiss each', async ({ page }) => {
    await openDesktop(page, { open: ['gallery'] })
    // the sheet: Cancel, then the scrim
    await page.getByTestId('gallery-present-sheet').scrollIntoViewIfNeeded()
    await page.getByTestId('gallery-present-sheet').click()
    await expect(page.getByTestId('gallery-sheet')).toBeVisible()
    await page.getByTestId('gallery-sheet-cancel').click()
    await expect(page.getByTestId('gallery-sheet')).toHaveCount(0)
    await page.getByTestId('gallery-present-sheet').click()
    await expect(page.getByTestId('gallery-sheet')).toBeVisible()
    await page.getByTestId('gallery-sheet-scrim').click({ position: { x: 4, y: 4 } })
    await expect(page.getByTestId('gallery-sheet')).toHaveCount(0)
    // the alert: Escape runs Cancel, and so does the Cancel button
    await page.getByTestId('gallery-show-alert').click()
    await expect(page.getByTestId('gallery-alert')).toBeVisible()
    // focus moves into the dialog a moment after it opens; Escape is heard from there
    await expect(page.getByTestId('gallery-alert-action-cancel')).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('gallery-alert')).toHaveCount(0)
    await page.getByTestId('gallery-show-alert').click()
    await page.getByTestId('gallery-alert-action-cancel').click()
    await expect(page.getByTestId('gallery-alert')).toHaveCount(0)
  })

  test('G4: find the system colours and the grays', async ({ page }) => {
    await openDesktop(page, { open: ['gallery'] })
    for (const name of ['blue', 'green', 'red', 'gray', 'gray6']) {
      await page.getByTestId(`gallery-color-${name}`).scrollIntoViewIfNeeded()
      await expect(page.getByTestId(`gallery-color-${name}`)).toBeVisible()
    }
    await expect(page.getByTestId('window-gallery-content')).toContainText('System Colors')
    await expect(page.getByTestId('window-gallery-content')).toContainText('Grays')
  })
})
