/**
 * Processes key user journeys, end to end: the real shell, the real host, the app's own client in a frame of its
 * window, and its Luau backend, which reads the machine and ends programs through `vnd:sysinfo`.
 *
 * The machine is the one that runs the specs, so no journey assumes what else runs on it. Each starts the programs it
 * needs (a Node process that does nothing, one that ignores the request to quit) as the person running the specs,
 * finds them by their PID, and ends only those: never a process it did not start. The journeys stop what they started.
 */
import { spawn, type ChildProcess } from 'node:child_process'
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'

const ID = 'processes'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
// the kit's text field holds the real input in its shadow tree
const field = (page: Page, name: string): Locator => part(page, name).locator('input')
const row = (page: Page, pid: number): Locator => part(page, `row-${pid}`)
const WINDOWS = process.platform === 'win32'

test.describe.configure({ mode: 'serial' })

const started: ChildProcess[] = []

/** Start a program that does nothing until it is ended; `stubborn` ignores the request to quit. Resolves with its PID once it runs. */
async function launch(stubborn = false): Promise<number> {
  const script = `${stubborn ? "process.on('SIGTERM', () => {});" : ''} setInterval(() => {}, 1000); process.stdout.write('ready')`
  const child = spawn(process.execPath, ['-e', script], { stdio: ['ignore', 'pipe', 'ignore'] })
  started.push(child)
  await new Promise<void>((resolve, reject) => {
    child.stdout?.once('data', () => resolve())
    child.once('error', reject)
    child.once('exit', () => reject(new Error('the program exited before it was ready')))
  })
  if (child.pid === undefined) throw new Error('the program has no PID')
  return child.pid
}

/**
 * Choose a program and wait until it can be ended. A program only just started is shown but not held until a later
 * reading has told it apart from another that is given its PID, and that reading is another observation of it: the
 * choice is made again, as a person would.
 */
async function choose(page: Page, pid: number): Promise<void> {
  await expect(async () => {
    await row(page, pid).click()
    await expect(part(page, 'force')).toHaveJSProperty('disabled', false, { timeout: 1500 })
  }).toPass({ timeout: 25_000 })
}

/** Whether a process is still there (a signal of 0 only asks). */
function running(pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

test.afterAll(() => {
  for (const child of started) child.kill('SIGKILL')
})

/** Open the window and look for one program by its PID. */
async function find(page: Page, pid: number): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await expect(part(page, 'tiles')).toBeVisible()
  await field(page, 'search').fill(String(pid))
  await expect(row(page, pid)).toBeVisible()
}

