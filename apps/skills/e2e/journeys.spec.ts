/**
 * Skills key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its window,
 * and its Luau backend, which keeps the skills in its database. Another app reads them over the bus with the public
 * `list_skills` and `get_skill`; here they are called as an agent calls them, through the host's MCP endpoint (each
 * public request is a tool), and answer the same.
 *
 * The journeys are one person's story, one after another on the shared host: add a skill with a file, change it and
 * remove it, and have another app read them. Each skill's name ends in this run's own mark, so what another run or
 * another app's journeys left behind is not in the way.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { expect, frameOf, openDesktop, openFromHome, test } from '../../../e2e/fixtures'
import { callTool } from '../../../e2e/host/pair'

const ID = 'skills'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree, its text area the real textarea
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const area = (page: Page, name: string): Locator => part(page, name).locator('textarea')
// a skill's row, by its name (the row's heading, in its shadow tree)
const rowOf = (page: Page, name: string): Locator => frame(page).getByTestId(/^skills-skill-/).filter({ hasText: name })

const RUN = `${Date.now()}`
const NAME = `release-notes-${RUN}`
const RENAMED = `changelog-${RUN}`
const TEXT = `---\nname: ${NAME}\ndescription: Write release notes from the merged pull requests.\n---\n\n# Release notes\n\nRun scripts/log.sh first.\n`
const FILE = { path: 'scripts/log.sh', text: '#!/bin/sh\ngit log --merges --oneline\n' }

test.describe.configure({ mode: 'serial' })

/** Fill the open skill form: its name, its SKILL.md, and a file at each of `files`. */
async function fill(page: Page, name: string, text: string, files: { path: string; text: string }[] = []): Promise<void> {
  await field(page, 'form-name').fill(name)
  await area(page, 'form-text').fill(text)
  for (const [i, file] of files.entries()) {
    await part(page, 'form-add-file').click()
    await field(page, `form-file-${i}-path`).fill(file.path)
    await area(page, `form-file-${i}-text`).fill(file.text)
  }
}

/** Add a skill from the list, and wait until it is listed. */
async function add(page: Page, name: string, text: string, files: { path: string; text: string }[] = []): Promise<void> {
  await part(page, 'add').click()
  await fill(page, name, text, files)
  await part(page, 'form-save').click()
  await expect(rowOf(page, name)).toBeVisible()
}

/** Remove a skill from its form, answering the question. */
async function remove(page: Page, name: string): Promise<void> {
  await rowOf(page, name).click()
  await part(page, 'form-remove').click()
  await part(page, 'confirm-confirm').click()
  await expect(rowOf(page, name)).toHaveCount(0)
}

test.describe('Skills journeys', () => {
  test('SK1: open Skills from the Home Screen, add a skill with a file, and see it listed by what it is for', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(part(page, 'library')).toBeVisible()

    await part(page, 'add').click()
    // nothing to add until it has a name and its SKILL.md
    await expect(part(page, 'form-save')).toHaveJSProperty('disabled', true)
    // a name that cannot be a folder's is refused, with the reason, and the form stays
    await fill(page, 'release notes', TEXT)
    await part(page, 'form-save').click()
    await expect(part(page, 'form-failure')).toContainText("A skill's name is the name of its folder")
    // and so is a file whose path leaves the skill's folder
    await field(page, 'form-name').fill(NAME)
    await part(page, 'form-add-file').click()
    await field(page, 'form-file-0-path').fill('../outside.sh')
    await part(page, 'form-save').click()
    await expect(part(page, 'form-failure')).toContainText("../outside.sh must be a path in the skill's folder")
    await field(page, 'form-file-0-path').fill(FILE.path)
    await area(page, 'form-file-0-text').fill(FILE.text)
    await part(page, 'form-save').click()

    // the list again, the skill in it by the description of its front matter
    await expect(part(page, 'form')).toHaveCount(0)
    await expect(rowOf(page, NAME)).toHaveJSProperty('subheading', 'Write release notes from the merged pull requests.')

    // it is kept by the host, not by the page: a window opened again shows it, with its file
    await openDesktop(page, { open: [ID] })
    await rowOf(page, NAME).click()
    await expect(field(page, 'form-name')).toHaveValue(NAME)
    await expect(area(page, 'form-text')).toHaveValue(TEXT)
    await expect(field(page, 'form-file-0-path')).toHaveValue(FILE.path)
    await expect(area(page, 'form-file-0-text')).toHaveValue(FILE.text)
    await part(page, 'form-cancel').click()
    await expect(part(page, 'library')).toBeVisible()
  })

  test('SK2: change the skill, its name and its files, and remove it', async ({ page }) => {
    await openDesktop(page, { open: [ID] })
    await add(page, `other-${RUN}`, '# Other')

    // another skill's name is refused
    await rowOf(page, NAME).click()
    await field(page, 'form-name').fill(`other-${RUN}`)
    await part(page, 'form-save').click()
    await expect(part(page, 'form-failure')).toHaveText(`There is a skill named other-${RUN} already.`)

    // renamed, its SKILL.md without front matter, its file taken out
    await field(page, 'form-name').fill(RENAMED)
    await area(page, 'form-text').fill('# Changelog')
    await part(page, 'form-file-0-remove').click()
    await part(page, 'form-save').click()
    await expect(rowOf(page, NAME)).toHaveCount(0)
    await expect(rowOf(page, RENAMED)).toHaveJSProperty('subheading', 'SKILL.md')

    // removing asks first; Cancel keeps it
    await rowOf(page, RENAMED).click()
    await part(page, 'form-remove').click()
    await expect(part(page, 'confirm')).toContainText(`Remove ${RENAMED}?`)
    await part(page, 'confirm-cancel').click()
    await expect(part(page, 'confirm')).toHaveCount(0)
    await part(page, 'form-cancel').click()
    await expect(rowOf(page, RENAMED)).toBeVisible()

    await remove(page, RENAMED)
    await remove(page, `other-${RUN}`)
  })

  test('SK3: another app reads the skills: it lists them by what they are for, and reads one with its files', async ({ page }) => {
    await openDesktop(page, { open: [ID] })
    await add(page, NAME, TEXT, [FILE])
    // a quoted description, and a folded one over two lines
    const quoted = `quoted-${RUN}`
    const folded = `folded-${RUN}`
    await add(page, quoted, '---\ndescription: "Say: it is \\"done\\"."\n---\n# Quoted')
    await add(page, folded, '---\ndescription: >\n  Fold these\n  two lines.\nname: folded\n---\n# Folded')

    // the public `list_skills`, by name, each with the description of its front matter
    const listed = JSON.parse(await callTool(`${ID}__list_skills`, {})) as { skills: { name: string; description: string }[] }
    const mine = listed.skills.filter((s) => s.name.endsWith(RUN))
    expect(mine).toEqual([
      { name: folded, description: 'Fold these two lines.' },
      { name: quoted, description: 'Say: it is "done".' },
      { name: NAME, description: 'Write release notes from the merged pull requests.' }
    ])
    // and `get_skill`: its SKILL.md and its other files
    expect(JSON.parse(await callTool(`${ID}__get_skill`, { name: NAME }))).toEqual({ name: NAME, text: TEXT, files: [FILE] })
    await expect(callTool(`${ID}__get_skill`, { name: `nothing-${RUN}` })).rejects.toThrow(`There is no skill named nothing-${RUN}.`)

    for (const name of [NAME, quoted, folded]) await remove(page, name)
    expect(JSON.parse(await callTool(`${ID}__list_skills`, {})).skills.filter((s: { name: string }) => s.name.endsWith(RUN))).toEqual([])
  })
})
