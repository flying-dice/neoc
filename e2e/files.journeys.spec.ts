/**
 * Files journeys (client/journeys/files.md). Files is a built-in page with fixed sample data, so these check what
 * the page shows today; nothing here touches the host's state.
 */
import { expect, expectNoOverflow, openDesktop, resizeWindow, test, windowOf } from './fixtures'

const refactorPlan = 'files-recent-refactor-plan.md'
const uptimeReport = 'files-recent-uptime-report.csv'

test.describe('Files journeys', () => {
  test('F1: find the refactor plan the Coder agent wrote, and when', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    const row = page.getByTestId(refactorPlan)
    await expect(row).toBeVisible()
    await expect(row).toContainText('refactor-plan.md')
    await expect(row).toContainText('Coder')
    await expect(row).toContainText('1 hr ago')
  })

  test('F2: search for a file by part of its name, then clear the search', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    await expect(page.getByTestId(uptimeReport)).toBeVisible()
    await page.getByTestId('files-search').fill('refactor')
    await expect(page.getByTestId(refactorPlan)).toBeVisible()
    await expect(page.getByTestId(uptimeReport)).toHaveCount(0)
    await page.getByTestId('files-search-clear').click()
    await expect(page.getByTestId('files-search')).toHaveValue('')
    await expect(page.getByTestId(uptimeReport)).toBeVisible()
    await expect(page.getByTestId(refactorPlan)).toBeVisible()
  })

  test('F3: find where deleted files go', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    const deleted = page.getByTestId('files-location-deleted')
    await expect(deleted).toBeVisible()
    await expect(deleted).toContainText('Recently Deleted')
  })

  test('F4: browse files in the smallest window', async ({ page }) => {
    await openDesktop(page, { open: ['files'] })
    await resizeWindow(page, 'files', 100, 100)
    const win = (await windowOf(page, 'files').boundingBox())!
    expect(Math.round(win.width), 'the shell minimum width').toBe(320)
    await expectNoOverflow(page.getByTestId('window-files-content'))
    for (const id of [refactorPlan, 'files-recent-agent-memory-survey.md', 'files-location-deleted']) {
      await page.getByTestId(id).scrollIntoViewIfNeeded()
      await expect(page.getByTestId(id)).toBeVisible()
    }
  })
})