test.describe('Processes journeys', () => {
  test('PR1: open Processes from the home screen and see how busy the machine is and what runs on it', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(part(page, 'app')).toBeVisible()
    await expect(part(page, 'tiles')).toBeVisible()
    // memory is measured at once; the machine has some, and some of it is in use
    await expect(part(page, 'tile-memory-value')).toHaveText(/\d+(\.\d)? [KMGT]?B/)
    await expect(part(page, 'tile-memory-note')).toContainText('of ')
    // the processes: this one is running them
    await expect(frame(page).locator('[data-testid^="processes-row-"]').first()).toBeVisible()
    await expect(part(page, 'none')).toBeVisible()
  })

  test('PR2: a program is found by its PID, and chosen: its details are shown', async ({ page }) => {
    const pid = await launch()
    await find(page, pid)
    // nothing else matches a whole PID that is not the start of another
    await row(page, pid).click()
    await expect(part(page, 'detail-name')).toHaveText(/node|bun/i)
    await expect(part(page, 'detail-state')).toContainText(`PID ${pid}`)
    await expect(part(page, 'detail-status')).toBeVisible()
    // it was started by this program's own test: its parent is another process, shown to go to
    await expect(part(page, 'parent')).toBeVisible()
    await field(page, 'search').fill('no-such-process-name-xyz')
    await expect(part(page, 'empty')).toBeVisible()
    await expect(row(page, pid)).toHaveCount(0)
    expect(running(pid)).toBe(true)
  })

  test('PR3: Terminate asks first, and asks the program to quit; Cancel sends nothing', async ({ page }) => {
    test.skip(WINDOWS, 'Windows has no request to quit: only Force Terminate is offered there')
    const pid = await launch()
    await find(page, pid)
    await choose(page, pid)
    await expect(part(page, 'terminate')).toHaveJSProperty('disabled', false)

    await part(page, 'terminate').click()
    // the kit's alert holds its words in its shadow tree, and has no box of its own
    await expect(part(page, 'confirm')).toContainText('Terminate “')
    await part(page, 'confirm-cancel').click()
    await expect(part(page, 'confirm')).toHaveCount(0)
    expect(running(pid)).toBe(true)

    await part(page, 'terminate').click()
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'notice')).toContainText('to quit')
    // it quits: the window says so, and nothing more can be done to it
    await expect(part(page, 'quit')).toBeVisible()
    await expect(part(page, 'detail-state')).toContainText('Quit')
    await expect(part(page, 'terminate')).toHaveJSProperty('disabled', true)
    await expect(part(page, 'force')).toHaveJSProperty('disabled', true)
    await expect.poll(() => running(pid)).toBe(false)
  })

  test('PR4: a program that ignores the request to quit stays, and Force Terminate stops it at once', async ({ page }) => {
    test.skip(WINDOWS, 'Windows has no request to quit to ignore')
    const pid = await launch(true)
    await find(page, pid)
    await choose(page, pid)
    await part(page, 'terminate').click()
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'notice')).toContainText('to quit')
    // the request was not turned into a force: it is still there, after the window has read the machine again
    await page.waitForTimeout(2500)
    expect(running(pid)).toBe(true)
    await expect(part(page, 'detail-state')).not.toContainText('Quit')
    await expect(part(page, 'terminate')).toHaveJSProperty('disabled', false)

    await part(page, 'force').click()
    await expect(part(page, 'confirm')).toContainText('cannot save its work')
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'quit')).toBeVisible()
    await expect.poll(() => running(pid)).toBe(false)
  })

  test('PR5: Force Terminate stops a program on every platform, after asking', async ({ page }) => {
    const pid = await launch()
    await find(page, pid)
    await choose(page, pid)
    if (WINDOWS) await expect(part(page, 'terminate')).toHaveCount(0)
    await part(page, 'force').click()
    await part(page, 'confirm-cancel').click()
    expect(running(pid)).toBe(true)
    await part(page, 'force').click()
    await part(page, 'confirm-confirm').click()
    await expect(part(page, 'quit')).toBeVisible()
    await expect.poll(() => running(pid)).toBe(false)
  })

  test('PR6: a program that quits on its own is said to have quit, and the next one is chosen without a trace of it', async ({ page }) => {
    const first = await launch()
    const second = await launch()
    await find(page, first)
    await row(page, first).click()
    await expect(part(page, 'detail-state')).toContainText(`PID ${first}`)
    process.kill(first, 'SIGKILL')
    await expect(part(page, 'quit')).toBeVisible({ timeout: 15_000 })
    await expect(part(page, 'force')).toHaveJSProperty('disabled', true)
    await field(page, 'search').fill(String(second))
    await row(page, second).click()
    await expect(part(page, 'detail-state')).toContainText(`PID ${second}`)
    await expect(part(page, 'quit')).toHaveCount(0)
    await expect(part(page, 'force')).toHaveJSProperty('disabled', false)
  })

  test('PR7: the machine as a whole', async ({ page }) => {
    await openDesktop(page, { open: [ID] })
    await part(page, 'view-overview').click()
    await expect(part(page, 'overview')).toBeVisible()
    await expect(part(page, 'overview-memory')).toBeVisible()
    await expect(part(page, 'uptime')).toBeVisible()
    await part(page, 'view-processes').click()
    await expect(part(page, 'tiles')).toBeVisible()
  })

  test('PR8: in a narrow window one column shows at a time, with the way back', async ({ page }) => {
    const pid = await launch()
    await openDesktop(page, { open: [ID] })
    await resizeWindow(page, ID, 420, 640)
    await expectNoOverflow(page.getByTestId(`window-${ID}-content`))
    await expect(part(page, 'tiles')).toBeVisible()
    await field(page, 'search').fill(String(pid))
    await row(page, pid).click()
    await expect(part(page, 'detail-name')).toBeVisible()
    await expect(part(page, 'tiles')).toBeHidden()
    await part(page, 'detail-back').click()
    await expect(part(page, 'tiles')).toBeVisible()
    await part(page, 'back').click()
    await expect(part(page, 'view-overview')).toBeVisible()
  })

  test('PR9: a system process can be looked at without the right to end it, and nothing is signalled', async ({ page }) => {
    // the first process of the machine: it belongs to the system, and this journey never asks to end it
    const pid = WINDOWS ? 4 : 1
    await openDesktop(page, { open: [ID] })
    await expect(part(page, 'tiles')).toBeVisible()
    await field(page, 'search').fill(String(pid))
    await expect(row(page, pid)).toBeVisible()
    await row(page, pid).click()
    await expect(part(page, 'detail-state')).toContainText(`PID ${pid}`)
    await expect(part(page, 'detail-memory')).toBeVisible()
    // whether it may be ended is the backend's to say: when it may not, the window says why instead of failing at the click
    if ((await part(page, 'force').evaluate((button) => (button as HTMLButtonElement & { disabled: boolean }).disabled)) === true) {
      await expect(part(page, 'unavailable')).toBeVisible()
    }
    await expect(part(page, 'failure')).toHaveCount(0)
  })

  test('PR10: a window the shell has put away is not read, and is read again when it is back', async ({ page }) => {
    await openDesktop(page, { open: [ID] })
    await expect(part(page, 'tiles')).toBeVisible()
    // count the readings the window asks the backend for
    const body = frame(page).locator('body')
    await body.evaluate(() => {
      const bridge = (globalThis as unknown as { vos: { call: (method: string, params?: unknown) => Promise<unknown> } }).vos
      const call = bridge.call.bind(bridge)
      const counted = globalThis as unknown as { __readings: number }
      counted.__readings = 0
      bridge.call = (method, params) => {
        if (method === 'snapshot') counted.__readings += 1
        return call(method, params)
      }
    })
    const readings = (): Promise<number> => body.evaluate(() => (globalThis as unknown as { __readings: number }).__readings)
    await expect.poll(readings, { timeout: 15_000 }).toBeGreaterThanOrEqual(2)
    await expect(part(page, 'app')).toHaveAttribute('data-showing', 'true')

    // put away: the shell keeps the frame (so it is not `document.hidden`), and the window stops reading
    await chooseLayout(page, ID, 'minimize')
    await expect(part(page, 'app')).toHaveAttribute('data-showing', 'false')
    const away = await readings()
    await page.waitForTimeout(5000)
    // a reading that was already on its way may land: no more than one
    expect(await readings()).toBeLessThanOrEqual(away + 1)

    // back on the stage: it is read at once, and again on its timer
    await page.getByTestId(`dock-${ID}`).click()
    await expect(windowOf(page, ID)).toBeVisible()
    await expect(part(page, 'app')).toHaveAttribute('data-showing', 'true')
    await expect.poll(readings, { timeout: 15_000 }).toBeGreaterThan(away + 1)
  })

  test('PR11: only a choice the backend still holds can end a program: a made-up id, and one that was let go, end nothing', async ({ page }) => {
    const first = await launch()
    const second = await launch()
    await openDesktop(page, { open: [ID] })
    await expect(part(page, 'tiles')).toBeVisible()
    const body = frame(page).locator('body')
    // the ids the backend issues, as the window receives them
    await body.evaluate(() => {
      const bridge = (globalThis as unknown as { vos: { call: (method: string, params?: unknown) => Promise<unknown> } }).vos
      const call = bridge.call.bind(bridge)
      const seen = globalThis as unknown as { __ids: string[]; __chosen: unknown[] }
      seen.__ids = []
      seen.__chosen = []
      bridge.call = async (method, params) => {
        const answer = await call(method, params)
        if (method === 'select') {
          seen.__ids.push((answer as { selection: string }).selection)
          seen.__chosen.push((params as { process: unknown }).process)
        }
        return answer
      }
    })
    const ids = (): Promise<string[]> => body.evaluate(() => (globalThis as unknown as { __ids: string[] }).__ids)
    const terminate = (selection: string): Promise<string> =>
      body.evaluate(async (_body, id) => {
        const bridge = (globalThis as unknown as { vos: { call: (method: string, params?: unknown) => Promise<unknown> } }).vos
        try {
          await bridge.call('terminate', { selection: id, force: true })
          return 'sent'
        } catch (e) {
          return (e as Error).message
        }
      }, selection)

    await field(page, 'search').fill(String(first))
    await row(page, first).click()
    await expect(part(page, 'detail-state')).toContainText(`PID ${first}`)
    await field(page, 'search').fill(String(second))
    await row(page, second).click()
    await expect(part(page, 'detail-state')).toContainText(`PID ${second}`)
    // only just started: a choice made before the next reading has confirmed it cannot end it, so it is chosen again
    await choose(page, second)
    await expect.poll(async () => (await ids()).length).toBeGreaterThanOrEqual(2)
    const all = await ids()
    const letGo = all[0]
    const held = all[all.length - 1]

    // the list still shows a process that is not the one with its PID now (a PID given to another process, even in the
    // same second): the same PID and start time with another sighting is refused, and nothing is held for it
    const refused = await body.evaluate(async () => {
      const bridge = (globalThis as unknown as { vos: { call: (method: string, params?: unknown) => Promise<unknown> } }).vos
      const chosen = (globalThis as unknown as { __chosen: { observation: number }[] }).__chosen.at(-1)!
      try {
        await bridge.call('select', { process: { ...chosen, observation: chosen.observation + 1000 } })
        return 'chosen'
      } catch (e) {
        return (e as Error).message
      }
    })
    expect(refused).toBe('That program has quit.')
    expect(await ids()).toHaveLength(all.length)

    // an id the backend never issued
    expect(await terminate('made-up')).toContain('no longer holds it')
    // the choice of the first program was let go when the second was chosen: it ends nothing, and is not made again
    expect(await terminate(letGo as string)).toContain('no longer holds it')
    expect(running(first)).toBe(true)
    expect(running(second)).toBe(true)
    // the choice that is held ends its own program, and only that
    expect(await terminate(held as string)).toBe('sent')
    await expect.poll(() => running(second)).toBe(false)
    expect(running(first)).toBe(true)
  })
})
