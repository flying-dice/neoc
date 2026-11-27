/**
 * Files journeys, on the real host: the Files page browses the documents the installed apps keep and the host's own
 * file system ("This Computer"), downloads a file (`GET /files/...`), uploads one (`PUT /files/...`), makes a folder
 * and removes entries after a confirmation (`host.files.*`).
 *
 * **Where these write.** This Computer is the whole disk of the machine the specs run on, so every journey that
 * changes anything there works only inside a directory it makes itself under the run's own folder (`FILES_DIR`,
 * test-results/host/files), which it reaches by typing its path, and never opens or changes anything else.
 *
 * The apps' documents come from the React task-list fixture (e2e/apps/tasks-react), installed at start: its
 * `_save_document` method writes a file with `app.files`, which is how any app's documents come to be. (The fixture the
 * Discover journeys install and remove is not used: they run while these do.) Each journey works in a folder of its
 * own and removes it at the end, so the journeys do not see each other's files and the app is left as it was found.
 */
import { execFileSync } from 'node:child_process'
import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { type Locator, type Page } from '@playwright/test'
import { expect, expectAccessible, expectNoOverflow, openDesktop, resizeWindow, settle, test, windowOf } from './fixtures'
import { FILES_DIR, ORIGIN, portable } from './host/config'

const APP = 'tasks-react'

/** A host call the shell makes (`window.__vos.host.api`). */
interface HostApi {
  call(method: string, params?: Record<string, unknown>): Promise<unknown>
}
const hostCall = <T>(page: Page, method: string, params: Record<string, unknown>): Promise<T> =>
  page.evaluate(
    ([m, p]) =>
      (window as unknown as { __vos: { host: { api: HostApi } } }).__vos.host.api.call(m as string, p as Record<string, unknown>),
    [method, params] as const
  ) as Promise<T>

/** The app writes a document, as its own backend would at any time. */
const save = (page: Page, path: string, text: string): Promise<unknown> =>
  hostCall(page, 'app.call', { app: APP, method: '_save_document', args: { path, text } })

const namesIn = async (page: Page, path: string): Promise<string[]> =>
  (await hostCall<{ entries: { name: string }[] }>(page, 'host.files.list', { app: APP, path })).entries.map((e) => e.name)

const entry = (page: Page, name: string): Locator => page.getByTestId(`files-entry-${name}`)

let journeys = 0
/** A name no other journey uses. */
const ownName = (): string => `journey-${Date.now().toString(36)}-${++journeys}`

/** The desktop with `files` written into a folder of this journey in the app's Documents, and the Files window open on it. */
async function openAppFolder(page: Page, files: Record<string, string>): Promise<string> {
  const folder = ownName()
  await openDesktop(page)
  for (const [path, text] of Object.entries(files)) await save(page, `${folder}/${path}`, text)
  await openDesktop(page, { open: ['files'] })
  await page.getByTestId(`files-app-${APP}`).click()
  await entry(page, folder).click()
  await expect(page.getByTestId('files-nav-back')).toBeVisible()
  return folder
}

const removeAppFolder = async (page: Page, folder: string): Promise<void> => {
  await hostCall(page, 'host.files.remove', { app: APP, path: folder, recursive: true })
}

/**
 * A directory of this journey on the machine's disk, holding `files` (a name ending in `/` is a folder), and the Files window
 * open on it: This Computer, then its path typed into the path field.
 */
async function openOwnDirectory(page: Page, files: Record<string, string> = {}): Promise<string> {
  const dir = join(FILES_DIR, ownName())
  mkdirSync(dir, { recursive: true })
  for (const [name, text] of Object.entries(files)) {
    if (name.endsWith('/')) mkdirSync(join(dir, name), { recursive: true })
    else {
      mkdirSync(join(dir, name, '..'), { recursive: true })
      writeFileSync(join(dir, name), text)
    }
  }
  await openDesktop(page, { open: ['files'] })
  await page.getByTestId('files-place-computer').click()
  await goTo(page, dir)
  return dir
}

