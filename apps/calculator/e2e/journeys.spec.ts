/**
 * Calculator key user journeys, end to end in the real desktop (web mode):
 * the renderer, the window manager and the Calculator's real main module and
 * tools, on the in-memory host. Journeys are listed in ../JOURNEYS.md; the
 * design research panel walks the same ones.
 */
import { expect, expectNoOverflow, openDesktop, resizeWindow, settle, test, windowOf } from '../../../e2e/fixtures'

const key = (name: string) => `calculator-key-${name}`

test.describe('Calculator journeys', () => {
  test('J1: open the Calculator from the Dock and do a sum with the mouse', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId('dock-calculator').click()
    await settle(page)
    await expect(windowOf(page, 'calculator')).toBeVisible()
    for (const k of ['1', '2', 'add', '7', 'equals']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('19')
    await expect(page.getByTestId('calculator-display-caption')).toHaveText('12 + 7')
  })

  test('J2: type a calculation with the keyboard', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    await page.getByTestId('calculator').focus()
    await page.keyboard.type('250*0.18')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('calculator-display-value')).toHaveText('45')
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('calculator-display-value')).toHaveText('0')
  })

  test('J3: chain operations; the pending operator is highlighted until the next number', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    await page.getByTestId(key('9')).click()
    await page.getByTestId(key('multiply')).click()
    await expect(page.getByTestId(key('multiply'))).toHaveAttribute('aria-pressed', 'true')
    await page.getByTestId(key('3')).click()
    await expect(page.getByTestId(key('multiply'))).toHaveAttribute('aria-pressed', 'false')
    await page.getByTestId(key('subtract')).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('27')
    await page.getByTestId(key('7')).click()
    await page.getByTestId(key('equals')).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('20')
  })

  test('J4: recover from an error', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    for (const k of ['8', 'divide', '0', 'equals']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('Error')
    await page.getByTestId(key('clear')).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('0')
  })

  test('J5: ask Claude to put a result on the Calculator', async ({ page }) => {
    await openDesktop(page, { open: ['agents'] })
    await page.getByTestId('agents-new-chat').click()
    const input = page.getByTestId('agents-composer-input')
    await input.fill('What is 18% of 2,450? Show it on the calculator.')
    await input.press('Enter')
    // The agent's tool opens the Calculator and shows the result there.
    await expect(windowOf(page, 'calculator')).toBeVisible()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('441')
    await expect(page.getByTestId('calculator-display-caption')).toHaveText('2450 * 0.18')
  })

  test('J6: the Calculator keeps working in Slide Over next to another app', async ({ page }) => {
    await openDesktop(page, { open: ['files', 'calculator'] })
    await page.getByTestId('window-calculator-multitasking').click()
    await page.getByTestId('window-calculator-menu-slideover').click()
    await settle(page)
    await expect(windowOf(page, 'calculator')).toHaveClass(/slideover/)
    for (const k of ['6', 'multiply', '7', 'equals']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId('calculator-display-value')).toHaveText('42')
  })

  test('J7: the Calculator still works at its smallest window size', async ({ page }) => {
    await openDesktop(page, { open: ['calculator'] })
    await resizeWindow(page, 'calculator', 100, 100)
    await expectNoOverflow(page.getByTestId('window-calculator-content'))
    for (const k of ['5', 'add', '5', 'equals']) {
      const box = (await page.getByTestId(key(k)).boundingBox())!
      expect(Math.min(box.width, box.height), `${k} key is at least 44px`).toBeGreaterThanOrEqual(44)
      await page.getByTestId(key(k)).click()
    }
    await expect(page.getByTestId('calculator-display-value')).toHaveText('10')
  })
})
