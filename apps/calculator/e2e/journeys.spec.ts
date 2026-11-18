/**
 * Calculator key user journeys, end to end: the real shell in the browser, the real Rust host, the Calculator as
 * the Luau app it ships as. Journeys are listed in ../JOURNEYS.md; the design research panel walks the same ones.
 *
 * The host has one calculator however many windows are open, so these run one after another and each starts by
 * clearing it.
 */
import { expect, expectNoOverflow, openDesktop, resizeWindow, settle, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const key = (name: string) => `calculator-key-${name}`
const display = 'calculator-display'
const caption = 'calculator-caption'

test.describe.configure({ mode: 'serial' })

/** "C" clears what is typed, "AC" (nothing typed) clears everything: twice always ends at 0. */
async function clearCalculator(page: import('@playwright/test').Page): Promise<void> {
  await page.getByTestId(key('clear')).click()
  await page.getByTestId(key('clear')).click()
  await expect(page.getByTestId(display)).toHaveText('0')
}

async function openCalculator(page: import('@playwright/test').Page, others: string[] = []): Promise<void> {
  await openDesktop(page, { open: [...others, 'calculator'] })
  await expect(page.getByTestId('calculator-view')).toBeVisible()
  await clearCalculator(page)
}

test.describe('Calculator journeys', () => {
  test('J1: open the Calculator from the Dock and do a sum with the mouse', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId('dock-calculator').click()
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await expect(page.getByTestId('calculator-view')).toBeVisible()
    await clearCalculator(page)
    for (const k of ['1', '2', 'add', '7', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('19')
    await expect(page.getByTestId(caption)).toHaveText('12 + 7')
  })

  test('J2: type a calculation with the keyboard', async ({ page }) => {
    await openCalculator(page)
    await page.getByTestId(key('clear')).focus()
    await page.keyboard.type('250*0.18')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display)).toHaveText('45')
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await expect(page.getByTestId(display)).toHaveText('0')
  })

  test('J3: chain operations; the pending operator is highlighted until the next number', async ({ page }) => {
    await openCalculator(page)
    await page.getByTestId(key('9')).click()
    await page.getByTestId(key('mul')).click()
    await expect(page.getByTestId(key('mul'))).toHaveAttribute('aria-pressed', 'true')
    await page.getByTestId(key('3')).click()
    await expect(page.getByTestId(key('mul'))).toHaveAttribute('aria-pressed', 'false')
    await page.getByTestId(key('sub')).click()
    await expect(page.getByTestId(display)).toHaveText('27')
    await page.getByTestId(key('7')).click()
    await page.getByTestId(key('eq')).click()
    await expect(page.getByTestId(display)).toHaveText('20')
  })

  test('J4: recover from an error', async ({ page }) => {
    await openCalculator(page)
    for (const k of ['8', 'div', '0', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('Error')
    await page.getByTestId(key('clear')).click()
    await expect(page.getByTestId(display)).toHaveText('0')
    for (const k of ['4', 'add', '4', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('8')
  })

  test('J5: an agent puts a result on the Calculator and the open window follows', async ({ page }) => {
    await openDesktop(page)
    await expect(windowOf(page, 'calculator')).toHaveCount(0)

    // the agent calls the app's tool over MCP with the host's agent token; the host asks the shell for a window
    const shown = await callTool('calculator__show_on_calculator', { expression: '2450 * 0.18' })
    expect(shown).toContain('441')
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await expect(page.getByTestId(display)).toHaveText('441')
    await expect(page.getByTestId(caption)).toHaveText('2450 * 0.18')

    // the window is open: a second result arrives live, with no window opened and nothing pressed
    await callTool('calculator__show_on_calculator', { expression: '20 + 22' })
    await expect(page.getByTestId(display)).toHaveText('42')
    await expect(windowOf(page, 'calculator')).toHaveCount(1)

    // the person carries on from the agent's result
    await page.getByTestId(key('mul')).click()
    await page.getByTestId(key('2')).click()
    await page.getByTestId(key('eq')).click()
    await expect(page.getByTestId(display)).toHaveText('84')
  })

  test('J6: the Calculator keeps working in Slide Over next to another app', async ({ page }) => {
    await openCalculator(page, ['files'])
    await page.getByTestId('window-calculator-multitasking').click()
    await page.getByTestId('window-calculator-menu-slideover').click()
    await settle(page)
    await expect(windowOf(page, 'calculator')).toHaveClass(/slideover/)
    for (const k of ['6', 'mul', '7', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('42')
  })

  test('J7: the Calculator still works at its smallest window size', async ({ page }) => {
    await openCalculator(page)
    await resizeWindow(page, 'calculator', 100, 100)
    const win = (await windowOf(page, 'calculator').boundingBox())!
    expect(Math.round(win.width), 'the manifest minimum width').toBe(320)
    expect(Math.round(win.height), 'the manifest minimum height').toBe(480)
    await expectNoOverflow(page.getByTestId('window-calculator-content'))
    for (const k of ['5', 'add', '5', 'eq']) {
      const box = (await page.getByTestId(key(k)).boundingBox())!
      expect(Math.min(box.width, box.height), `${k} key is at least 44px`).toBeGreaterThanOrEqual(44)
      await page.getByTestId(key(k)).click()
    }
    await expect(page.getByTestId(display)).toHaveText('10')
  })
})
