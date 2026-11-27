/**
 * Notes key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window, and
 * its Luau backend, which keeps the folders and the notes in the app's database and offers them to agents as tools.
 *
 * The notes live on the shared host, so each test names its own (a title no other run has) and finds them by that name.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'notes'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const search = (page: Page): Locator => part(page, 'search').locator('input')
const newFolder = (page: Page): Locator => part(page, 'new-folder-input').locator('input')
const text = (page: Page): Locator => part(page, 'text').locator('textarea')

let serial = 0
const unique = (what: string): string => `${what} ${Date.now().toString(36)}-${++serial}`

/** A note's row in the list, found by its title. */
const row = (page: Page, title: string): Locator => frame(page).locator('ui-row[data-testid^="notes-item-"]').filter({ hasText: title })
/** A folder of the person's own in the sidebar, found by its name. */
const folderRow = (page: Page, name: string): Locator => frame(page).locator('ui-row[data-testid^="notes-folder-"]').filter({ hasText: name })

async function openNotes(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await expect(part(page, 'title')).toHaveText('All Notes')
}

/** Start a note, type it, and wait until the list has it under its title: it is saved. */
async function write(page: Page, body: string): Promise<void> {
  await part(page, 'new').click()
  // the new note is open once its text is empty: until then the text is the note that was open, and what is typed
  // there is lost when the new one opens
  await expect(text(page)).toHaveValue('')
  await expect(text(page)).toBeEditable()
  await text(page).fill(body)
  await expect(row(page, body.split('\n')[0] ?? body)).toBeVisible()
}

async function show(page: Page, name: 'all' | 'deleted', title: string): Promise<void> {
  await part(page, name).click()
  await expect(part(page, 'title')).toHaveText(title)
}

