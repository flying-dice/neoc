/**
 * Notebooks key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which keeps the notebooks and their notes in the app's database, reads each note's links and tags
 * from its text, and offers the notes to agents as tools.
 *
 * The notes live on the shared host, so each test names its own (a title no other run has) and finds them by that name.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'notebooks'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const title = (page: Page): Locator => part(page, 'title').locator('input')
const search = (page: Page): Locator => part(page, 'search').locator('input')
const editor = (page: Page): Locator => part(page, 'editor')

let serial = 0
const unique = (what: string): string => `${what} ${Date.now().toString(36)}-${++serial}`

const escaped = (text: string): string => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
/** A note's row in the list, found by its title: a row is named with its title first, then its first line and when it changed. */
const row = (page: Page, name: string): Locator =>
  frame(page)
    .locator('ui-row[data-testid^="notebooks-note-"]')
    .and(frame(page).getByRole('option', { name: new RegExp(`^${escaped(name)}(,|$)`) }))
/** A link to a note in the reading view, found by the title it names. */
const link = (page: Page, name: string): Locator => part(page, `reading-link-${name}`)
/** A note that links to the open one, found by its title. */
const backlink = (page: Page, name: string): Locator => part(page, 'backlinks').getByRole('button').filter({ hasText: name })

async function openNotebooks(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await expect(part(page, 'new')).toBeVisible()
}

/** Make a note called `name` that says `body`, through the quick switcher, and wait until it is saved. */
async function write(page: Page, name: string, body: string): Promise<void> {
  await part(page, 'switch').click()
  await part(page, 'switcher-input').fill(name)
  await part(page, 'switcher-create').click()
  await expect(title(page)).toHaveValue(name)
  await editor(page).fill(body)
  await expect(part(page, 'status')).toHaveText('Saved')
}

async function read(page: Page): Promise<void> {
  await part(page, 'mode-read').click()
  await expect(part(page, 'reading')).toBeVisible()
}

