/**
 * App frame journeys: an app whose screens are a web client of its own, in a frame of
 * its window (or alone, on its own page at `/apps/<id>`), on a Luau backend, on the real host. The two framework fixtures (`e2e/apps/tasks-svelte` and
 * `tasks-react`: the same task list, written once in each framework) are installed at start from a folder no registry
 * lists (`e2e/host/start.ts`); every journey runs against both.
 *
 * They share the fixtures' data, so they run one after another, and each journey adds tasks with its own titles.
 */
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { type Page } from '@playwright/test'
import { FIXTURE_APPS, fileUrl, fixtureAppRef, type FixtureApp } from './host/config'
import { callTool } from './host/pair'
import { chooseLayout, expect, expectRunning, frameOf, openDesktop, openFromHome, settle, test, windowOf } from './fixtures'

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
      await expectRunning(page, id, false)
      const url = fixtureAppRef(id)
      const preview = await hostCall<{ sha256: string; id: string }>(page, 'app.preview', { url })
      expect(preview.id).toBe(id)
      await hostCall(page, 'app.install', { url, sha256: preview.sha256 })
      await expectRunning(page, id)

      // it is on the Home Screen (not in the Dock: nobody pinned it), and opens from there
      await openFromHome(page, id)
      await expect(page.getByTestId(`dock-${id}`), 'open and not pinned: one of the Dock’s recent apps').toBeVisible()
      await expect(frame(page).getByTestId(`${id}-framework`)).toHaveText(FRAMEWORK[id])
      await expect(page.getByTestId(`${id}-opening`)).toHaveCount(0)
      // the handshake told the app it is in the shell, which draws UI of its own
      await expect(frame(page).getByTestId(`${id}-where`)).toHaveText('in the shell')
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
      // the agent calls the app's tool over MCP; the tool tells the open window with its `tasks_changed` notification: no reload, no click
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
            storage: await attempt(() => localStorage.length),
            // what `sandbox` is there for. Each of these says `blocked` or `allowed` itself, so a sandbox that is missing
            // makes it say `allowed`: no popup, no way to read the shell's location, no form submission
            popup: (() => {
              try {
                return window.open('https://example.com/', '_blank') === null ? 'blocked' : 'allowed'
              } catch {
                return 'blocked'
              }
            })(),
            topLocation: await attempt(() => window.top?.location.href),
            topNavigation: (() => {
              try {
                window.top!.location.href = 'http://127.0.0.1:9/'
              } catch {
                return 'blocked'
              }
              // an assignment that did not throw says nothing: whether the shell moved is checked from outside
              return 'attempted'
            })(),
            // a form works inside the frame (`allow-forms`): the `submit` event fires, and a handler that prevents the default keeps
            // the document (an unprevented submission is AF9)
            formSubmit: await (async () => {
              const form = document.createElement('form')
              form.action = 'http://127.0.0.1:9/'
              form.method = 'post'
              let fired = false
              form.addEventListener('submit', (event) => {
                event.preventDefault()
                fired = true
              })
              document.body.append(form)
              form.requestSubmit()
              await new Promise((r) => setTimeout(r, 300))
              // a submission that went through replaces this document (and this call with it)
              const stayed = document.body.contains(form)
              form.remove()
              return !stayed ? 'allowed' : fired ? 'blocked' : 'no submit event'
            })()
          }
        })
      // every probe reports what happened: `allowed` is a failure (the others throw, or say `blocked`)
      for (const [probe, outcome] of Object.entries(probes)) expect(outcome, probe).not.toBe('allowed')
      expect(probes.popup, 'window.open returns null').toBe('blocked')
      expect(probes.formSubmit, 'the submit event fired and the prevented submission left the document alone').toBe(
        'blocked'
      )
      // a prevented submission is not a page the frame left
      await page.waitForTimeout(500)
      await expect(page.getByTestId(`${id}-failed`)).toHaveCount(0)
      // and the shell did not move, and the frame is still the app's
      expect(new URL(page.url()).pathname).toBe('/')
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      // the browser reports each refused request as a console error: those are the expected ones (the policy did it),
      // and anything else the page logged still fails the test
      const refused = errors.filter((e) => /Content Security Policy|sandbox/i.test(e))
      expect(refused.length, 'the policy refused the fetches and the socket').toBeGreaterThanOrEqual(3)
      errors.splice(0, errors.length, ...errors.filter((e) => !refused.includes(e)))
    })

    test(`AF13 (${id}): the bus refuses a call the contract does not allow`, async ({ page }) => {
      await openDesktop(page)
      // what a client could send if it ignored its generated types: the shell forwards it, the bus holds it to the contract
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
      expect(await answer('_add', { title: 5 })).toEqual({
        code: -32602,
        message: 'Invalid params: /title: 5 is not of type "string"'
      })
      expect(await answer('_add', {})).toEqual({ code: -32602, message: 'Invalid params: "title" is a required property' })
      expect(await answer('_toggle', { id: 0 })).toEqual({
        code: -32602,
        message: 'Invalid params: /id: 0 is less than the minimum of 1'
      })
      expect(await answer('delete', {})).toEqual({ code: -32601, message: 'Method not found: delete' })
      // and the window is unharmed: it still lists what is there
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
    })

    test(`AF14 (${id}): the app declares what its page needs and the host applies it`, async ({ page }) => {
      const declared = id === 'tasks-react'
      // the page's policy is the host's, built from `vos.csp` and `vos.permissions` in its package.json: nothing external unless the app named it, and never scripts
      const response = await page.request.get(`/apps/${id}/static/`)
      const csp = response.headers()['content-security-policy'] ?? ''
      expect(csp).toContain('sandbox allow-scripts')
      expect(csp).not.toContain('connect-src')
      expect(csp.includes('https://cdn.example.com')).toBe(declared)
      expect(csp).toMatch(/script-src 'self';/)
      expect(response.headers()['x-content-type-options']).toBe('nosniff')
      // and the frame's `allow` carries what it asked for, which works under the sandbox
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      await expect(page.getByTestId(`${id}-frame`)).toHaveAttribute('allow', declared ? 'camera' : '')
      const allowed = await frame(page)
        .locator('body')
        .evaluate(() => (document as unknown as { featurePolicy: { allowsFeature(f: string): boolean } }).featurePolicy.allowsFeature('camera'))
      expect(allowed, 'the camera is allowed in the frame only when the app asked').toBe(declared)
    })

    test(`AS1 (${id}): opened on its own page, the app works with no shell`, async ({ page }) => {
      await page.goto(`/apps/${id}`)
      // the page's title is the system's name, then the app's
      await expect(page).toHaveTitle(FRAMEWORK[id] === 'Svelte' ? 'VOS - Tasks (Svelte)' : 'VOS - Tasks (React)')
      // the page is the app and nothing else: one frame, no chrome, no shell
      await expect(page.locator('iframe')).toHaveCount(1)
      await expect(page.getByTestId('dock')).toHaveCount(0)
      await expect(frame(page).getByTestId(`${id}-where`)).toHaveText('on its own')
      const title = unique('On its own')
      await frame(page).getByTestId(`${id}-title`).fill(title)
      await frame(page).getByTestId(`${id}-add`).click()
      await expect(frame(page).locator('li', { hasText: title })).toBeVisible()
      // the same data the shell's window shows
      await openDesktop(page, { open: [id] })
      await expect(frame(page).locator('li', { hasText: title })).toBeVisible()
    })

    test(`AS2 (${id}): on its own page the app hears the backend's events and follows the browser's appearance`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: 'light' })
      await page.goto(`/apps/${id}`)
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('light')
      await frame(page).locator('body').evaluate(() => ((window as unknown as { marked: boolean }).marked = true))
      const title = unique('From the agent, on its own')
      await callTool(`${id}__add_task`, { title })
      await expect(frame(page).locator('li', { hasText: title })).toBeVisible()
      await page.emulateMedia({ colorScheme: 'dark' })
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('dark')
      await expect(frame(page).locator('html')).toHaveAttribute('data-theme', 'dark')
      expect(
        await frame(page).locator('body').evaluate(() => (window as unknown as { marked?: boolean }).marked),
        'the frame was not reloaded'
      ).toBe(true)
    })

    test(`AS3 (${id}): on its own page the app still reaches nothing but its backend`, async ({ page, errors }) => {
      await page.goto(`/apps/${id}`)
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
            fetchSession: await attempt(() => fetch('/session')),
            parentDocument: await attempt(() => window.parent.document.title),
            cookie: await attempt(() => document.cookie)
          }
        })
      for (const [probe, outcome] of Object.entries(probes)) expect(outcome, probe).not.toBe('allowed')
      const refused = errors.filter((e) => /Content Security Policy/.test(e))
      errors.splice(0, errors.length, ...errors.filter((e) => !refused.includes(e)))
    })

    test(`AF8 (${id}): a client that navigates its own frame away is noticed, and the window offers the way back`, async ({
      page
    }) => {
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      // no policy stops a frame navigating itself: the shell sees a document it did not load
      await frame(page)
        .locator('body')
        .evaluate(() => {
          location.href = '/session'
        })
      await expect(page.getByTestId(`${id}-failed`)).toBeVisible({ timeout: 10_000 })
      await expect(page.getByTestId(`${id}-failed`)).toContainText('left its page')
      await page.getByTestId(`${id}-failed-retry`).click()
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      await expect(page.getByTestId(`${id}-failed`)).toHaveCount(0)
    })

    test(`AF9 (${id}): a form nobody prevented stays on the page; one submitted by script, with no event, sends the frame away and the window notices`, async ({
      page,
      errors
    }) => {
      await openDesktop(page, { open: [id] })
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      // the bridge prevents a submission the app's handlers did not: the handlers run and the page stays (a dialog form still closes its dialog)
      const outcome = await frame(page)
        .locator('body')
        .evaluate(async () => {
          const form = document.createElement('form')
          form.action = 'http://127.0.0.1:9/'
          form.method = 'post'
          let fired = false
          form.addEventListener('submit', () => {
            fired = true
          })
          document.body.append(form)
          form.requestSubmit()
          await new Promise((r) => setTimeout(r, 300))
          const stayed = document.body.contains(form)
          form.remove()
          const dialog = document.createElement('dialog')
          const inner = document.createElement('form')
          inner.method = 'dialog'
          dialog.append(inner)
          document.body.append(dialog)
          dialog.showModal()
          inner.requestSubmit()
          const closed = !dialog.open
          dialog.remove()
          return { fired, stayed, closed }
        })
      expect(outcome).toEqual({ fired: true, stayed: true, closed: true })
      await expect(page.getByTestId(`${id}-failed`)).toHaveCount(0)
      // `form.submit()` fires no event, so nothing can prevent it: the browser starts the navigation. The policy refuses to send the
      // data (`form-action 'none'`), but the page is gone all the same: the window says the app left its page
      await frame(page)
        .locator('body')
        .evaluate(() => {
          const form = document.createElement('form')
          form.action = 'http://127.0.0.1:9/'
          form.method = 'post'
          document.body.append(form)
          setTimeout(() => form.submit(), 0)
        })
      await expect(page.getByTestId(`${id}-failed`)).toBeVisible({ timeout: 10_000 })
      await expect(page.getByTestId(`${id}-failed`)).toContainText('left its page')
      await page.getByTestId(`${id}-failed-retry`).click()
      await expect(frame(page).getByTestId(`${id}-app`)).toBeVisible()
      // the browser's refusals are expected here: the policy and the shell's own frame policy did it
      const refused = errors.filter((e) => /Content Security Policy|violates the following|Refused|blocked/i.test(e))
      expect(refused.length).toBeGreaterThanOrEqual(1)
      errors.splice(0, errors.length, ...errors.filter((e) => !refused.includes(e)))
    })

    test(`AF12 (${id}): what the client holds in its page survives opening another app and coming back`, async ({ page }) => {
      const other = FIXTURE_APPS.find((a) => a !== id)!
      await openDesktop(page, { open: [id] })
      const typed = unique('Unsent')
      await frame(page).getByTestId(`${id}-title`).fill(typed)
      // a marker only this document has: a page that was reloaded would not have it
      await frame(page).locator('body').evaluate((_el, m) => ((window as unknown as { __kept: string }).__kept = m), typed)

      // another app takes the stage; this one's window is off it, and is hidden, not gone
      await openFromHome(page, other)
      await expect(windowOf(page, id)).toBeHidden()
      await expect(page.getByTestId(`${id}-frame`)).toBeAttached()
      // a hidden frame takes no focus and is not in the accessibility tree
      expect(await page.getByTestId(`${id}-frame`).evaluate((el) => !!el.closest('[inert]') && !!el.closest('[aria-hidden="true"]'))).toBe(true)

      // back, from the Dock's recent apps (it is open): the same document, with what was typed in it
      await page.getByTestId(`dock-${id}`).click()
      await expect(windowOf(page, id)).toBeVisible()
      await expect(frame(page).getByTestId(`${id}-title`)).toHaveValue(typed)
      expect(await frame(page).locator('body').evaluate(() => (window as unknown as { __kept?: string }).__kept)).toBe(typed)
      // it still works: the call gate and the bridge are unaffected
      await frame(page).getByTestId(`${id}-add`).click()
      await expect(frame(page).locator('li', { hasText: typed })).toBeVisible()
    })

    test(`AF6 (${id}): the frame follows light and dark live, and its styles stay inside`, async ({ page }) => {
      await openDesktop(page, { open: [id], theme: 'light' })
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('light')
      const shellBackground = await page.evaluate(() => getComputedStyle(document.body).backgroundColor)
      // a mark in the frame's own window: a reload of the frame would lose it
      await frame(page)
        .locator('body')
        .evaluate(() => ((window as unknown as { marked: boolean }).marked = true))

      await page.evaluate(() => (window.__vos.settings.theme = 'dark'))
      await expect(frame(page).getByTestId(`${id}-theme`)).toHaveText('dark')
      await expect(frame(page).locator('html')).toHaveAttribute('data-theme', 'dark')
      expect(
        await frame(page)
          .locator('body')
          .evaluate(() => (window as unknown as { marked?: boolean }).marked),
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
      (
        window as unknown as { __vos: { wm: { open(app: string, options: { alongside: boolean }): void } } }
      ).__vos.wm.open(id, { alongside: true }),
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

test('AS4: an unpaired visitor to an app’s own page is told to pair first', async ({ page }) => {
  await page.context().clearCookies()
  await page.goto('/apps/tasks-svelte')
  await expect(page.getByRole('heading', { name: 'Pair this device first' })).toBeVisible()
  await expect(page.locator('iframe')).toHaveCount(0)
  await page.getByRole('link', { name: 'Open vos to pair' }).click()
  await expect(page).toHaveURL(/\/$/)
})

test('AS5: an app’s page opened straight from its static address is contained, and says it needs its host page', async ({ page, errors }) => {
  // the policy has `sandbox`: opened in a tab the page has an opaque origin, so no cookie of the host reaches it
  await page.goto('/apps/tasks-svelte/static/')
  expect(await page.evaluate(() => window.origin)).toBe('null')
  await expect(page.locator('body')).toContainText('This app could not start: This page needs its host page: open the app at /apps/tasks-svelte/')
  errors.splice(0, errors.length, ...errors.filter((e) => !/This page needs its host page/.test(e)))
})

test('AF15: a page whose script throws while it loads is reported with its own message', async ({ page, errors }) => {
  // a hand-built app package (not one of the fixtures, so no other journey meets it): its page throws at load
  const id = 'brokenapp'
  const dir = mkdtempSync(join(tmpdir(), 'vos-broken-'))
  try {
    const files: Record<string, string> = {
      'app.luau': 'return { ping = function() return "pong" end }\n',
      'static/index.html':
        '<!doctype html>\n<html lang="en"><head><title>Broken</title><script src="/apps/bridge.js"></script></head>' +
        '<body><div id="root"></div><script type="module" src="./boom.js"></script></body></html>\n',
      'static/boom.js': "throw new Error('the broken app threw on purpose')\n"
    }
    for (const [path, text] of Object.entries(files)) {
      mkdirSync(dirname(join(dir, path)), { recursive: true })
      writeFileSync(join(dir, path), text)
    }
    const listed = Object.entries(files).map(([path, text]) => ({
      path,
      url: path,
      sha256: createHash('sha256').update(text).digest('hex')
    }))
    writeFileSync(
      join(dir, 'app-ref.json'),
      JSON.stringify({
        manifest: { id, name: 'Broken', version: '1.0.0', main: 'app.luau', ui: { root: 'static' }, window: { w: 420, h: 480, minW: 320, minH: 320 } },
        files: listed
      })
    )
    await openDesktop(page)
    const url = fileUrl(join(dir, 'app-ref.json'))
    const preview = await hostCall<{ sha256: string }>(page, 'app.preview', { url })
    await hostCall(page, 'app.install', { url, sha256: preview.sha256 })
    try {
      await expectRunning(page, id)
      await openFromHome(page, id)
      const failed = page.getByTestId(`${id}-failed`)
      await expect(failed).toBeVisible({ timeout: 15_000 })
      // the script is loaded from the host as a module, which the host answers with the header that lets the browser say what went wrong
      await expect(failed).toContainText('the broken app threw on purpose')
    } finally {
      await hostCall(page, 'app.uninstall', { app: id, purge: true })
    }
    // the frame's own report of the throw is a console error of the page: expected here
    errors.splice(0, errors.length, ...errors.filter((e) => !/the broken app threw on purpose/.test(e)))
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('AF10: an app whose handlers never answer is stuck alone: the shell, another app and the host carry on', async ({
  page
}) => {
  const [hung, other] = ['tasks-svelte', 'tasks-react'] as const
  await openDesktop(page, { open: [hung] })
  await page.evaluate(
    (id) =>
      (window as unknown as { __vos: { wm: { open(app: string, o: { alongside: boolean }): void } } }).__vos.wm.open(
        id,
        { alongside: true }
      ),
    other
  )
  await expect(frameOf(page, other).getByTestId(`${other}-app`)).toBeVisible()
  await expect(frameOf(page, hung).getByTestId(`${hung}-app`)).toBeVisible()

  // ten calls of a handler that waits (the app's eight places are taken, two wait), and then some more, as a page that retries does
  const startHung = (): Promise<void> =>
    frameOf(page, hung)
      .locator('body')
      .evaluate((_el, count) => {
        const w = window as unknown as {
          __rpc: (m: string, a?: unknown) => Promise<unknown>
          __hung: Promise<unknown>[]
        }
        w.__hung = [
          ...(w.__hung ?? []),
          ...Array.from({ length: count }, () => w.__rpc('_hang').catch((e: unknown) => String(e)))
        ]
      }, 10)
  await startHung()
  await startHung()

  // the shell's own calls and the other app go on at once
  await expect
    .poll(() => hostCall<{ apps: unknown[] }>(page, 'app.list', {}).then((r) => Array.isArray(r.apps)))
    .toBe(true)
  const title = unique('While another app hangs')
  await frameOf(page, other).getByTestId(`${other}-title`).fill(title)
  await frameOf(page, other).getByTestId(`${other}-add`).click()
  await expect(frameOf(page, other).locator('li', { hasText: title })).toBeVisible()

  // while the handlers hang, the app's own calls wait behind them: nothing else does. Closing the window tells the host to stop the
  // handlers it was running for it, so opening it again finds the app whole at once
  await chooseLayout(page, hung, 'close')
  await expect(windowOf(page, hung)).toHaveCount(0)
  await openFromHome(page, hung)
  await expect(frameOf(page, hung).getByTestId(`${hung}-app`)).toBeVisible()
  await expect
    .poll(() => hostCall<{ apps: unknown[] }>(page, 'app.list', {}).then((r) => Array.isArray(r.apps)))
    .toBe(true)

  const title2 = unique('After the window was closed and opened again')
  await frameOf(page, hung).getByTestId(`${hung}-title`).fill(title2)
  await frameOf(page, hung).getByTestId(`${hung}-add`).click()
  await expect(frameOf(page, hung).locator('li', { hasText: title2 })).toBeVisible()
})

test('AF11: uninstalling an app answers the calls its handlers still hold, so the host keeps no permit for them', async ({
  page
}) => {
  const id = 'tasks-svelte'
  await openDesktop(page)
  // straight to the host (no window, no gate): five calls of a handler that waits
  await page.evaluate((app) => {
    const w = window as unknown as {
      __vos: { host: { api: { call(m: string, p: object): Promise<unknown> } } }
      __direct: Promise<string>[]
    }
    w.__direct = Array.from({ length: 5 }, () =>
      w.__vos.host.api.call('app.call', { app, method: '_hang', args: null }).then(
        () => 'answered',
        (e: unknown) => `refused: ${e instanceof Error ? e.message : String(e)}`
      )
    )
  }, id)
  await hostCall(page, 'app.uninstall', { app: id, purge: true })
  // answered (with the reason) well inside the shell's 30 s, not left running
  const outcomes = await page.evaluate(() =>
    Promise.race([
      (window as unknown as { __direct: Promise<string>[] }).__direct.length &&
        Promise.all((window as unknown as { __direct: Promise<string>[] }).__direct),
      new Promise<string[]>((r) => setTimeout(() => r(['still waiting']), 8000))
    ])
  )
  expect(outcomes).not.toContain('still waiting')
  // the host carries on, and the app comes back for the journeys after this one
  const url = fixtureAppRef(id)
  const preview = await hostCall<{ sha256: string }>(page, 'app.preview', { url })
  await hostCall(page, 'app.install', { url, sha256: preview.sha256 })
})
