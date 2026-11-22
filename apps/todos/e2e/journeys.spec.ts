/**
 * Todos key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window, and
 * its Luau backend, which keeps the lists and the todos in the app's database and offers them to agents as tools.
 *
 * The todos live on the shared host, so each test names its own (a title no other run has) and finds them by that name.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'todos'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const newTodo = (page: Page): Locator => part(page, 'new-input').locator('input')
const newList = (page: Page): Locator => part(page, 'new-list-input').locator('input')

let serial = 0
const unique = (what: string): string => `${what} ${Date.now().toString(36)}-${++serial}`

/** A todo's row, found by its title: the check is named after it (`Complete: Buy milk`). */
const row = (page: Page, title: string): Locator =>
  frame(page)
    .locator('li')
    .filter({ has: frame(page).getByRole('checkbox', { name: `: ${title}` }) })
const check = (page: Page, title: string): Locator => row(page, title).getByRole('checkbox')
const within = (page: Page, title: string, name: string): Locator => row(page, title).getByTestId(new RegExp(`^todos-item-\\d+-${name}$`))
/** A list of the person's own in the sidebar, found by its name. */
const listRow = (page: Page, name: string): Locator => frame(page).locator('ui-row[data-testid^="todos-list-"]').filter({ hasText: name })

async function openTodos(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await expect(part(page, 'smart-today')).toBeVisible()
}

const SMART = { today: 'Today', scheduled: 'Scheduled', all: 'All', flagged: 'Flagged', completed: 'Completed' } as const

/** Show a smart list, and wait until it is the one in front. */
async function show(page: Page, smart: keyof typeof SMART): Promise<void> {
  await part(page, `smart-${smart}`).click()
  await expect(part(page, 'title')).toHaveText(SMART[smart])
}

async function add(page: Page, title: string): Promise<void> {
  await newTodo(page).fill(title)
  await newTodo(page).press('Enter')
  await expect(row(page, title)).toBeVisible()
}

/** A list of its own for a journey, shown as soon as it is added. */
async function addList(page: Page, name: string): Promise<void> {
  await newList(page).fill(name)
  await newList(page).press('Enter')
  await expect(part(page, 'title')).toHaveText(name)
}

/** Today where the app's client is, as the app writes a day. */
const today = (page: Page): Promise<string> =>
  frame(page)
    .locator('body')
    .evaluate(() => {
      const now = new Date()
      return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
    })

