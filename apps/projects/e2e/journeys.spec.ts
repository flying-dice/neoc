/**
 * Projects key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which keeps the projects, their issues and their pages in the app's database and offers them to
 * agents as tools.
 *
 * The projects live on the shared host, so each test makes its own (a name and a key no other run has) and finds what
 * it made by name.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'projects'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')

let serial = 0
const unique = (what: string): string => `${what} ${Date.now().toString(36)}-${++serial}`
/** A key no other run has: a letter, then four of the 36 digits of the clock and the count. */
const uniqueKey = (): string => `K${((Date.now() + ++serial * 7919) % 36 ** 4).toString(36).toUpperCase().padStart(4, '0')}`

/** A project in the sidebar, found by its name. */
const projectRow = (page: Page, name: string): Locator => frame(page).locator('ui-row[data-testid^="projects-project-"]').filter({ hasText: name })
/** An issue's card on the board, found by its title. */
const card = (page: Page, title: string): Locator => frame(page).locator('li[data-testid^="projects-card-"]').filter({ hasText: title })
const column = (page: Page, status: 'todo' | 'doing' | 'done'): Locator => part(page, `column-${status}`)
/** A page in the list of pages, found by its title. */
const pageRow = (page: Page, title: string): Locator => frame(page).locator('ui-row[data-testid^="projects-page-"]').filter({ hasText: title })

async function openProjects(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'app')).toBeVisible()
  await expect(part(page, 'new')).toBeVisible()
}

/** A project of its own for a journey, shown as soon as it is added. */
async function addProject(page: Page, name: string, key: string): Promise<void> {
  await part(page, 'new').click()
  await field(page, 'form-name').fill(name)
  await field(page, 'form-key').fill(key)
  await part(page, 'form-add').click()
  await expect(part(page, 'title')).toHaveText(name)
  await expect(part(page, 'key')).toHaveText(key)
}

async function addIssue(page: Page, title: string): Promise<void> {
  await field(page, 'new-issue-input').fill(title)
  await field(page, 'new-issue-input').press('Enter')
  await expect(column(page, 'todo').locator('li').filter({ hasText: title })).toBeVisible()
}

