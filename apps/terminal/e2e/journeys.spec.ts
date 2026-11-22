/**
 * Terminal key user journeys, end to end: the real shell page, the real host, the app's own client (xterm.js) in a frame of
 * its window, and its Luau backend, which runs a real shell on a pseudo-terminal (`vnd:portable_pty`).
 *
 * The shell is the host's `SHELL`: the end-to-end host (e2e/host/start.ts) makes that a plain `sh` whose prompt is `$ ` and
 * which reads no start-up file, so what is on the screen is the same on every machine. The specs run on the machine the
 * host runs on, so a test can ask the operating system whether a shell's process is still there.
 */
import type { FrameLocator, Locator, Page } from '@playwright/test'
import { chooseLayout, expect, frameOf, openDesktop, openFromHome, resizeWindow, test, windowOf } from '../../../e2e/fixtures'

const ID = 'terminal'
const frame = (page: Page): FrameLocator => frameOf(page, ID)
const part = (page: Page, name: string): Locator => frame(page).getByTestId(`${ID}-${name}`)
const screen = (page: Page): Locator => part(page, 'screen')

/** The session is running and the shell's prompt is on the screen. */
async function prompted(page: Page): Promise<void> {
  await expect(part(page, 'app')).toHaveAttribute('data-phase', 'running')
  await expect(screen(page)).toContainText('$')
}

async function openTerminal(page: Page): Promise<void> {
  await openDesktop(page, { open: [ID] })
  await prompted(page)
  // the terminal has the keyboard once its window does
  await screen(page).click()
}

/** Type a line into the terminal and press Return. */
async function run(page: Page, line: string): Promise<void> {
  await page.keyboard.type(line)
  await page.keyboard.press('Enter')
}

/** What the shell printed between `label:` and `:` (the typed line itself has a `$` there, so only the output matches). */
async function printed(page: Page, label: string): Promise<string> {
  const pattern = new RegExp(`${label}:([^:$]*):`)
  await expect(screen(page)).toContainText(pattern)
  return pattern.exec((await screen(page).textContent()) ?? '')![1]!
}

const alive = (pid: number): boolean => {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

test.describe('Terminal journeys', () => {
  test('TM1: open Terminal from the Home Screen, type a command and see its output', async ({ page }) => {
    await openDesktop(page)
    await openFromHome(page, ID)
    await expect(windowOf(page, ID)).toBeVisible()
    await prompted(page)
    await screen(page).click()
    await run(page, 'echo sum:$((40 + 2)):')
    expect(await printed(page, 'sum')).toBe('42')
    // dark whatever the appearance
    await expect(part(page, 'app')).toHaveAttribute('data-theme', 'dark')
  })

  test('TM2: the shell is the person\'s own, in their home folder, on a terminal that draws like xterm', async ({ page }) => {
    await openTerminal(page)
    await run(page, 'echo term:$TERM:; cd; echo home:$([ "$PWD" = "$HOME" ] && echo yes):')
    expect(await printed(page, 'term')).toBe('xterm-256color')
    expect(await printed(page, 'home')).toBe('yes')
  })

  test('TM3: the terminal has the size of its window, and follows it', async ({ page }) => {
    await openTerminal(page)
    await run(page, 'echo a:$(stty size):')
    const [rows, cols] = (await printed(page, 'a')).split(' ').map(Number)
    expect(rows).toBeGreaterThan(5)
    expect(cols).toBeGreaterThan(20)

    await resizeWindow(page, ID, 900, 600)
    // the shell hears of the new size a moment after the window has it
    await expect(async () => {
      await run(page, 'echo b:$(stty size):')
      const [newRows, newCols] = (await printed(page, 'b')).split(' ').map(Number)
      expect(newRows).toBeGreaterThan(rows!)
      expect(newCols).toBeGreaterThan(cols!)
      await run(page, 'clear')
    }).toPass()
  })

  test('TM4: exit shows that the shell ended, and New Session starts another', async ({ page }) => {
    await openTerminal(page)
    await run(page, 'echo first:$$:')
    const first = await printed(page, 'first')
    await run(page, 'exit 3')
    await expect(part(page, 'app')).toHaveAttribute('data-phase', 'exited')
    await expect(part(page, 'bar-message')).toHaveText('The shell exited with code 3.')
    // what it printed stays until the new session
    await expect(screen(page)).toContainText(`first:${first}:`)

    await part(page, 'bar-action').click()
    await prompted(page)
    await expect(part(page, 'bar')).toHaveCount(0)
    await expect(screen(page)).not.toContainText('first:')
    // it has the keyboard, and it is another shell
    await run(page, 'echo second:$$:')
    expect(await printed(page, 'second')).not.toBe(first)
    expect(alive(Number(first)), 'the first shell is gone').toBe(false)
  })

  test('TM5: closing the window ends the shell', async ({ page }) => {
    await openTerminal(page)
    await run(page, 'echo pid:$$:')
    const pid = Number(await printed(page, 'pid'))
    expect(alive(pid)).toBe(true)
    await chooseLayout(page, ID, 'close')
    await expect(windowOf(page, ID)).toHaveCount(0)
    await expect.poll(() => alive(pid), { message: 'the shell ends with its window' }).toBe(false)
  })

  test('TM6: a reload starts a new session, on an empty screen', async ({ page }) => {
    await openTerminal(page)
    await run(page, 'MARK=kept; echo mark:$MARK:; echo pid:$$:')
    expect(await printed(page, 'mark')).toBe('kept')
    const pid = await printed(page, 'pid')

    await page.reload()
    await openTerminal(page)
    await expect(screen(page)).not.toContainText('mark:kept:')
    await run(page, 'echo mark:$MARK:; echo pid:$$:')
    expect(await printed(page, 'mark'), 'another shell: the variable is not set').toBe('')
    expect(await printed(page, 'pid')).not.toBe(pid)
  })
})
