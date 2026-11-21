/**
 * Calculator key user journeys, end to end: the real shell in the browser, the real Rust host, the Calculator as
 * the Lua app it ships as. Journeys are listed in ../JOURNEYS.md; the design research panel walks the same ones.
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
    await expect(page.getByTestId('calculator-view'), 'a window opened from the Dock takes the keys').toBeFocused()
    await clearCalculator(page)
    for (const k of ['1', '2', 'add', '7', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('19')
    await expect(page.getByTestId(caption)).toHaveText('12 + 7')
  })

  test('J2: type a calculation with the keyboard', async ({ page }) => {
    await openCalculator(page)
    await page.getByTestId('calculator-view').focus() // the window itself: its shortcuts work without a key focused
    await page.keyboard.type('250*0.18')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display)).toHaveText('45')
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await expect(page.getByTestId(display)).toHaveText('0')

    // the other keys of the pad: + and = (Enter is the pad's own), Backspace for a slip of the finger
    await page.keyboard.type('12+79')
    await page.keyboard.press('Backspace')
    await page.keyboard.type('=')
    await expect(page.getByTestId(display)).toHaveText('19')
    await expect(page.getByTestId(caption)).toHaveText('12 + 7')

    // a key reached with Tab is pressed by Enter and by Space, as any button is (until a sum is typed on it)
    await clearCalculator(page)
    await page.getByTestId('calculator-view').focus()
    for (let i = 0; i < 5; i++) await page.keyboard.press('Tab') // AC, ±, %, ÷, 7
    await expect(page.getByTestId(key('7'))).toBeFocused()
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display)).toHaveText('7')
    await page.keyboard.press('Tab') // 8
    await page.keyboard.press(' ')
    await expect(page.getByTestId(display)).toHaveText('78')
    // ...but a key tabbed to and then typed on is not being navigated: Enter finishes the sum (it once pressed 8 again)
    await page.keyboard.type('+2')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display), 'Enter after typing on a key reached with Tab is =').toHaveText('80')
    await expect(page.getByTestId(caption)).toHaveText('78 + 2')

    // A key that was clicked keeps focus, and a keydown on it turns :focus-visible on in Chromium; Enter is still "="
    // there, because the person did not walk to that key.
    await clearCalculator(page)
    for (const k of ['7', 'add', '2']) await page.getByTestId(key(k)).click()
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display), 'Enter after clicks is =, not another press of 2').toHaveText('9')
    await expect(page.getByTestId(caption)).toHaveText('7 + 2')

    await clearCalculator(page)
    await page.getByTestId(key('7')).click()
    await page.keyboard.type('+3') // typed keys on a clicked key
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display), 'Enter after typing on a clicked key is =').toHaveText('10')

    // Tab hands Enter to the focused key again, and a click takes it back
    await clearCalculator(page)
    await page.getByTestId(key('7')).click()
    await page.keyboard.press('Tab') // 8
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display), 'a key reached with Tab').toHaveText('78')
    await page.getByTestId(key('add')).click()
    await page.getByTestId(key('2')).click()
    await page.keyboard.press('Enter')
    await expect(page.getByTestId(display)).toHaveText('80')
  })

  test('J3: chain operations; the pending operator is highlighted until the next number', async ({ page }) => {
    await openCalculator(page)
    const fillOf = (k: string) => page.getByTestId(key(k)).evaluate((el) => getComputedStyle(el).backgroundColor)
    const idle = await fillOf('mul')
    await page.getByTestId(key('9')).click()
    await page.getByTestId(key('mul')).click()
    await expect(page.getByTestId(key('mul'))).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByTestId(caption)).toHaveText('9 ×') // and said in words
    await expect.poll(() => fillOf('mul'), 'the pending key looks different, not only aria-pressed').not.toBe(idle)
    await page.getByTestId(key('3')).click()
    await expect(page.getByTestId(key('mul'))).toHaveAttribute('aria-pressed', 'false')
    await expect.poll(() => fillOf('mul')).toBe(idle)
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
    await expect(page.getByTestId(caption)).toHaveText("Can't divide by zero")
    // the reason is announced by the window's announcer (always on the page), not by a live region of its own
    await expect(page.getByTestId('calculator-announcer')).toHaveText("Can't divide by zero")
    await expect(page.getByTestId(caption)).not.toHaveAttribute('aria-live')
    await page.getByTestId(key('add')).click() // an operator has nothing to work on while the error shows
    await expect(page.getByTestId(display)).toHaveText('Error')
    await page.getByTestId(key('clear')).click()
    await expect(page.getByTestId(display)).toHaveText('0')
    await expect(page.getByTestId(caption)).toHaveText('')
    await expect(page.getByTestId('calculator-announcer')).toHaveText('') // so the same reason is read again next time
    for (const k of ['4', 'add', '4', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('8')

    // a digit is the other way out: it starts a new sum, as on an iPad
    for (const k of ['5', 'div', '0', 'eq', '6', 'add', '1', 'eq']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('7')
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

    // the menu did not take the keyboard with it: choosing from it leaves the keys with the calculator
    await clearCalculator(page)
    await page.getByTestId('window-calculator-multitasking').click()
    await page.getByTestId('window-calculator-menu-slideover').click() // already there: the same choice, the menu closes
    await expect(page.getByTestId('calculator-view'), 'the menu hands focus back to the window').toBeFocused()
    await page.keyboard.type('6*7=')
    await expect(page.getByTestId(display)).toHaveText('42')
    // and so does a typed key with focus on the "···" button itself (reached with Tab, not clicked)
    await clearCalculator(page)
    await page.getByTestId('window-calculator-multitasking').focus()
    await page.keyboard.type('6*7=')
    await expect(page.getByTestId(display)).toHaveText('42')

    // closing the menu by pressing "···" again does not leave the keys on the page behind it either
    await clearCalculator(page)
    await page.getByTestId('window-calculator-multitasking').click()
    await expect(page.getByTestId('window-calculator-menu')).toBeVisible()
    await page.getByTestId('window-calculator-multitasking').click()
    await expect(page.getByTestId('window-calculator-menu')).toHaveCount(0)
    await page.keyboard.type('6*7=')
    await expect(page.getByTestId(display)).toHaveText('42')

    // Files, waiting in the strip, comes back beside the Slide Over and is still usable
    await page.getByTestId('stage-strip-files').click()
    await expect(windowOf(page, 'files')).toBeVisible()
    await page.getByTestId('files-search').fill('refactor')
    await expect(page.getByTestId('files-recent-refactor-plan.md')).toBeVisible()
    await expect(page.getByTestId('files-recent-uptime-report.csv')).toHaveCount(0)
    await expect(windowOf(page, 'calculator')).toHaveClass(/slideover/)
    await expect(page.getByTestId(display)).toHaveText('42')
  })

  test('J7: the Calculator still works at its smallest window size', async ({ page }) => {
    await openCalculator(page)
    await resizeWindow(page, 'calculator', 100, 100)
    const win = (await windowOf(page, 'calculator').boundingBox())!
    expect(Math.round(win.width), 'the manifest minimum width').toBe(320)
    expect(Math.round(win.height), 'the manifest minimum height').toBe(500)
    await expectNoOverflow(page.getByTestId('window-calculator-content'))

    // dragging the resize corner leaves the keyboard with the calculator, not with the window frame
    await page.keyboard.type('5+5=')
    await expect(page.getByTestId(display)).toHaveText('10')
    await clearCalculator(page)
    for (const k of ['5', 'add', '5', 'eq']) {
      const box = (await page.getByTestId(key(k)).boundingBox())!
      expect(Math.min(box.width, box.height), `${k} key is at least 44px`).toBeGreaterThanOrEqual(44)
      await page.getByTestId(key(k)).click()
    }
    await expect(page.getByTestId(display)).toHaveText('10')

    // the longest number the keypad takes (nine digits, grouped) is shrunk to fit the display, never cut to "123,456,78…"
    await clearCalculator(page)
    for (const k of ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0']) await page.getByTestId(key(k)).click()
    await expect(page.getByTestId(display)).toHaveText('123,456,789')
    await expect
      .poll(() =>
        page.getByTestId(display).evaluate((el) => {
          const text = el.querySelector('.fit-text')!
          const glyphs = document.createRange()
          glyphs.selectNodeContents(text)
          return glyphs.getBoundingClientRect().width <= text.getBoundingClientRect().width
        })
      )
      .toBe(true)
  })
})
