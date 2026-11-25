/**
 * Example key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend. They are the journeys a copy of the app starts from: open it, read a part the backend served,
 * call the backend with something typed, and fit the smallest window.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'

const ID = 'example'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')

async function openExample(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'part-contract')).toBeVisible()
}

test.describe('Example journeys', () => {
  test('EX1: open Example from the home screen and read a part the backend served', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(part(page, 'app')).toBeVisible()
    await expect(part(page, 'nothing')).toBeVisible()
    await part(page, 'part-backend').click()
    await expect(part(page, 'detail-title')).toHaveText('Backend')
    await expect(part(page, 'fact-backend-entry')).toHaveAttribute('value', 'server/main.luau')
  })

  test('EX2: greet someone: the name goes to the backend and its answer comes back', async ({ page }) => {
    await openExample(page)
    await part(page, 'hello').click()
    await expect(part(page, 'hello-greet')).toHaveAttribute('disabled')
    await field(page, 'hello-name').fill('  Ada ')
    await part(page, 'hello-greet').click()
    await expect(part(page, 'hello-greeting')).toHaveAttribute('heading', "Hello, Ada! This came from the Example app's Luau backend.")
    await expect(part(page, 'failure')).toHaveCount(0)
  })

  test('EX3: the smallest window shows the sidebar, and a part one push away', async ({ page }) => {
    await openExample(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(360)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await part(page, 'part-page').click()
    await expect(part(page, 'detail-title')).toHaveText('Page')
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await part(page, 'detail-back').click()
    await expect(part(page, 'sidebar')).toBeVisible()
  })
})
