/**
 * Terminal journeys (client/journeys/terminal.md). The Terminal is a built-in page showing fixed sample output
 * (it is not a shell and not live), so these check what the page shows today.
 */
import { expect, expectAccessible, expectNoOverflow, openDesktop, resizeWindow, test, windowOf } from './fixtures'

test.describe('Terminal journeys', () => {
  test('T1: see at a glance which agents are running, waiting or idle', async ({ page }) => {
    await openDesktop(page, { open: ['terminal'] })
    const out = page.getByTestId('terminal-output')
    await expect(out).toContainText('research   running')
    await expect(out).toContainText('coder      waiting')
    await expect(out).toContainText('PR ready for review')
    await expect(out).toContainText('inbox      idle')
  })

  test('T2: read the output in the smallest window', async ({ page }) => {
    await openDesktop(page, { open: ['terminal'] })
    await resizeWindow(page, 'terminal', 100, 100)
    const win = (await windowOf(page, 'terminal').boundingBox())!
    expect(Math.round(win.width), 'the shell minimum width').toBe(320)
    await expectNoOverflow(page.getByTestId('window-terminal-content'))
    await expect(page.getByTestId('terminal-output')).toContainText('PR ready for review')
    const scrollsSideways = await page
      .getByTestId('terminal-output')
      .evaluate((el) => el.scrollWidth > el.clientWidth + 1)
    expect(scrollsSideways, 'long lines wrap instead of scrolling sideways').toBe(false)
  })

  test('T3: in light mode the terminal stays dark and legible', async ({ page }, testInfo) => {
    await openDesktop(page, { open: ['terminal'], theme: 'light' })
    await expect(page.getByTestId('terminal')).toHaveAttribute('data-theme', 'dark')
    const [bg, fg] = await page.getByTestId('terminal-output').evaluate((el) => {
      const rgb = (c: string): number[] => (c.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number)
      let node: Element | null = el
      let bgColor = 'rgba(0, 0, 0, 0)'
      while (node && /rgba\(.*, 0\)|transparent/.test(bgColor)) {
        bgColor = getComputedStyle(node).backgroundColor
        node = node.parentElement
      }
      return [rgb(bgColor), rgb(getComputedStyle(el).color)]
    })
    const luminance = ([r, g, b]: number[]): number => (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255
    expect(luminance(bg), 'the terminal background is dark').toBeLessThan(0.25)
    expect(luminance(fg), 'the output text is lighter than its background').toBeGreaterThan(luminance(bg))
    await expectAccessible(page, testInfo, { include: '[data-testid="window-terminal"]' })
  })
})