test.describe('Notebooks journeys', () => {
  test('NB1: open Notebooks from the Dock and see the notebook', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(part(page, 'notebook')).toContainText('Notebook')
    for (const name of ['new', 'daily', 'switch', 'graph-toggle', 'search']) await expect(part(page, name)).toBeVisible()
    await expect(part(page, 'none')).toBeVisible()
  })

  test('NB2: start a note, write it in Markdown, and read it', async ({ page }) => {
    const name = unique('Garden plan')
    await openNotebooks(page)
    await part(page, 'new').click()
    await expect(title(page)).toHaveValue(/^Untitled/)
    await title(page).fill(name)
    await title(page).press('Enter')
    await expect(row(page, name)).toBeVisible()
    await editor(page).fill('# The beds\n\nSow in **March**.\n\n- [ ] Order the seeds\n- [x] Clear the bed')
    await expect(part(page, 'status')).toHaveText('Saved')
    await expect(row(page, name)).toHaveJSProperty('subheading', 'Sow in March.') // the list shows its first line, without its marks
    await read(page)
    await expect(part(page, 'reading').getByRole('heading', { name: 'The beds' })).toBeVisible()
    await expect(part(page, 'reading-task-4')).not.toBeChecked()
    // a task ticked as the note reads is ticked in what it says
    await part(page, 'reading-task-4').click()
    await expect(part(page, 'reading-task-4')).toBeChecked()
    await part(page, 'mode-edit').click()
    await expect(editor(page)).toHaveValue(/- \[x\] Order the seeds/)
  })

  test('NB3: a link leads to its note, makes the note when there is none, and the note says what links to it', async ({ page }) => {
    const plan = unique('Plan')
    const herbs = unique('Herbs')
    await openNotebooks(page)
    await write(page, plan, `The bed by the door: [[${herbs}|the herb bed]].`)
    await read(page)
    await expect(link(page, herbs)).toHaveText('the herb bed')
    await expect(link(page, herbs)).toHaveAttribute('title', `${herbs} is not written yet`)
    // following it makes the note, open to be written, and it knows where it was named
    await link(page, herbs).click()
    await expect(title(page)).toHaveValue(herbs)
    await expect(editor(page)).toBeVisible()
    await expect(backlink(page, plan)).toContainText('the herb bed')
    await backlink(page, plan).click()
    await expect(title(page)).toHaveValue(plan)
    await read(page)
    await expect(link(page, herbs)).toHaveAttribute('title', herbs) // the note is there now
  })

  test('NB4: renaming a note rewrites the links to it; a title another note has is said at the field', async ({ page }) => {
    const plan = unique('Plan')
    const herbs = unique('Herbs')
    const renamed = `${herbs} bed`
    await openNotebooks(page)
    await write(page, herbs, 'Basil and thyme.')
    await write(page, plan, `See [[${herbs}]].`)
    await title(page).fill(herbs)
    await title(page).press('Enter')
    await expect(part(page, 'title-problem')).toHaveText(`There is already a note called ${herbs}.`)
    await expect(part(page, 'failure')).toHaveCount(0)
    await title(page).fill(plan)
    await title(page).press('Enter')
    await expect(part(page, 'title-problem')).toHaveCount(0)
    await row(page, herbs).click()
    await expect(title(page)).toHaveValue(herbs)
    await title(page).fill(renamed)
    await title(page).press('Enter')
    await expect(row(page, renamed)).toBeVisible()
    await backlink(page, plan).click()
    await expect(editor(page)).toHaveValue(`See [[${renamed}]].`)
  })

  test('NB5: tags narrow the list, and a search finds words in what the notes say', async ({ page }) => {
    const tag = `t${Date.now().toString(36)}${++serial}`
    const tagged = unique('Tagged')
    const other = unique('Other')
    const word = `zyx${Date.now().toString(36)}`
    await openNotebooks(page)
    await write(page, tagged, `About the beds. #${tag}`)
    await write(page, other, `Nothing of the sort, but ${word} is here.`)
    await part(page, `tag-${tag}`).click()
    await expect(part(page, `tag-${tag}`)).toHaveAttribute('aria-pressed', 'true')
    await expect(row(page, tagged)).toBeVisible()
    await expect(row(page, other)).toHaveCount(0)
    await part(page, `tag-${tag}`).click()
    await expect(row(page, other)).toBeVisible()
    await search(page).fill(word.toUpperCase())
    const hit = frame(page).locator('ui-row[data-testid^="notebooks-hit-"]')
    await expect(hit).toHaveCount(1)
    await expect(hit).toHaveJSProperty('heading', other)
    await expect(hit).toHaveJSProperty('subheading', `Nothing of the sort, but ${word} is here.`)
    await hit.click()
    await expect(title(page)).toHaveValue(other)
    await search(page).fill('')
    await expect(row(page, tagged)).toBeVisible()
  })

  test('NB6: the graph shows the notes and their links, and a dot opens its note', async ({ page }) => {
    const a = unique('Alpha')
    const b = unique('Beta')
    await openNotebooks(page)
    await write(page, a, 'The first.')
    await write(page, b, `Linked to [[${a}]].`)
    await part(page, 'graph-toggle').click()
    await expect(part(page, 'graph')).toBeVisible()
    await expect(part(page, 'graph-count')).toContainText(/\d+ notes?, \d+ links?/)
    await part(page, 'graph').getByRole('button', { name: a }).click()
    await expect(title(page)).toHaveValue(a)
    await expect(backlink(page, b)).toBeVisible()
  })

  test("NB7: today's note is named after the day, and is the same note the next time", async ({ page }) => {
    await openNotebooks(page)
    const today = await frame(page)
      .locator('body')
      .evaluate(() => {
        const now = new Date()
        return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
      })
    await part(page, 'daily').click()
    await expect(title(page)).toHaveValue(today)
    await part(page, 'new').click()
    await expect(title(page)).toHaveValue(/^Untitled/)
    await part(page, 'daily').click()
    await expect(title(page)).toHaveValue(today)
    await expect(row(page, today)).toHaveCount(1)
  })

  test('NB8: bookmark a note, then delete it after the alert asks', async ({ page }) => {
    const name = unique('Kept for now')
    await openNotebooks(page)
    await write(page, name, 'Soon gone.')
    await part(page, 'pin').click()
    await expect(part(page, 'pin')).toHaveAttribute('pressed', '')
    await expect(row(page, name)).toHaveJSProperty('icon', 'star')
    await part(page, 'delete').click()
    await expect(part(page, 'confirm')).toContainText(`Delete ${name}?`)
    await part(page, 'confirm-cancel').click()
    await expect(row(page, name)).toBeVisible()
    await part(page, 'delete').click()
    await part(page, 'confirm-delete').click()
    await expect(row(page, name)).toHaveCount(0)
    await expect(part(page, 'none')).toBeVisible()
  })

  test('NB9: a notebook of its own: add it, write in it, and delete it with its notes', async ({ page }) => {
    const notebook = unique('Recipes')
    const name = unique('Bread')
    await openNotebooks(page)
    await part(page, 'add-notebook').click()
    const field = part(page, 'new-notebook-input').locator('input')
    await field.fill('notebook')
    await field.press('Enter')
    await expect(part(page, 'new-notebook-problem')).toHaveText('There is already a notebook called notebook.')
    await field.fill(notebook)
    await field.press('Enter')
    await expect(part(page, 'notebook').locator('option:checked')).toHaveText(notebook)
    await expect(part(page, 'empty')).toBeVisible()
    await write(page, name, 'Flour, water, salt.')
    await part(page, 'delete-notebook').click()
    await expect(part(page, 'confirm-notebook')).toContainText('Its notes are deleted with it.')
    await part(page, 'confirm-notebook-delete').click()
    await expect(part(page, 'notebook').locator('option:checked')).toHaveText('Notebook')
    await expect(part(page, 'notebook').locator('option', { hasText: notebook })).toHaveCount(0)
    await expect(row(page, name)).toHaveCount(0)
  })

  test('NB10: an agent writes a note, reads one and searches; the open window follows', async ({ page }) => {
    const mine = unique('Mine')
    const theirs = unique('From the agent')
    await openNotebooks(page)
    await write(page, mine, 'What I wrote.')
    await expect(row(page, theirs)).toHaveCount(0)
    // the agent calls the app's tools over MCP; a tool tells the open window with app.emit: no reload, no click
    const written = JSON.parse(await callTool(`${ID}__write_note`, { title: theirs, body: `Said the agent, about [[${mine}]]. #agent` })) as {
      id: number
      created: boolean
    }
    expect(written.created).toBe(true)
    await expect(row(page, theirs)).toBeVisible()
    await expect(backlink(page, theirs)).toContainText('Said the agent')
    expect(await callTool(`${ID}__read_note`, { title: mine.toUpperCase() })).toContain('What I wrote.')
    expect(await callTool(`${ID}__list_notes`, { tag: 'agent' })).toContain(theirs)
    expect(await callTool(`${ID}__search_notes`, { query: 'said the agent' })).toContain(theirs)
    // added after what the note says, and the window that has it open reads it again
    await row(page, theirs).click()
    await expect(title(page)).toHaveValue(theirs)
    await callTool(`${ID}__write_note`, { title: theirs, body: 'And one more line.', append: true })
    await expect(editor(page)).toHaveValue(`Said the agent, about [[${mine}]]. #agent\n\nAnd one more line.`)
  })

  test('NB11: close the window and come back: the notes are still there', async ({ page }) => {
    const name = unique('Remember me')
    await openNotebooks(page)
    await write(page, name, 'Still here.')
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await openFromHome(page, ID)
    await row(page, name).click()
    await expect(editor(page)).toHaveValue('Still here.')
  })

  test('NB12: the Notebooks window at its smallest still works', async ({ page }) => {
    const name = unique('In a small window')
    await openNotebooks(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(420)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // below 700 wide the notes and the note are one column each: a note picked shows it, Back returns to the notes
    await expect(part(page, 'new')).toBeVisible()
    await expect(part(page, 'none')).toBeHidden()
    await write(page, name, 'Written small.')
    await expect(part(page, 'new')).toBeHidden()
    await expect(editor(page)).toBeInViewport()
    await part(page, 'back').click()
    await expect(row(page, name)).toBeVisible()
    await expect(editor(page)).toBeHidden()
  })
})
