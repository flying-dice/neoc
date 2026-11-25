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
import { expect, expectNoOverflow, frameOf, openDesktop, openFromHome, resizeWindow, test } from '../../../e2e/fixtures'

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
    await row(page, pid).click()
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
    await row(page, pid).click()
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
    await row(page, pid).click()
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
})
