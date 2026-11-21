/**
 * Discover journeys V1 to V5 (client/journeys/discover.md): reading the landing window, the host's agents, searching,
 * narrowing by section and a narrow window. V6 and V7 (installing, disabling and removing apps) live in
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

  test('V2: see which agents this host has, with nothing offered to install', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    const agent = page.getByTestId('discover-agent-scripted')
    await expect(agent).toContainText('Scripted agent')
    await expect(agent).toContainText('Answers by script, for tests')
    // the Agents app runs, so the only action is OPEN (it opens the Agents app); there is never a GET
    await expect(page.getByTestId('discover-agent-scripted-get')).toHaveText('OPEN')
    await expect(page.getByTestId('discover-agents')).not.toContainText('GET')
  })

  test('V3: search for something that is not there, understand the result, get everything back', async ({
    page
  }) => {
    await openDesktop(page, { open: ['discover'] })
    await expect(page.getByTestId('discover-agent-scripted')).toBeVisible()
    await page.getByTestId('discover-search').fill('spreadsheet')
    await expect(page.getByTestId('discover-agent-scripted')).toHaveCount(0)
    await expect(page.getByTestId('discover-app-calculator')).toHaveCount(0)
    const text = page.getByTestId(content)
    await expect(text).toContainText('No apps match “spreadsheet”.')
    await expect(text).toContainText('No agents match “spreadsheet”.')
    // clearing the search brings everything back
    await page.getByTestId('discover-search-clear').click()
    await expect(page.getByTestId('discover-search')).toHaveValue('')
    await expect(page.getByTestId('discover-agent-scripted')).toBeVisible()
    await expect(page.getByTestId('discover-app-calculator')).toBeVisible()
    await expect(text).not.toContainText('No agents match')
  })

  test('V4: look at only the agents, then only the apps, then everything', async ({ page }) => {
    await openDesktop(page, { open: ['discover'] })
    await page.getByTestId('discover-section-agents').click()
    await expect(page.getByTestId('discover-agents')).toBeVisible()
    await expect(page.getByTestId('discover-apps')).toHaveCount(0)
    await page.getByTestId('discover-section-apps').click()
    await expect(page.getByTestId('discover-apps')).toBeVisible()
    await expect(page.getByTestId('discover-agents')).toHaveCount(0)
    await page.getByTestId('discover-section-discover').click()
    await expect(page.getByTestId('discover-apps')).toBeVisible()
    await expect(page.getByTestId('discover-agents')).toBeVisible()
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