test.describe('Todos journeys', () => {
  test('TD1: open Todos from the Dock and see the lists', async ({ page }) => {
    await openDesktop(page)
    await page.getByTestId(`dock-${ID}`).click()
    await expect(windowOf(page, ID)).toBeVisible()
    for (const smart of Object.keys(SMART)) await expect(part(page, `smart-${smart}`)).toBeVisible()
    await expect(listRow(page, 'Reminders')).toBeVisible()
    await expect(part(page, 'title')).toHaveText('Today')
  })

  test('TD2: add a todo, tick it off, find it among the completed and bring it back', async ({ page }) => {
    const title = unique('Buy milk')
    await openTodos(page)
    await show(page, 'all')
    await add(page, title)
    await expect(newTodo(page)).toHaveValue('') // ready for the next
    await check(page, title).click()
    await expect(row(page, title)).toHaveCount(0)
    await show(page, 'completed')
    await expect(check(page, title)).toBeChecked()
    await check(page, title).click()
    await expect(row(page, title)).toHaveCount(0)
    await show(page, 'all')
    await expect(check(page, title)).not.toBeChecked()
  })

  test('TD3: give a todo a day, notes and a flag; Today and Flagged gather it', async ({ page }) => {
    const title = unique('Call the dentist')
    await openTodos(page)
    await show(page, 'all')
    await add(page, title)
    await within(page, title, 'details').click()
    await within(page, title, 'due').fill(await today(page))
    await expect(within(page, title, 'due-label')).toHaveText('Today')
    await within(page, title, 'notes').locator('input').fill('Ask for a morning')
    await within(page, title, 'notes').locator('input').press('Tab')
    await within(page, title, 'flag').click()
    await expect(within(page, title, 'flag')).toHaveAttribute('pressed', '')
    await show(page, 'today')
    await expect(row(page, title)).toBeVisible()
    await expect(within(page, title, 'notes-label')).toHaveText('Ask for a morning')
    await show(page, 'flagged')
    await expect(row(page, title)).toBeVisible()
    // the day is taken away again: Today lets go of it
    await within(page, title, 'details').click()
    await within(page, title, 'no-due').click()
    await expect(within(page, title, 'due-label')).toHaveCount(0)
    await show(page, 'today')
    await expect(row(page, title)).toHaveCount(0)
  })

  test('TD4: rename a todo by typing over its title', async ({ page }) => {
    const title = unique('Water the plants')
    const renamed = `${title}, and the herbs`
    await openTodos(page)
    await show(page, 'all')
    await add(page, title)
    // the row goes by the title the todo has until the new one is committed
    await within(page, title, 'title').locator('input').fill(renamed)
    await within(page, title, 'title').locator('input').press('Enter')
    await expect(row(page, renamed)).toBeVisible()
    // the backend has it: another list shows the new title
    await show(page, 'completed')
    await show(page, 'all')
    await expect(within(page, renamed, 'title').locator('input')).toHaveValue(renamed)
  })

  test('TD5: a list of its own: add it, fill it, and delete it with its todos', async ({ page }) => {
    const list = unique('Groceries')
    const title = unique('Bread')
    await openTodos(page)
    await addList(page, list)
    await expect(listRow(page, list)).toHaveAttribute('selected', '')
    await add(page, title)
    await expect(listRow(page, list)).toHaveAttribute('aria-label', `${list}, 1`) // it counts what is still to do
    await part(page, 'delete-list').click()
    await expect(part(page, 'confirm')).toContainText('Its todos are deleted with it.')
    await part(page, 'confirm-delete').click()
    await expect(listRow(page, list)).toHaveCount(0)
    await expect(part(page, 'title')).toHaveText('All')
    await expect(row(page, title)).toHaveCount(0)
  })

  test('TD6: an agent adds a todo and ticks it off; the open window follows', async ({ page }) => {
    const title = unique('From the agent')
    await openTodos(page)
    await show(page, 'all')
    await expect(row(page, title)).toHaveCount(0)
    // the agent calls the app's tools over MCP; a tool tells the open window with app.emit: no reload, no click
    const added = JSON.parse(await callTool(`${ID}__add_todo`, { title, notes: 'Said the agent', flagged: true })) as { id: number; list: string }
    expect(added.list).toBe('Reminders')
    await expect(row(page, title)).toBeVisible()
    await expect(within(page, title, 'flag')).toHaveAttribute('pressed', '')
    expect(await callTool(`${ID}__list_todos`, {})).toContain(title)
    await callTool(`${ID}__complete_todo`, { id: added.id })
    await expect(row(page, title)).toHaveCount(0)
    expect(await callTool(`${ID}__list_todos`, { completed: true })).toContain(title)
  })

  test('TD7: close the window and come back: the todos are still there', async ({ page }) => {
    const title = unique('Remember me')
    await openTodos(page)
    await show(page, 'all')
    await add(page, title)
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await page.getByTestId(`dock-${ID}`).click()
    await show(page, 'all')
    await expect(row(page, title)).toBeVisible()
  })

  test('TD8: the Todos window at its smallest still works', async ({ page }) => {
    const title = unique('In a small window')
    await openTodos(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(420)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // below 640 wide the lists and the todos are one column each: a list picked shows its todos, Back returns to the lists
    await expect(part(page, 'smart-all')).toBeVisible()
    await expect(part(page, 'title')).toBeHidden()
    await show(page, 'all')
    await expect(part(page, 'smart-all')).toBeHidden()
    await add(page, title)
    await expect(newTodo(page)).toBeInViewport()
    await part(page, 'back').click()
    await expect(part(page, 'smart-all')).toBeVisible()
    await expect(part(page, 'title')).toBeHidden()
  })
})
