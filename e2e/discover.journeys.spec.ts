/**
 * Discover journeys V1 and V3 to V5: reading the landing window, searching, the sidebar's sections and a narrow
 * window. V6 and V7 (installing, disabling and removing apps) live in
 * client/e2e/discover.spec.ts: they change what the host has installed and run one after another there.
 * Nothing here installs anything.
 */
import { chooseLayout, expect, expectNoOverflow, openDesktop, test, windowOf } from './fixtures'

const content = 'window-discover-content'

test.describe('Discover journeys', () => {
  test('V1: a newcomer can tell what vos does from the landing window', async ({ page }) => {
    await openDesktop(page)
    await expect(windowOf(page, 'discover')).toBeVisible()
    const text = page.getByTestId(content)
    await expect(text).toContainText('WELCOME TO VOS')
    await expect(text).toContainText('Your agents, on one desktop.')
    await expect(text).toContainText('Run research, code, inbox and ops agents side by side')
    await expect(text).toContainText('keep everything on your machine')
    // and the obvious next step is there
    await expect(page.getByTestId('discover-apps')).toContainText('Calculator')
  })

  test('V3: search for something that is not there, understand the result, get everything back', async ({
    page
  }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-app-calculator')).toBeVisible()
    await page.getByTestId('discover-search').fill('spreadsheet')
    await expect(page.getByTestId('discover-app-calculator')).toHaveCount(0)
    const text = page.getByTestId(content)
    await expect(text).toContainText('No apps match “spreadsheet”.')
    // clearing the search brings everything back
    await page.getByTestId('discover-search-clear').click()
    await expect(page.getByTestId('discover-search')).toHaveValue('')
    await expect(page.getByTestId('discover-app-calculator')).toBeVisible()
    await expect(text).not.toContainText('No apps match')
  })

  test('V4: the sidebar has Discover and Apps, and no Agents; the apps are listed under both', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-section-agents')).toHaveCount(0)
    await expect(page.getByTestId('discover-agents')).toHaveCount(0)
    await page.getByTestId('discover-section-apps').click()
    await expect(page.getByTestId('discover-apps')).toBeVisible()
    await page.getByTestId('discover-section-discover').click()
    await expect(page.getByTestId('discover-apps')).toBeVisible()
  })

  test('V5: Discover in Slide Over still reads well', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    await chooseLayout(page, 'discover', 'slideover')
    await expect(windowOf(page, 'discover')).toHaveClass(/\bslideover\b/)
    const box = (await windowOf(page, 'discover').boundingBox())!
    expect(Math.round(box.width), 'Slide Over is narrow').toBeLessThan(420)
    await expectNoOverflow(page.getByTestId(content))
    await expect(page.getByTestId('discover-apps')).toBeVisible()
  })
})
