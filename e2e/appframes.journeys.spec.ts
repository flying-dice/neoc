/**
 * App frame journeys (client/journeys/app-frames.md): an app whose screens are a web client of its own, in a frame of
 * its window, on a Luau backend, on the real host. The two framework fixtures (`e2e/apps/tasks-svelte` and
 * `tasks-react`: the same task list, written once in each framework) are installed at start from a folder no registry
 * lists (`e2e/host/start.ts`); every journey runs against both.
 *
 * They share the fixtures' data, so they run one after another, and each journey adds tasks with its own titles.
 */
import { type Page } from '@playwright/test'
import { FIXTURE_APPS, fixtureAppRef, type FixtureApp } from './host/config'
import { callTool } from './host/pair'
import { expect, frameOf, openDesktop, settle, test, windowOf } from './fixtures'

test.describe.configure({ mode: 'serial' })

interface HostApi {
  call(method: string, params?: Record<string, unknown>): Promise<unknown>
}
/** The host-management call the shell makes (`window.__vos.host.api`), as Discover's Get button does. */
const hostCall = <T>(page: Page, method: string, params: Record<string, unknown>): Promise<T> =>
  page.evaluate(
    ([m, p]) =>
      (window as unknown as { __vos: { host: { api: HostApi } } }).__vos.host.api.call(
        m as string,
        p as Record<string, unknown>
      ),
    [method, params] as const
  ) as Promise<T>

const FRAMEWORK: Record<FixtureApp, string> = { 'tasks-svelte': 'Svelte', 'tasks-react': 'React' }

/** Titles are unique per run of a journey, so the list the fixtures keep never makes one journey depend on another. */
let serial = 0
const unique = (what: string): string => `${what} ${Date.now().toString(36)}-${++serial}`

