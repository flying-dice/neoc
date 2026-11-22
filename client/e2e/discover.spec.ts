/**
 * Discover journeys (client/journeys/discover.md): finding, installing, opening, disabling and removing apps on
 * the real host, from the test registry (e2e/host/start.ts). They use Memos (e2e/apps/memos), which the registry offers and
 * nothing else installs, so they never disturb the Calculator the other specs share.
 */
import { expect, frameOf, openDesktop, test, windowOf } from '../../e2e/fixtures'

test.describe.configure({ mode: 'serial' })

test.describe('Discover journeys', () => {
  test('V6: install an app from the registry, open it, disable it, enable it, remove it', async ({
    page
  }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-app-calculator-get')).toHaveText('OPEN')
    await expect(page.getByTestId('discover-app-memos-get')).toHaveText('GET')
    // Get installs at once, so the page says what an installed app can do before anyone presses it
    await expect(page.getByTestId('discover-apps-access-note')).toContainText(
      'read and write files, use the network and run processes'
    )
    await expect(page.getByTestId('dock-memos')).toHaveCount(0)

    // install: the app appears in the Dock and its window opens, with the app's own page in its frame
    await page.getByTestId('discover-app-memos-get').click()
    await expect(windowOf(page, 'memos')).toBeVisible()
    await expect(frameOf(page, 'memos').getByTestId('memos-app')).toBeVisible()
    await expect(page.getByTestId('dock-memos')).toBeVisible()
    await page.getByTestId('dock-discover').click() // opening Memos put Discover in the strip: bring it back
    await expect(page.getByTestId('discover-app-memos-get')).toHaveText('OPEN')

    // disable: the host unloads it, its window closes and it leaves the Dock, but it stays in Discover
    await page.getByTestId('discover-app-memos-toggle').click()
    await expect(windowOf(page, 'memos')).toHaveCount(0)
    await expect(page.getByTestId('dock-memos')).toHaveCount(0)
    await expect(page.getByTestId('discover-app-memos-state')).toContainText('Disabled')
    await expect(page.getByTestId('discover-app-memos-get')).toHaveText('ENABLE')

    // enable it again and open it
    await page.getByTestId('discover-app-memos-get').click()
    await expect(frameOf(page, 'memos').getByTestId('memos-app')).toBeVisible()
    await expect(page.getByTestId('dock-memos')).toBeVisible()
    await page.getByTestId('dock-discover').click()

    // remove: window and Dock icon go, and the registry offers it again
    await page.getByTestId('discover-app-memos-remove').click()
    await expect(windowOf(page, 'memos')).toHaveCount(0)
    await expect(page.getByTestId('dock-memos')).toHaveCount(0)
    await expect(page.getByTestId('discover-app-memos-get')).toHaveText('GET')
    await expect(page.getByTestId('discover-app-memos-remove')).toHaveCount(0)
  })
})