test.describe('Notes journeys', () => {
  test('NT1: open Notes from the Dock and see the folders', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(part(page, 'all')).toBeVisible()
    await expect(folderRow(page, 'Notes')).toBeVisible()
    await expect(part(page, 'deleted')).toBeVisible()
    await expect(part(page, 'title')).toHaveText('All Notes')
    await expect(part(page, 'none')).toBeVisible()
  })

  test('NT2: write a note: its first line is its title, the next its preview, and it is there after another is opened', async ({ page }) => {
    const title = unique('Shopping')
    const other = unique('Ideas')
    await openNotes(page)
    await write(page, `${title}\nMilk and bread`)
    await expect(row(page, title)).toContainText('Milk and bread')
    await expect(part(page, 'edited')).toContainText('Edited')
    await write(page, other)
    await row(page, title).click()
    await expect(text(page)).toHaveValue(`${title}\nMilk and bread`)
  })

  test('NT3: a note left with nothing in it is not kept', async ({ page }) => {
    const title = unique('Kept')
    await openNotes(page)
    await write(page, title)
    const before = await part(page, 'count').textContent()
    await part(page, 'new').click()
    await expect(text(page)).toHaveValue('')
    await expect(part(page, 'count')).not.toHaveText(before ?? '')
    await row(page, title).click()
    await expect(part(page, 'count')).toHaveText(before ?? '')
  })

  test('NT4: pin a note: it goes under Pinned, and comes back out', async ({ page }) => {
    const title = unique('Wifi password')
    await openNotes(page)
    await write(page, title)
    const pinned = part(page, 'section-Pinned').locator('ui-row').filter({ hasText: title })
    await expect(pinned).toHaveCount(0)
    await part(page, 'pin').click()
    await expect(part(page, 'pin')).toHaveAttribute('pressed', '')
    await expect(pinned).toBeVisible()
    await part(page, 'pin').click()
    await expect(pinned).toHaveCount(0)
    await expect(row(page, title)).toBeVisible()
  })

  test('NT5: search finds a note by words inside it, from whatever folder is shown', async ({ page }) => {
    const title = unique('Meeting')
    const word = `budget${Date.now().toString(36)}`
    await openNotes(page)
    await write(page, `${title}\nAsk about the ${word}`)
    await show(page, 'deleted', 'Recently Deleted')
    await search(page).fill(word.toUpperCase())
    await expect(part(page, 'title')).toHaveText('Search')
    await expect(row(page, title)).toBeVisible()
    await expect(part(page, 'count')).toHaveText('1 Note')
    await search(page).fill(`${word} that nobody wrote`)
    await expect(part(page, 'empty')).toContainText('No Results')
  })

  test('NT6: delete a note, find it in Recently Deleted, recover it; then delete it for good', async ({ page }) => {
    const title = unique('Old plan')
    await openNotes(page)
    await write(page, `${title}\nNever mind`)
    await part(page, 'remove').click()
    await expect(row(page, title)).toHaveCount(0)
    await show(page, 'deleted', 'Recently Deleted')
    await expect(part(page, 'new')).toHaveCount(0) // nothing is started among the deleted
    await row(page, title).click()
    await expect(text(page)).toHaveValue(`${title}\nNever mind`)
    await expect(text(page)).not.toBeEditable()
    await part(page, 'restore').click()
    await expect(row(page, title)).toHaveCount(0)
    await show(page, 'all', 'All Notes')
    await row(page, title).click()
    await expect(text(page)).toBeEditable()
    // deleted again, and this time for good: an alert asks first
    await part(page, 'remove').click()
    await show(page, 'deleted', 'Recently Deleted')
    await row(page, title).click()
    await part(page, 'purge').click()
    await expect(part(page, 'confirm-purge')).toContainText('It cannot be brought back.')
    await part(page, 'confirm-purge-confirm').click()
    await expect(row(page, title)).toHaveCount(0)
    await show(page, 'all', 'All Notes')
    await expect(row(page, title)).toHaveCount(0)
  })

  test('NT7: a folder of its own: add it, move a note into it, and delete it; its notes go to Recently Deleted', async ({ page }) => {
    const folder = unique('Recipes')
    const title = unique('Pancakes')
    await openNotes(page)
    await write(page, title)
    await newFolder(page).fill(folder)
    await newFolder(page).press('Enter')
    await expect(part(page, 'title')).toHaveText(folder)
    await expect(folderRow(page, folder)).toHaveAttribute('selected', '')
    // a name a folder has already is said under the field, and cannot be added
    await newFolder(page).fill(folder.toUpperCase())
    await expect(part(page, 'new-folder-problem')).toContainText('There is already a folder called')
    await expect(part(page, 'new-folder-add')).toHaveAttribute('disabled', '')
    await expect(part(page, 'failure')).toHaveCount(0)
    await newFolder(page).fill('')
    await show(page, 'all', 'All Notes')
    await row(page, title).click()
    await part(page, 'move').locator('select').selectOption({ label: folder })
    await expect(folderRow(page, folder)).toHaveAttribute('aria-label', `${folder}, 1`)
    await folderRow(page, folder).click()
    await expect(row(page, title)).toBeVisible()
    await part(page, 'delete-folder').click()
    await expect(part(page, 'confirm-folder')).toContainText('Its notes go to Recently Deleted.')
    await part(page, 'confirm-folder-confirm').click()
    await expect(folderRow(page, folder)).toHaveCount(0)
    await expect(part(page, 'title')).toHaveText('All Notes')
    await expect(row(page, title)).toHaveCount(0)
    await show(page, 'deleted', 'Recently Deleted')
    await expect(row(page, title)).toBeVisible()
  })

  test('NT8: an agent writes a note and adds to it; the open window follows', async ({ page }) => {
    const title = unique('From the agent')
    await openNotes(page)
    await expect(row(page, title)).toHaveCount(0)
    // the agent calls the app's tools over MCP; a tool tells the open window with a notification: no reload, no click
    const made = JSON.parse(await callTool(`${ID}__create_note`, { body: `${title}\nSaid the agent` })) as { id: number; folder: string }
    expect(made.folder).toBe('Notes')
    await expect(row(page, title)).toContainText('Said the agent')
    await row(page, title).click()
    await expect(text(page)).toHaveValue(`${title}\nSaid the agent`)
    await callTool(`${ID}__append_to_note`, { id: made.id, text: 'And one more thing' })
    await expect(text(page)).toHaveValue(`${title}\nSaid the agent\nAnd one more thing`)
    expect(await callTool(`${ID}__list_notes`, { search: 'one more thing' })).toContain(title)
    expect(await callTool(`${ID}__read_note`, { id: made.id })).toContain('And one more thing')
  })

  test('NT9: close the window and come back: the note is still there', async ({ page }) => {
    const title = unique('Remember me')
    await openNotes(page)
    await write(page, `${title}\nEvery word of it`)
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await openFromHome(page, ID)
    await row(page, title).click()
    await expect(text(page)).toHaveValue(`${title}\nEvery word of it`)
  })

  test('NT10: the Notes window at its smallest still works', async ({ page }) => {
    const title = unique('In a small window')
    await openNotes(page)
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(420)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(420)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // under 700 wide (the kit's split view) the folders, the notes and the note are one column each
    await expect(part(page, 'title')).toBeVisible()
    await expect(part(page, 'all')).toBeHidden()
    await part(page, 'new').click()
    await expect(text(page)).toBeEditable()
    await expect(part(page, 'title')).toBeHidden()
    await text(page).fill(title)
    await part(page, 'back-to-list').click()
    await expect(row(page, title)).toBeVisible()
    await part(page, 'back').click()
    await expect(part(page, 'all')).toBeVisible()
    await expect(part(page, 'title')).toBeHidden()
  })
})