for (const id of FIXTURE_APPS) {
  test.describe(`App frame journeys: ${id}`, () => {
    const frame = (page: Page) => frameOf(page, id)

    test(`AF1 (${id}): install an app with a client and open it`, async ({ page }) => {
      await openDesktop(page)
      // start from a host that does not have it (it is installed at start): the same two calls Discover's Get makes
      await hostCall(page, 'app.uninstall', { app: id, purge: true })
      await expect(page.getByTestId(`dock-${id}`)).toHaveCount(0)
      const url = fixtureAppRef(id)
      const preview = await hostCall<{ sha256: string; id: string }>(page, 'app.preview', { url })
      expect(preview.id).toBe(id)
      await hostCall(page, 'app.install', { url, sha256: preview.sha256 })
      await expect(page.getByTestId(`dock-${id}`)).toBeVisible()

      await page.getByTestId(`dock-${id}`).click()
      await expect(windowOf(page, id)).toBeVisible()
      await expect(frame(page).getByTestId(`${id}-framework`)).toHaveText(FRAMEWORK[id])
      await expect(page.getByTestId(`${id}-opening`)).toHaveCount(0)
    })

    test(`AF2 (${id}): add a task and see it in the list`, async ({ page }) => {
      await openDesktop(page, { open: [id] })
      const title = unique('Buy milk')
      await frame(page).getByTestId(`${id}-title`).fill(title)
      await frame(page).getByTestId(`${id}-add`).click()
      const item = frame(page).locator('li', { hasText: title })
      await expect(item).toBeVisible()
      await expect(frame(page).getByTestId(`${id}-title`), 'the field is empty for the next one').toHaveValue('')

      // it is the backend's data: a second window of the page, reopened, shows it again
      await page.reload()
      await openDesktop(page, { open: [id] })
      await expect(frame(page).locator('li', { hasText: title })).toBeVisible()

      // and the person ticks it off
      const toggle = frame(page).locator('li', { hasText: title }).getByRole('checkbox')
      await toggle.click() // (not .check(): a controlled checkbox follows the backend's answer, a moment later)
      await expect(toggle).toBeChecked()
    })

    test(`AF3 (${id}): an error from the backend shows as a message`, async ({ page }) => {
      await openDesktop(page, { open: [id] })
      await frame(page).getByTestId(`${id}-title`).fill('   ')
      await frame(page).getByTestId(`${id}-add`).click()
      await expect(frame(page).getByTestId(`${id}-error`)).toHaveText('A task needs a title.')
      // the window is still live: the next add works and the message goes
      const title = unique('After the error')
      await frame(page).getByTestId(`${id}-title`).fill(title)
      await frame(page).getByTestId(`${id}-title`).press('Enter') // the keyboard adds too
      await expect(frame(page).getByTestId(`${id}-error`)).toHaveCount(0)
      await expect(frame(page).locator('li', { hasText: title })).toBeVisible()
    })

    test(`AF4 (${id}): an agent changes the data and the open window follows`, async ({ page }) => {
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      const title = unique('From the agent')
      await expect(frame(page).locator('li', { hasText: title })).toHaveCount(0)
      // the agent calls the app's tool over MCP; the tool tells the open window with app.emit: no reload, no click
      const answer = await callTool(`${id}__add_task`, { title })
      expect(answer).toContain('tasks')
      await expect(frame(page).locator('li', { hasText: title })).toBeVisible()
    })

    test(`AF5 (${id}): the client reaches nothing but its own backend`, async ({ page, errors }) => {
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      const probes = await frame(page)
        .locator('body')
        .evaluate(async () => {
          const attempt = async (what: () => unknown): Promise<string> => {
            try {
              await what()
              return 'allowed'
            } catch (e) {
              return e instanceof Error ? e.name : String(e)
            }
          }
          return {
            // no network: not the shell's own host, not its /rpc, nothing else
            fetchSession: await attempt(() => fetch('/session')),
            fetchElsewhere: await attempt(() => fetch('http://127.0.0.1:9/')),
            socket: await attempt(
              () =>
                new Promise((resolve, reject) => {
                  const socket = new WebSocket(`ws://${location.host || '127.0.0.1'}/rpc`)
                  socket.onopen = () => resolve(null)
                  socket.onerror = () => reject(new Error('refused'))
                })
            ),
            // no way into the shell's page, its cookie or its storage
            parentDocument: await attempt(() => window.parent.document.title),
            cookie: await attempt(() => document.cookie),
            storage: await attempt(() => localStorage.length)
          }
        })
      for (const [probe, outcome] of Object.entries(probes)) expect(outcome, probe).not.toBe('allowed')
      // the browser reports each refused request as a console error: those are the expected ones (the policy did it),
      // and anything else the page logged still fails the test
      const refused = errors.filter((e) => /Content Security Policy/.test(e))
      expect(refused.length, 'the policy refused the fetches and the socket').toBeGreaterThanOrEqual(3)
      errors.splice(0, errors.length, ...errors.filter((e) => !refused.includes(e)))
    })

    test(`AF8 (${id}): the router refuses a call the definition does not allow`, async ({ page }) => {
      await openDesktop(page)
      // what a client could send if it ignored its generated types: the shell forwards it, the router answers
      const answer = (method: string, args: unknown): Promise<{ code?: number; message: string }> =>
        page.evaluate(
          async ([app, m, a]) => {
            try {
              await (
                window as unknown as { __vos: { host: { api: HostApi } } }
              ).__vos.host.api.call('app.call', { app, method: m, args: a })
              return { message: 'allowed' }
            } catch (e) {
              const { code, message } = e as { code?: number; message: string }
              return { code, message }
            }
          },
          [id, method, args] as const
        )
      expect(await answer('add', { title: 5 })).toEqual({
        code: -32602,
        message: 'Invalid params: params.title must be a string, not a number'
      })
      expect(await answer('add', {})).toEqual({ code: -32602, message: 'Invalid params: params.title is required' })
      expect(await answer('toggle', { id: 0 })).toEqual({
        code: -32602,
        message: 'Invalid params: params.id must be at least 1'
      })
      expect(await answer('delete', {})).toEqual({ code: -32601, message: 'Method not found: delete' })
      // and the window is unharmed: it still lists what is there
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
    })

    test(`AF6 (${id}): the frame follows light and dark live, and its styles stay inside`, async ({ page }) => {
      await openDesktop(page, { open: [id], theme: 'light' })
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('light')
      const shellBackground = await page.evaluate(() => getComputedStyle(document.body).backgroundColor)
      // a mark in the frame's own window: a reload of the frame would lose it
      await frame(page).locator('body').evaluate(() => ((window as unknown as { marked: boolean }).marked = true))

      await page.evaluate(() => (window.__vos.settings.theme = 'dark'))
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('dark')
      await expect(frame(page).locator('html')).toHaveAttribute('data-theme', 'dark')
      expect(
        await frame(page).locator('body').evaluate(() => (window as unknown as { marked?: boolean }).marked),
        'the frame was not reloaded'
      ).toBe(true)
      // the client's styles (`body { background }`, `h1`) did not reach the shell
      expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).toBe(shellBackground)

      await page.evaluate(() => (window.__vos.settings.theme = 'light'))
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('light')
      await settle(page)
    })
  })
}

test('AF7: two apps with clients each reach only their own backend', async ({ page }) => {
  const [a, b] = ['tasks-svelte', 'tasks-react'] as const
  await openDesktop(page, { open: [a] })
  // the second beside the first, on the same stage: both are on screen
  await page.evaluate(
    (id) =>
      (window as unknown as { __vos: { wm: { open(app: string, options: { alongside: boolean }): void } } }).__vos.wm.open(
        id,
        { alongside: true }
      ),
    b
  )
  await expect(windowOf(page, b)).toBeVisible()
  const title = unique('Only in one')
  await frameOf(page, a).getByTestId(`${a}-title`).fill(title)
  await frameOf(page, a).getByTestId(`${a}-add`).click()
  await expect(frameOf(page, a).locator('li', { hasText: title })).toBeVisible()
  // the other app's backend never saw it (and its window was not told: events go to their own app's frames)
  await settle(page)
  await expect(frameOf(page, b).locator('li', { hasText: title })).toHaveCount(0)
  await expect(frameOf(page, b).getByTestId(`${b}-app`)).toBeVisible()
})