/**
 * A directory of the disk as the Files page and the host say it: from `/`, with `/` between the names, on every platform.
 * On Unix that is the path itself; on Windows the drive is the first name (`C:\Users\me` is `/C:/Users/me`).
 */
const shown = (path: string): string => (process.platform === 'win32' ? `/${portable(path)}` : path)

/**
 * Take from this user the right to open a directory, or give it back. On Unix its mode says that; on Windows a mode says
 * nothing of the kind, and an entry of the directory's access list does: one that denies everyone the reading of it.
 */
function lock(dir: string, locked: boolean): void {
  if (process.platform !== 'win32') return chmodSync(dir, locked ? 0o000 : 0o700)
  const everyone = '*S-1-1-0'
  execFileSync('icacls', locked ? [dir, '/deny', `${everyone}:(RD)`] : [dir, '/remove:d', everyone], { stdio: 'ignore' })
}

/** Type a directory of the disk into the path field and go there. */
async function goTo(page: Page, dir: string): Promise<void> {
  await page.getByTestId('files-path').fill(shown(dir))
  await page.getByTestId('files-path').press('Enter')
  await expect(page.getByTestId('files-nav')).toContainText(basename(dir))
}

test.describe('Files journeys: an app’s documents', () => {
  test('F1: find a document an app saved: the app, its folders, the file with its size and date', async ({ page }) => {
    const folder = ownName()
    await openDesktop(page)
    await save(page, `${folder}/reports/2026/summary.md`, 'x'.repeat(1500))
    await save(page, `${folder}/notes.txt`, 'hello')
    await openDesktop(page, { open: ['files'] })

    // the app that keeps documents says how many things are at the top of them
    const app = page.getByTestId(`files-app-${APP}`)
    await expect(app).toContainText(/\d+ items?/)
    await app.click()
    await expect(page.getByTestId('files-nav-back')).toContainText('Files')
    await entry(page, folder).click()

    // a folder comes before a file, and a file says its size and when it changed
    await expect(entry(page, 'reports')).toBeVisible()
    await expect(entry(page, 'notes.txt')).toContainText('5 bytes')
    await expect(entry(page, 'notes.txt')).toContainText(/\d{4}/)
    await entry(page, 'reports').click()
    await entry(page, '2026').click()
    await expect(entry(page, 'summary.md')).toContainText('1.5 KB')
    await expect(page.getByTestId('files-nav')).toContainText('2026')

    // and back up, a level at a time, to the places
    await page.getByTestId('files-nav-back').click()
    await expect(entry(page, '2026')).toBeVisible()
    await page.getByTestId('files-nav-back').click()
    await expect(entry(page, 'notes.txt')).toBeVisible()
    await page.getByTestId('files-nav-back').click()
    await page.getByTestId('files-nav-back').click()
    await expect(page.getByTestId(`files-app-${APP}`)).toBeVisible()
    await removeAppFolder(page, folder)
  })

  test('F2: search the open folder by part of a name, then clear the search', async ({ page }) => {
    const folder = await openAppFolder(page, { 'refactor-plan.md': 'a', 'uptime-report.csv': 'b', 'inbox/digest.txt': 'c' })
    await expect(entry(page, 'uptime-report.csv')).toBeVisible()
    await page.getByTestId('files-search').fill('REFACTOR')
    await expect(entry(page, 'refactor-plan.md')).toBeVisible()
    await expect(entry(page, 'uptime-report.csv')).toHaveCount(0)
    await expect(entry(page, 'inbox')).toHaveCount(0)
    await page.getByTestId('files-search').fill('nothing like it')
    await expect(page.getByTestId('files-no-results')).toBeVisible()
    await page.getByTestId('files-search-clear').click()
    await expect(page.getByTestId('files-search')).toHaveValue('')
    await expect(entry(page, 'uptime-report.csv')).toBeVisible()
    await expect(entry(page, 'inbox')).toBeVisible()
    await removeAppFolder(page, folder)
  })

  test('F3: download a file; a page an app wrote comes as a file to save, never as a page of the host', async ({ page }) => {
    const hostile = '<script>document.title = "ran"</script>'
    const folder = await openAppFolder(page, { 'report été.csv': 'a,b\n1,2\n', 'page.html': hostile })

    const [download] = await Promise.all([page.waitForEvent('download'), entry(page, 'report été.csv').click()])
    expect(download.suggestedFilename()).toBe('report été.csv')
    expect(readFileSync(await download.path(), 'utf8')).toBe('a,b\n1,2\n')
    await expect(windowOf(page, 'files'), 'the shell stayed where it was').toBeVisible()

    const [saved] = await Promise.all([page.waitForEvent('download'), entry(page, 'page.html').click()])
    expect(readFileSync(await saved.path(), 'utf8')).toBe(hostile)
    const res = await page.request.get(`/files/app/${APP}/${folder}/page.html`)
    expect(res.status()).toBe(200)
    expect(res.headers()['content-disposition']).toContain('attachment')
    expect(res.headers()['x-content-type-options']).toBe('nosniff')
    expect(res.headers()['content-type']).toBe('application/octet-stream')
    await removeAppFolder(page, folder)
  })

  test('F4: remove a file after confirming, keep it when the alert is cancelled, and a folder says what goes with it', async ({ page }) => {
    const folder = await openAppFolder(page, { 'keep.txt': 'k', 'old.txt': 'o', 'drafts/a.txt': 'a', 'drafts/b.txt': 'b' })

    await page.getByTestId('files-entry-old.txt-remove').click()
    const alert = page.getByTestId('files-remove-alert')
    await expect(alert).toContainText('Remove “old.txt”?')
    await page.getByTestId('files-remove-alert-action-cancel').click()
    await expect(alert).toHaveCount(0)
    await expect(entry(page, 'old.txt')).toBeVisible()
    expect(await namesIn(page, folder)).toContain('old.txt')

    await page.getByTestId('files-entry-old.txt-remove').click()
    await page.getByTestId('files-remove-alert-action-remove').click()
    await expect(entry(page, 'old.txt')).toHaveCount(0)
    await expect(entry(page, 'keep.txt')).toBeVisible()
    expect(await namesIn(page, folder)).toEqual(['drafts', 'keep.txt'])

    // a folder that is not empty: the question says how much is in it, and then it goes with all of that
    await page.getByTestId('files-entry-drafts-remove').click()
    await expect(alert).toContainText('the 2 items in it')
    await page.getByTestId('files-remove-alert-action-remove').click()
    await expect(entry(page, 'drafts')).toHaveCount(0)
    expect(await namesIn(page, folder)).toEqual(['keep.txt'])
    await removeAppFolder(page, folder)
  })

  test('F5: an app that keeps no documents says so', async ({ page }, testInfo) => {
    await openDesktop(page, { open: ['files'] })
    const app = page.getByTestId('files-app-tasks-svelte')
    await expect(app).toContainText('No documents')
    await app.click()
    await expect(page.getByTestId('files-empty')).toContainText('has not saved any files')
    await expectAccessible(page, testInfo, { include: '[data-testid="window-files"]' })
  })

  test('F6: browse files in the smallest window', async ({ page }) => {
    const long = 'a-very-long-file-name-that-an-app-chose-without-thinking-about-narrow-windows.txt'
    const folder = await openAppFolder(page, { [long]: 'x', 'short.txt': 'y' })
    await resizeWindow(page, 'files', 100, 100)
    const win = (await windowOf(page, 'files').boundingBox())!
    expect(Math.round(win.width), 'the shell minimum width').toBe(320)
    await expectNoOverflow(page.getByTestId('window-files-content'))
    for (const name of [long, 'short.txt']) {
      await entry(page, name).scrollIntoViewIfNeeded()
      await expect(entry(page, name)).toBeVisible()
      await expect(page.getByTestId(`files-entry-${name}-remove`)).toBeVisible()
    }
    await page.getByTestId('files-nav-back').click()
    await page.getByTestId('files-nav-back').click()
    await settle(page)
    await expectNoOverflow(page.getByTestId('window-files-content'))
    await removeAppFolder(page, folder)
  })

  test('F7: a browser that is not paired can neither download nor upload', async ({ page, playwright }) => {
    const folder = await openAppFolder(page, { 'private.txt': 'for paired devices only' })
    const dir = join(FILES_DIR, ownName())
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'private.txt'), 'on the disk')
    // no cookies of its own: the project's paired storage state is not this stranger's
    const stranger = await playwright.request.newContext({ baseURL: ORIGIN, storageState: { cookies: [], origins: [] } })
    for (const url of [`/files/app/${APP}/${folder}/private.txt`, `/files/host${shown(dir)}/private.txt`]) {
      const read = await stranger.get(url)
      expect(read.status(), url).toBe(401)
      expect(await read.text()).not.toMatch(/paired devices only|on the disk/)
      expect((await stranger.put(url.replace('private', 'planted'), { data: 'planted' })).status(), url).toBe(401)
    }
    await stranger.dispose()
    expect(existsSync(join(dir, 'planted.txt'))).toBe(false)
    // the paired one can
    expect(await (await page.request.get(`/files/app/${APP}/${folder}/private.txt`)).text()).toBe('for paired devices only')
    expect(await (await page.request.get(`/files/host${shown(dir)}/private.txt`)).text()).toBe('on the disk')
    await removeAppFolder(page, folder)
    rmSync(dir, { recursive: true })
  })
})

