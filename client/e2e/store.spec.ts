/**
 * App store journeys in web mode: Discover lists the server's known apps;
 * Remove unregisters an app (its Dock icon and windows go) and GET installs it
 * again and opens it. The in-memory host runs the same register / unregister
 * lifecycle as the server.
 */
import { expect, openDesktop, settle, test, windowOf } from '../../e2e/fixtures'

test.describe('App store', () => {
  test('S1: remove an app from Discover, then get it back', async ({ page }) => {
    await openDesktop(page, { open: ['calculator', 'discover'] })
    await expect(page.getByTestId('dock-calculator')).toBeVisible()
    await expect(page.getByTestId('discover-app-calculator-get')).toHaveText('OPEN')

    await page.getByTestId('discover-app-calculator-remove').click()
    await expect(page.getByTestId('discover-app-calculator-get')).toHaveText('GET')
    await expect(page.getByTestId('dock-calculator')).toHaveCount(0)
    await expect(windowOf(page, 'calculator')).toHaveCount(0)

    await page.getByTestId('discover-app-calculator-get').click()
    await settle(page)
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await expect(page.getByTestId('dock-calculator')).toBeVisible()
    for (const k of ['6', 'multiply', '7', 'equals'])
      await page.getByTestId(`calculator-key-${k}`).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('42')
  })
})