test.describe('Projects journeys', () => {
  test('PJ1: open Projects from the Dock, and start a project', async ({ page }) => {
    const name = unique('Website')
    const key = uniqueKey()
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await part(page, 'new').click()
    // nothing to add yet: the button cannot be pressed, and that is not an error
    await expect(part(page, 'form-add')).toHaveJSProperty('disabled', true)
    await field(page, 'form-name').fill(name)
    // the key follows the name until one is typed
    await expect(field(page, 'form-key')).not.toHaveValue('')
    await field(page, 'form-key').fill(key)
    await part(page, 'form-add').click()
    await expect(part(page, 'title')).toHaveText(name)
    await expect(projectRow(page, name)).toHaveAttribute('selected', '')
    for (const status of ['todo', 'doing', 'done'] as const) await expect(column(page, status)).toBeVisible()
  })

  test('PJ2: a key another project has is said at the form, and a key that is not one before it is sent', async ({ page }) => {
    const key = uniqueKey()
    await openProjects(page)
    await addProject(page, unique('First'), key)
    await part(page, 'new').click()
    await field(page, 'form-name').fill(unique('Second'))
    await field(page, 'form-key').fill('1')
    await field(page, 'form-key').press('Tab')
    await expect(part(page, 'form')).toHaveJSProperty('footerTone', 'error')
    await expect(part(page, 'form')).toHaveJSProperty('footer', 'A key is two to five letters or digits, a letter first, like WEB.')
    await expect(part(page, 'form-add')).toHaveJSProperty('disabled', true)
    await field(page, 'form-key').fill(key)
    await expect(part(page, 'form')).toHaveJSProperty('footerTone', 'note')
    await part(page, 'form-add').click()
    await expect(part(page, 'form')).toHaveJSProperty('footer', `Another project has the key ${key}.`)
    await expect(part(page, 'failure')).toHaveCount(0)
    await part(page, 'form-cancel').click()
    await expect(part(page, 'form')).toHaveCount(0)
  })

  test('PJ3: raise an issue, move it across the board, and change it', async ({ page }) => {
    const key = uniqueKey()
    const title = unique('Fix the footer')
    await openProjects(page)
    await addProject(page, unique('Board'), key)
    await addIssue(page, title)
    await expect(field(page, 'new-issue-input')).toHaveValue('') // ready for the next
    await expect(card(page, title)).toContainText(`${key}-1`)
    // the button on the card moves it one column on
    await card(page, title).locator('ui-button').click()
    await expect(column(page, 'doing').locator('li').filter({ hasText: title })).toBeVisible()
    await expect(part(page, 'column-todo-count')).toHaveText('0')
    // open it: its column, its kind and its priority are choices; its title is typed over
    await card(page, title).getByRole('button', { name: title }).click()
    await expect(part(page, 'issue-key')).toHaveText(`${key}-1`)
    await part(page, 'issue-status-done').click()
    await part(page, 'issue-type-bug').click()
    await part(page, 'issue-priority-high').click()
    await field(page, 'issue-title').fill(`${title}, for good`)
    await field(page, 'issue-title').press('Enter')
    await part(page, 'issue-description').fill('The links go nowhere.')
    await part(page, 'issue-description').press('Tab')
    await part(page, 'issue-back').click()
    await expect(column(page, 'done').locator('li').filter({ hasText: `${title}, for good` })).toContainText('High')
    // the list of issues says the same
    await part(page, 'tab-issues').click()
    const row = frame(page).locator('ui-row[data-testid^="projects-issue-"]').filter({ hasText: `${title}, for good` })
    await expect(row).toHaveJSProperty('subheading', `${key}-1 · Bug · High`)
    await expect(row).toHaveJSProperty('value', 'Done')
    await row.click()
    await expect(part(page, 'issue-description')).toHaveValue('The links go nowhere.')
    // deleting asks first
    await part(page, 'issue-remove').click()
    await part(page, 'issue-confirm-delete').click()
    await expect(part(page, 'issues-empty')).toBeVisible()
  })

  test('PJ4: drag a card to another column', async ({ page }) => {
    const title = unique('Drag me')
    await openProjects(page)
    await addProject(page, unique('Dragging'), uniqueKey())
    await addIssue(page, title)
    await card(page, title).dragTo(column(page, 'done'))
    await expect(column(page, 'done').locator('li').filter({ hasText: title })).toBeVisible()
    await expect(part(page, 'column-done-count')).toHaveText('1')
  })

  test('PJ5: write a page, a page under it, and delete both', async ({ page }) => {
    const title = unique('Design')
    const under = unique('Colours')
    await openProjects(page)
    await addProject(page, unique('Writing'), uniqueKey())
    await part(page, 'tab-pages').click()
    await expect(part(page, 'pages-empty')).toBeVisible()
    await field(page, 'new-page-input').fill(title)
    await field(page, 'new-page-input').press('Enter')
    // a new page opens ready to be written
    await part(page, 'page-text').fill('How it should look.\n\nCalm and plain.')
    await part(page, 'page-done').click()
    await expect(part(page, 'page-title')).toHaveText(title)
    await expect(part(page, 'page-body')).toHaveText('How it should look.\n\nCalm and plain.')
    // Cancel drops what was typed
    await part(page, 'page-edit').click()
    await part(page, 'page-text').fill('Thrown away.')
    await part(page, 'page-cancel').click()
    await expect(part(page, 'page-body')).toContainText('Calm and plain.')
    // a page named here goes under it
    await field(page, 'new-subpage-input').fill(under)
    await field(page, 'new-subpage-input').press('Enter')
    await part(page, 'page-done').click()
    await expect(part(page, 'page-path')).toHaveText(title)
    await part(page, 'page-back').click()
    await expect(part(page, 'page-title')).toHaveText(title)
    await expect(frame(page).locator('ui-row[data-testid^="projects-subpage-"]').filter({ hasText: under })).toBeVisible()
    await part(page, 'page-back').click()
    await expect(pageRow(page, under)).toHaveJSProperty('subheading', `In ${title}`)
    // deleting the page takes the page under it
    await pageRow(page, title).first().click()
    await part(page, 'page-remove').click()
    await expect(part(page, 'page-confirm')).toContainText('The pages under it are deleted with it.')
    await part(page, 'page-confirm-delete').click()
    await expect(part(page, 'pages-empty')).toBeVisible()
  })

  test('PJ6: an agent raises an issue, moves it and writes a page; the open window follows', async ({ page }) => {
    const key = uniqueKey()
    const title = unique('From the agent')
    const written = unique('Notes')
    await openProjects(page)
    await addProject(page, unique('Agents'), key)
    // the agent calls the app's tools over MCP; a tool tells the open window with app.emit: no reload, no click
    const added = JSON.parse(await callTool(`${ID}__add_issue`, { project: key, title, type: 'bug', priority: 'high' })) as { key: string }
    expect(added.key).toBe(`${key}-1`)
    await expect(column(page, 'todo').locator('li').filter({ hasText: title })).toBeVisible()
    expect(await callTool(`${ID}__list_issues`, { project: key })).toContain(title)
    await callTool(`${ID}__move_issue`, { key: added.key, status: 'done' })
    await expect(column(page, 'done').locator('li').filter({ hasText: title })).toBeVisible()
    await callTool(`${ID}__write_page`, { project: key, title: written, body: 'Said the agent.' })
    await part(page, 'tab-pages').click()
    await pageRow(page, written).click()
    await expect(part(page, 'page-body')).toHaveText('Said the agent.')
    // the page is rewritten while it is open: its text follows
    await callTool(`${ID}__write_page`, { project: key, title: written, body: 'Said the agent, again.' })
    await expect(part(page, 'page-body')).toHaveText('Said the agent, again.')
    expect(await callTool(`${ID}__read_page`, { project: key, title: written })).toContain('Said the agent, again.')
    expect(await callTool(`${ID}__list_pages`, { project: key })).toContain(written)
  })

  test('PJ7: close the window and come back: the project is still there, and can be deleted', async ({ page }) => {
    const name = unique('Remember me')
    const title = unique('Still here')
    await openProjects(page)
    await addProject(page, name, uniqueKey())
    await addIssue(page, title)
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await openFromHome(page, ID)
    await projectRow(page, name).click()
    await expect(card(page, title)).toBeVisible()
    await part(page, 'delete').click()
    await expect(part(page, 'confirm')).toContainText('Its issues and its pages are deleted with it.')
    await part(page, 'confirm-delete').click()
    await expect(projectRow(page, name)).toHaveCount(0)
  })

  test('PJ8: the Projects window at its smallest still works', async ({ page }) => {
    const name = unique('Small')
    const title = unique('In a small window')
    await openProjects(page)
    await addProject(page, name, uniqueKey())
    await resizeWindow(page, ID, 100, 100)
    const win = (await windowOf(page, ID).boundingBox())!
    expect(Math.round(win.width), 'the minimum width of the contract').toBe(420)
    expect(Math.round(win.height), 'the minimum height of the contract').toBe(480)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    // below 720 wide the projects and the one that is shown are one column each
    await expect(part(page, 'title')).toBeVisible()
    await expect(part(page, 'new')).toBeHidden()
    await addIssue(page, title)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await part(page, 'back').click()
    await expect(projectRow(page, name)).toBeVisible()
    await expect(part(page, 'title')).toBeHidden()
  })
})
