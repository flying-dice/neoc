/**
 * Update journeys: an installed app updated to its next version from Discover, on the real host. The Journal is built at
 * 0.1.0 (e2e/apps/journal) and at 0.2.0 (e2e/apps/journal-next, the same app with a second migration) outside the
 * registry (e2e/host/start.ts). The journey installs 0.1.0 and writes in it, publishes 0.2.0 in the test registry, updates
 * from Discover, and sees the entry kept and migrated, and 0.2.0 running in the window that was open on 0.1.0. At the end
 * the registry is put back and the Journal uninstalled, so no other journey sees either.
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { type Page } from '@playwright/test'
import { JOURNAL_APP_REF, JOURNAL_NEXT_APP_REF, REGISTRY } from './host/config'
import { expect, expectRunning, frameOf, isRunning, openDesktop, test } from './fixtures'

test.describe.configure({ mode: 'serial' })

/** The host-management call the shell makes (`window.__vos.host.api`), as the Get button of Discover does. */
interface HostApi {
  call(method: string, params?: Record<string, unknown>): Promise<unknown>
}
const hostCall = <T>(page: Page, method: string, params: Record<string, unknown>): Promise<T> =>
  page.evaluate(
    ([m, p]) =>
      (window as unknown as { __vos: { host: { api: HostApi } } }).__vos.host.api.call(
        m as string,
        p as Record<string, unknown>
      ),
    [method, params] as const
  ) as Promise<T>

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDesktop(page)
  if (await isRunning(page, 'journal')) {
    await hostCall(page, 'app.uninstall', { app: 'journal', purge: true })
    await expectRunning(page, 'journal', false)
  }
  await context.close()
})

test.describe('Update journeys', () => {
  test('U1: update an app from Discover: its data is kept and the open window runs the new version', async ({
    page
  }) => {
    const registry = readFileSync(REGISTRY, 'utf8')
    try {
      // 0.1.0 installed (from a ref no registry lists, as an app added by URL is), and an entry written in it
      await openDesktop(page)
      const preview = await hostCall<{ sha256: string }>(page, 'app.preview', { url: JOURNAL_APP_REF })
      await hostCall(page, 'app.install', { url: JOURNAL_APP_REF, sha256: preview.sha256 })
      await expectRunning(page, 'journal')
      await openDesktop(page, { open: ['journal'] })
      const journal = frameOf(page, 'journal')
      await expect(journal.getByTestId('journal-version')).toHaveText('Journal 0.1.0')
      await journal.getByTestId('journal-body').fill('Kept across the update')
      await journal.getByTestId('journal-add').click()
      await expect(journal.getByTestId('journal-entry-1')).toHaveText('Kept across the update')

      // 0.2.0 is published: the test registry lists it
      const listed = JSON.parse(registry) as { apps: string[] }
      writeFileSync(REGISTRY, JSON.stringify({ apps: [...listed.apps, JOURNAL_NEXT_APP_REF] }))

      // Discover offers it as an update of the installed app, with the version change; the Journal's window stays open
      await page.evaluate(() => window.__vos.wm.open('discover'))
      const listing = page.getByTestId('discover-app-journal')
      await expect(listing).toContainText('0.1.0 → 0.2.0')
      await expect(page.getByTestId('discover-app-journal-get')).toHaveText('OPEN')

      // the update is approved before anything changes: Cancel changes nothing
      await page.getByTestId('discover-app-journal-update').click()
      const alert = page.getByTestId('discover-update-alert')
      await expect(alert).toContainText('Update Journal?')
      await expect(alert).toContainText('read and write files, use the network and run processes')
      await page.getByTestId('discover-update-alert-action-cancel').click()
      await expect(alert).toHaveCount(0)
      await expect(journal.getByTestId('journal-version')).toHaveText('Journal 0.1.0')

      // approved: the host runs 0.2.0 and Discover says so
      await page.getByTestId('discover-app-journal-update').click()
      await page.getByTestId('discover-update-alert-action-update').click()
      await expect(listing).toContainText('0.2.0 · this computer')
      await expect(page.getByTestId('discover-app-journal-update')).toHaveCount(0)
      await expect(page.getByTestId('discover-apps-error')).toHaveCount(0)

      // the window that was open on 0.1.0 loaded 0.2.0's page: the entry is kept, and 0.2.0's migration ran on it
      await expect(journal.getByTestId('journal-version')).toHaveText('Journal 0.2.0')
      await expect(journal.getByTestId('journal-entry-1')).toHaveText(
        'Kept across the update (written in 0.1.0)'
      )
      // and what is written now is 0.2.0's
      await page.getByTestId('dock-journal').click()
      await journal.getByTestId('journal-body').fill('Written after it')
      await journal.getByTestId('journal-add').click()
      await expect(journal.getByTestId('journal-entry-2')).toHaveText('Written after it (written in 0.2.0)')
    } finally {
      writeFileSync(REGISTRY, registry)
    }
  })
})
