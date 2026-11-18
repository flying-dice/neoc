/**
 * Discover journeys (client/journeys/discover.md): finding, installing, opening, disabling and removing apps on
 * the real host, from the test registry (e2e/host/start.ts). They use Notes, which the registry offers and
 * nothing else installs, so they never disturb the Calculator the other specs share.
 */
import { join } from 'node:path'
import { DIR } from '../../e2e/host/config'
import { expect, openDesktop, test, windowOf } from '../../e2e/fixtures'

const NOTES_APP_REF = `file://${join(DIR, 'registry/notes/0.1.0/app-ref.json')}`

test.describe.configure({ mode: 'serial' })

test.describe('Discover journeys', () => {
  test('V6: install an app from the registry, open it, disable it, enable it, remove it', async ({
    page
  }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-app-calculator-get')).toHaveText('OPEN')
    await expect(page.getByTestId('discover-app-notes-get')).toHaveText('GET')
    await expect(page.getByTestId('dock-notes')).toHaveCount(0)

    // install: the app appears in the Dock and its window opens, rendered from what the host sent
    await page.getByTestId('discover-app-notes-get').click()
    await expect(windowOf(page, 'notes')).toBeVisible()
    await expect(page.getByTestId('notes-view')).toBeVisible()
    await expect(page.getByTestId('dock-notes')).toBeVisible()
    await page.getByTestId('dock-discover').click() // opening Notes put Discover in the strip: bring it back
    await expect(page.getByTestId('discover-app-notes-get')).toHaveText('OPEN')

    // disable: the host unloads it, its window closes and it leaves the Dock, but it stays in Discover
    await page.getByTestId('discover-app-notes-toggle').click()
    await expect(windowOf(page, 'notes')).toHaveCount(0)
    await expect(page.getByTestId('dock-notes')).toHaveCount(0)
    await expect(page.getByTestId('discover-app-notes-state')).toContainText('Disabled')
    await expect(page.getByTestId('discover-app-notes-get')).toHaveText('ENABLE')

    // enable it again and open it
    await page.getByTestId('discover-app-notes-get').click()
    await expect(page.getByTestId('notes-view')).toBeVisible()
    await expect(page.getByTestId('dock-notes')).toBeVisible()
    await page.getByTestId('dock-discover').click()

    // remove: window and Dock icon go, and the registry offers it again
    await page.getByTestId('discover-app-notes-remove').click()
    await expect(windowOf(page, 'notes')).toHaveCount(0)
    await expect(page.getByTestId('dock-notes')).toHaveCount(0)
    await expect(page.getByTestId('discover-app-notes-get')).toHaveText('GET')
    await expect(page.getByTestId('discover-app-notes-remove')).toHaveCount(0)
  })

  test('V7: add an app by URL: preview first, a plain warning, then install', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-app-notes-get')).toHaveText('GET')

    // not an app ref: an error, nothing installed
    await page.getByTestId('discover-add-url').fill(`file://${DIR}/registry/nothing-here.json`)
    await page.getByTestId('discover-add-preview').click()
    await expect(page.getByTestId('discover-add-error')).toBeVisible()
    await expect(page.getByTestId('discover-add-preview-card')).toHaveCount(0)

    // a real app ref: what it is, where it comes from, what it asks for
    await page.getByTestId('discover-add-url').fill(NOTES_APP_REF)
    await page.getByTestId('discover-add-preview').click()
    const card = page.getByTestId('discover-add-preview-card')
    await expect(card).toContainText('Notes')
    await expect(card).toContainText('Version 0.1.0')
    await expect(card).toContainText('from this computer')
    await expect(card).toContainText('data, notify')
    await expect(page.getByTestId('dock-notes')).toHaveCount(0) // previewing installs nothing

    // the confirmation says what installing means; cancelling leaves things as they were
    await page.getByTestId('discover-add-install').click()
    const alert = page.getByTestId('discover-install-confirm')
    await expect(alert).toContainText('will run on this host')
    await expect(alert).toContainText('available to your agents')
    await page.getByTestId('discover-install-confirm-action-cancel').click()
    await expect(page.getByTestId('dock-notes')).toHaveCount(0)

    // confirming installs it and opens it
    await page.getByTestId('discover-add-install').click()
    await page.getByTestId('discover-install-confirm-action-install').click()
    await expect(page.getByTestId('notes-view')).toBeVisible()
    await expect(page.getByTestId('dock-notes')).toBeVisible()

    // clean up for the next journey
    await page.getByTestId('dock-discover').click()
    await expect(page.getByTestId('discover-add-preview-card')).toHaveCount(0)
    await page.getByTestId('discover-app-notes-remove').click()
    await expect(page.getByTestId('dock-notes')).toHaveCount(0)
  })
})
