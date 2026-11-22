/**
 * Calculator key user journeys (../JOURNEYS.md), end to end: the real shell, the real Rust host, the app's own
 * client in a frame of its window (or alone at /apps/calculator), its keys reaching `press` in its Luau backend.
 *
 * Each window holds its own calculator (the backend keeps nothing), so a journey starts from a fresh window.
 */
import type { FrameLocator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'calculator'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const key = (page: Page, name: string) => frame(page).getByTestId(`${ID}-key-${name}`)
const display = (page: Page) => frame(page).getByTestId(`${ID}-display`)
const caption = (page: Page) => frame(page).getByTestId(`${ID}-caption`)

async function tap(page: Page, keys: string[]): Promise<void> {
  // one at a time, each after the last one's answer is on the display: the order is the client's to keep, and the
  // test should not hide a missed key behind the next one
  for (const k of keys) await key(page, k).click()
}

async function openCalculator(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(frame(page).getByTestId(`${ID}-app`)).toBeVisible()
  await expect(display(page)).toHaveText('0')
}

test.describe('Calculator journeys', () => {
  test('CW1: open the Calculator from the home screen and do a sum with the mouse', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId(`dock-${ID}`).click()
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(display(page)).toHaveText('0')
    await tap(page, ['1', '2', 'add', '7'])
    await expect(display(page)).toHaveText('7')
    await key(page, 'eq').click()
    await expect(display(page)).toHaveText('19')
    await expect(caption(page)).toHaveText('12 + 7')
  })

  test('CW2: type a calculation with the keyboard', async ({ page }) => {
    await openCalculator(page)
    // the shell gives the frame the keyboard when its window is the focused one: no click first
    await page.keyboard.type('250*0.18')
    await page.keyboard.press('Enter')
    await expect(display(page)).toHaveText('45')
    await page.keyboard.press('Escape')
    await expect(display(page)).toHaveText('0')
    await expect(caption(page)).toHaveText('')

    await page.keyboard.type('12+79')
    await expect(display(page)).toHaveText('79')
    await page.keyboard.press('Backspace')
    await expect(display(page)).toHaveText('7')
    await page.keyboard.type('=')
    await expect(display(page)).toHaveText('19')
    await expect(caption(page)).toHaveText('12 + 7')

    // a key reached with Tab is pressed by Enter, as any button is; Enter after typing is `=` again
    await page.keyboard.press('Escape')
    await page.keyboard.press('Escape')
    await key(page, 'clear').focus()
    for (let i = 0; i < 4; i++) await page.keyboard.press('Tab') // ±, %, ÷, 7
    await page.keyboard.press('Enter')
    await expect(display(page)).toHaveText('7')
    await page.keyboard.type('+2')
    await page.keyboard.press('Enter')
    await expect(display(page)).toHaveText('9')
    await expect(caption(page)).toHaveText('7 + 2')
  })

  test('CW3: a mixed sum follows precedence and shows the waiting operator', async ({ page }) => {
    await openCalculator(page)
    await tap(page, ['2', 'add', '3', 'mul'])
    await expect(caption(page)).toHaveText('2 + 3 ×')
    await expect(display(page)).toHaveText('3')
    await expect(key(page, 'mul')).toHaveAttribute('pressed', '')
    await expect(key(page, 'add')).not.toHaveAttribute('pressed', '')
    await tap(page, ['4'])
    await expect(key(page, 'mul')).not.toHaveAttribute('pressed', '')
    await tap(page, ['eq'])
    await expect(display(page)).toHaveText('14')
    await expect(caption(page)).toHaveText('2 + 3 × 4')
  })

  test('CW4: divide by zero, see why, clear it and carry on', async ({ page }) => {
    await openCalculator(page)
    await tap(page, ['8', 'div', '0', 'eq'])
    await expect(display(page)).toHaveText('Error')
    await expect(caption(page)).toHaveText("Can't divide by zero")
    await expect(frame(page).getByTestId(`${ID}-announcer`)).toHaveText("Can't divide by zero")
    await tap(page, ['clear'])
    await expect(display(page)).toHaveText('0')
    await expect(frame(page).getByTestId(`${ID}-announcer`)).toHaveText('')
    await tap(page, ['4', 'add', '4', 'eq'])
    await expect(display(page)).toHaveText('8')
  })

  test('CW5: in the smallest window every key is visible and works', async ({ page }) => {
    await openCalculator(page)
    await resizeWindow(page, ID, 200, 200)
    const box = (await page.getByTestId(`window-${ID}-content`).boundingBox())!
    expect(Math.round(box.width)).toBeGreaterThanOrEqual(320)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    for (const k of ['clear', '7', 'div', '0', 'eq']) await expect(key(page, k)).toBeInViewport({ ratio: 1 })
    await tap(page, ['5', 'add', '5', 'eq'])
    await expect(display(page)).toHaveText('10')
  })

  test('CW7: an agent puts a result on the Calculator, closed and then open', async ({ page }) => {
    await openDesktop(page, { open: [] })
    await expect(windowOf(page, ID)).toHaveCount(0)
    const first = await callTool(`${ID}__show_on_calculator`, { expression: '18 / 100 * 2450' })
    expect(first).toContain('441')
    // the shell opened the window by itself, and the page took the waiting result when it started
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(display(page)).toHaveText('441')
    await expect(caption(page)).toHaveText('18 / 100 * 2450')
    // the second arrives in the open window, with nothing pressed
    await callTool(`${ID}__show_on_calculator`, { expression: '2 + 3 * 4' })
    await expect(display(page)).toHaveText('14')
    await expect(caption(page)).toHaveText('2 + 3 * 4')
    await expect(windowOf(page, ID)).toHaveCount(1)
    // and the person carries on from it
    await tap(page, ['add', '1', 'eq'])
    await expect(display(page)).toHaveText('15')
    await expect(caption(page)).toHaveText('14 + 1')
  })

  test('CW8: mixed sums by mouse and keyboard follow precedence, as the calculate tool does', async ({ page }) => {
    await openCalculator(page)
    await tap(page, ['2', 'add', '3', 'mul', '4', 'eq'])
    await expect(display(page)).toHaveText('14')
    await page.keyboard.press('Escape')
    await page.keyboard.type('2*3+4=')
    await expect(display(page)).toHaveText('10')
    await expect(caption(page)).toHaveText('2 × 3 + 4')
    for (const [expression, shown] of [
      ['2 + 3 * 4', '14'],
      ['2 * 3 + 4', '10']
    ]) {
      expect(JSON.parse(await callTool(`${ID}__calculate`, { expression }))).toMatchObject({ display: shown })
    }
  })

  for (const scheme of ['light', 'dark'] as const) {
    test(`CW6 (${scheme}): open the Calculator on its own page and do a sum`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme })
      await page.goto(`/apps/${ID}`)
      await expect(page).toHaveTitle('Calculator')
      await expect(page.getByTestId('dock')).toHaveCount(0)
      await expect(frame(page).locator('html')).toHaveAttribute('data-theme', scheme)
      // the page gives the app the keyboard once it is ready: a sum typed straight after load, no click first
      await expect(display(page)).toHaveText('0')
      await page.keyboard.type('6*7=')
      await expect(display(page)).toHaveText('42')
      await expect(caption(page)).toHaveText('6 × 7')
      await tap(page, ['clear', '8', 'div', '2', 'eq'])
      await expect(display(page)).toHaveText('4')
    })
  }
})
