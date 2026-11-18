/**
 * Discover journeys V1 to V5 (client/journeys/discover.md): reading the landing window, searching it, the privacy
 * section and a narrow window. V6 and V7 (installing, disabling and removing apps) live in
 * client/e2e/discover.spec.ts: they change what the host has installed and run one after another there.
 * Nothing here installs anything.
 */
import { chooseLayout, expect, expectNoOverflow, openDesktop, test, windowOf } from './fixtures'

const content = 'window-discover-content'

test.describe('Discover journeys', () => {
  test('V1: a newcomer can tell what AgentOS does from the landing window', async ({ page }) => {
    await openDesktop(page)
    await expect(windowOf(page, 'discover')).toBeVisible()
    const text = page.getByTestId(content)
    await expect(text).toContainText('WELCOME TO AGENTOS')
    await expect(text).toContainText('Your agents, on one desktop.')
    await expect(text).toContainText('Run research, code, inbox and ops agents side by side')
    await expect(text).toContainText('keep everything on your machine')
    // and the two obvious next steps are there
    await expect(page.getByTestId('discover-hero-open-agents')).toBeVisible()
    await expect(page.getByTestId('discover-apps')).toContainText('Calculator')
  })

  // The Featured Agents are fixed sample listings: GET only flips the button in this window, it installs nothing on
  // the host (agents come from agentos.toml or agents.create). The journey describes the intent; see discover.md.
  test.fixme('V2: find an agent that helps with code and get it', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    await page.getByTestId('discover-search').fill('code')
    await expect(page.getByTestId('discover-listing-coder')).toBeVisible()
    await page.getByTestId('discover-listing-coder-get').click()
    // would need: the Coder agent on the host's roster (Settings > Agents), not just a button label
    await expect(page.getByTestId('discover-listing-coder-get')).toHaveText('OPEN')
  })

  test('V3: search for something that is not there, understand the result, get everything back', async ({
    page
  }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-listing-coder')).toBeVisible()
    await page.getByTestId('discover-search').fill('spreadsheet')
    await expect(page.getByTestId('discover-listing-coder')).toHaveCount(0)
    await expect(page.getByTestId('discover-app-calculator')).toHaveCount(0)
    const text = page.getByTestId(content)
    await expect(text).toContainText('No apps match “spreadsheet”.')
    await expect(text).toContainText('No agents match “spreadsheet”.')
    // clearing the search brings everything back
    await page.getByTestId('discover-search-clear').click()
    await expect(page.getByTestId('discover-search')).toHaveValue('')
    await expect(page.getByTestId('discover-listing-coder')).toBeVisible()
    await expect(page.getByTestId('discover-app-calculator')).toBeVisible()
    await expect(text).not.toContainText('No agents match')
  })

  test('V4: find out what agents can read and do, and open Settings from there', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    const text = page.getByTestId(content)
    await page.getByTestId('discover-privacy-details').scrollIntoViewIfNeeded()
    await expect(text).toContainText('Agent Privacy')
    await expect(text).toContainText('Can Act for You')
    await expect(text).toContainText('Can Read')
    await expect(text).toContainText('Never Leaves Device')
    await page.getByTestId('discover-privacy-details').click()
    await expect(windowOf(page, 'settings')).toBeVisible()
  })

  test('V5: Discover in Slide Over still reads well', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    await chooseLayout(page, 'discover', 'slideover')
    await expect(windowOf(page, 'discover')).toHaveClass(/\bslideover\b/)
    const box = (await windowOf(page, 'discover').boundingBox())!
    expect(Math.round(box.width), 'Slide Over is narrow').toBeLessThan(420)
    await expectNoOverflow(page.getByTestId(content))
    await expect(page.getByTestId('discover-hero-open-agents')).toBeVisible()
  })
})