test.describe('Files journeys: this computer', () => {
  test('F8: go to a folder on the disk by its path, look around, and walk back up', async ({ page }) => {
    const dir = await openOwnDirectory(page, { 'readme.txt': 'hello disk', 'photos/': '', 'photos/cat.jpg': 'meow' })
    const elsewhere = join(FILES_DIR, ownName())
    mkdirSync(elsewhere)
    writeFileSync(join(elsewhere, 'far.txt'), 'far away')
    // On Windows a link to a folder that anyone may make is a junction (a symbolic link needs a right most accounts lack)
    const link = process.platform === 'win32' ? 'junction' : undefined
    symlinkSync(elsewhere, join(dir, 'shortcut'), link)
    symlinkSync(join(dir, 'gone'), join(dir, 'broken'), link)
    await page.getByTestId('files-path').press('Enter') // read the folder again

    await expect(entry(page, 'readme.txt')).toContainText('10 bytes')
    // a link is followed like any folder, and says it is one; a link that leads nowhere says so and does not break the list
    await expect(entry(page, 'shortcut')).toContainText('Link')
    await expect(entry(page, 'broken')).toContainText('not there')
    await entry(page, 'shortcut').click()
    await expect(entry(page, 'far.txt')).toBeVisible()
    await page.getByTestId('files-nav-back').click()

    await entry(page, 'photos').click()
    await expect(entry(page, 'cat.jpg')).toBeVisible()
    await expect(page.getByTestId('files-path')).toHaveValue(shown(join(dir, 'photos')))
    // Back goes up the disk, a folder at a time
    await page.getByTestId('files-nav-back').click()
    await expect(entry(page, 'readme.txt')).toBeVisible()
    await page.getByTestId('files-nav-back').click()
    await expect(page.getByTestId('files-path')).toHaveValue(shown(FILES_DIR))
    // a path that is not one is said, and nothing moves
    await page.getByTestId('files-path').fill('not/a/path')
    await page.getByTestId('files-path').press('Enter')
    await expect(page.getByTestId('files-path-problem')).toContainText('starts with “/”')
    rmSync(dir, { recursive: true })
    rmSync(elsewhere, { recursive: true })
  })

  test('F9: upload a file, see it in the list, download it back with the same bytes, and remove it', async ({ page }) => {
    const dir = await openOwnDirectory(page, { 'already-here.txt': 'x' })
    const bytes = Buffer.from(Array.from({ length: 70_000 }, (_, i) => i % 256)) // not text: every byte value

    await page.getByTestId('files-upload-input').setInputFiles([
      { name: 'photo 1.bin', mimeType: 'application/octet-stream', buffer: bytes },
      { name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('second file') }
    ])
    await expect(page.getByTestId('files-upload-photo 1.bin')).toContainText('Uploaded')
    await expect(page.getByTestId('files-upload-notes.txt')).toContainText('Uploaded')
    // the new files are in the list, and on the disk whole, with nothing left beside them
    await expect(entry(page, 'photo 1.bin')).toContainText('70 KB')
    expect(readFileSync(join(dir, 'photo 1.bin')).equals(bytes)).toBe(true)
    expect(readdirSync(dir).sort()).toEqual(['already-here.txt', 'notes.txt', 'photo 1.bin'])
    await page.getByTestId('files-uploads-dismiss').click()
    await expect(page.getByTestId('files-uploads')).toHaveCount(0)

    const [download] = await Promise.all([page.waitForEvent('download'), entry(page, 'photo 1.bin').click()])
    expect(download.suggestedFilename()).toBe('photo 1.bin')
    expect(readFileSync(await download.path()).equals(bytes), 'the same bytes come back').toBe(true)

    await page.getByTestId('files-entry-photo 1.bin-remove').click()
    await expect(page.getByTestId('files-remove-alert')).toContainText('Remove “photo 1.bin”?')
    await page.getByTestId('files-remove-alert-action-remove').click()
    await expect(entry(page, 'photo 1.bin')).toHaveCount(0)
    expect(readdirSync(dir).sort()).toEqual(['already-here.txt', 'notes.txt'])
    rmSync(dir, { recursive: true })
  })

  test('F10: a file whose name is taken is not replaced until the person says: keep both, or replace', async ({ page }) => {
    const dir = await openOwnDirectory(page, { 'report.csv': 'the old one' })
    const pick = (text: string) => page.getByTestId('files-upload-input').setInputFiles([{ name: 'report.csv', mimeType: 'text/csv', buffer: Buffer.from(text) }])
    const alert = page.getByTestId('files-conflict-alert')

    // the folder has been read: the page asks about a name it sees (before that it sends the file, and the host's refusal,
    // which the browser logs as an error, is what makes it ask)
    await expect(entry(page, 'report.csv')).toBeVisible()
    await pick('a new one')
    await expect(alert).toContainText('“report.csv” is already here')
    expect(readFileSync(join(dir, 'report.csv'), 'utf8'), 'nothing is replaced while the question is open').toBe('the old one')
    await page.getByTestId('files-conflict-alert-action-keep-both').click()
    await expect(entry(page, 'report (2).csv')).toBeVisible()
    expect(readFileSync(join(dir, 'report.csv'), 'utf8')).toBe('the old one')
    expect(readFileSync(join(dir, 'report (2).csv'), 'utf8')).toBe('a new one')

    await pick('the newest')
    await page.getByTestId('files-conflict-alert-action-cancel').click()
    await expect(alert).toHaveCount(0)
    expect(readFileSync(join(dir, 'report.csv'), 'utf8')).toBe('the old one')

    await pick('the newest')
    await page.getByTestId('files-conflict-alert-action-replace').click()
    await expect(alert).toHaveCount(0)
    await expect.poll(() => readFileSync(join(dir, 'report.csv'), 'utf8')).toBe('the newest')
    expect(readdirSync(dir).sort()).toEqual(['report (2).csv', 'report.csv'])
    rmSync(dir, { recursive: true })
  })

  test('F11: make a folder, upload into it by dropping a file on the list, and remove the folder with what is in it', async ({ page }) => {
    const dir = await openOwnDirectory(page, { 'Scans/': '' })

    await page.getByTestId('files-new-folder').click()
    await expect(page.getByTestId('files-folder-create')).toBeDisabled()
    await page.getByTestId('files-folder-name').fill('scans')
    await expect(page.getByTestId('files-folder-problem')).toContainText('already here')
    await expect(page.getByTestId('files-folder-create')).toBeDisabled()
    await page.getByTestId('files-folder-name').fill('Letters 2026')
    await expect(page.getByTestId('files-folder-problem')).toHaveCount(0)
    await page.getByTestId('files-folder-create').click()
    await expect(entry(page, 'Letters 2026')).toBeVisible()
    expect(existsSync(join(dir, 'Letters 2026'))).toBe(true)

    await entry(page, 'Letters 2026').click()
    await expect(page.getByTestId('files-empty')).toBeVisible()
    // a file dragged from the desktop of the device and dropped on the page
    const data = await page.evaluateHandle(() => {
      const transfer = new DataTransfer()
      transfer.items.add(new File(['Dear host'], 'letter.txt', { type: 'text/plain' }))
      return transfer
    })
    await page.getByTestId('files-empty').dispatchEvent('dragover', { dataTransfer: data })
    await page.getByTestId('files-empty').dispatchEvent('drop', { dataTransfer: data })
    await expect(entry(page, 'letter.txt')).toBeVisible()
    expect(readFileSync(join(dir, 'Letters 2026/letter.txt'), 'utf8')).toBe('Dear host')

    await page.getByTestId('files-nav-back').click()
    await page.getByTestId('files-entry-Letters 2026-remove').click()
    await expect(page.getByTestId('files-remove-alert')).toContainText('the 1 item in it')
    await page.getByTestId('files-remove-alert-action-remove').click()
    await expect(entry(page, 'Letters 2026')).toHaveCount(0)
    expect(readdirSync(dir)).toEqual(['Scans'])
    rmSync(dir, { recursive: true })
  })

  test('F12: a folder this user may not open says so, and the rest of the page goes on working', async ({ page }, testInfo) => {
    const dir = await openOwnDirectory(page, { 'locked/': '', 'open.txt': 'x' })
    lock(join(dir, 'locked'), true)
    try {
      test.skip(process.getuid?.() === 0, 'root may open anything')
      await entry(page, 'locked').click()
      await expect(page.getByTestId('files-error')).toContainText('You do not have permission to open this folder.')
      await expectAccessible(page, testInfo, { include: '[data-testid="window-files"]' })
      await page.getByTestId('files-nav-back').click()
      await expect(entry(page, 'open.txt')).toBeVisible()
      await expect(page.getByTestId('files-error')).toHaveCount(0)
    } finally {
      lock(join(dir, 'locked'), false)
      rmSync(dir, { recursive: true })
    }
  })

  test('F13: this computer in the smallest window', async ({ page }) => {
    const long = 'a-very-long-file-name-that-somebody-chose-without-thinking-about-narrow-windows.txt'
    const dir = await openOwnDirectory(page, { [long]: 'x', 'short.txt': 'y' })
    await resizeWindow(page, 'files', 100, 100)
    await page.getByTestId('files-new-folder').click()
    await page.getByTestId('files-upload-input').setInputFiles([{ name: 'short.txt', mimeType: 'text/plain', buffer: Buffer.from('z') }])
    await expect(page.getByTestId('files-conflict-alert')).toBeVisible()
    await page.getByTestId('files-conflict-alert-action-cancel').click()
    await settle(page)
    await expectNoOverflow(page.getByTestId('window-files-content'))
    for (const id of ['files-path', 'files-folder-name', 'files-folder-create', `files-entry-${long}`, `files-entry-${long}-remove`]) {
      await page.getByTestId(id).scrollIntoViewIfNeeded()
      await expect(page.getByTestId(id)).toBeVisible()
    }
    rmSync(dir, { recursive: true })
  })
})
