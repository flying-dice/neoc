/**
 * Background work journeys (client/journeys/background.md): an app that works on its own, on the real host. The
 * Ticker (e2e/apps/ticker) counts up from a task its backend starts at load; the window follows
 * it with no input, the count goes on while the window is closed, and a dropped connection comes back to a
 * current window. The Ticker is built outside the registry (e2e/host/start.ts), so Discover never
 * offers it; these specs install it and remove it again. They share one Ticker, so they run one after another.
 *
 * Timing: nothing here sleeps to wait for something. Every wait is a poll on what the page shows. The one pause,
 * in BG2, is to show that nothing changes, which no poll can.
 */
import { type Page, type WebSocketRoute } from '@playwright/test'
import { TICKER_APP_REF } from './host/config'
import { chooseLayout, expect, expectRunning, frameOf, isRunning, openDesktop, openFromHome, test, windowOf } from './fixtures'

test.describe.configure({ mode: 'serial' })

/** What the Ticker's own page draws, in the frame of its window. */
const ticker = (page: Page) => frameOf(page, 'ticker')
const ticks = (page: Page) => ticker(page).getByTestId('ticker-ticks')
const status = (page: Page) => ticker(page).getByTestId('ticker-status')
const toggle = (page: Page) => ticker(page).getByTestId('ticker-toggle')

/** The host-management call the shell makes (`window.__vos.host.api`), as the Get button of Discover does. */
interface HostApi {
  call(method: string, params?: Record<string, unknown>): Promise<unknown>
}
const hostCall = <T>(page: Page, method: string, params: Record<string, unknown>): Promise<T> =>
  page.evaluate(
    ([m, p]) =>
      (window as unknown as { __vos: { host: { api: HostApi } } }).__vos.host.api.call(
        m as string,
        p as Record<string, unknown>
      ),
    [method, params] as const
  ) as Promise<T>

async function installTicker(page: Page): Promise<void> {
  if (await isRunning(page, 'ticker')) return
  const preview = await hostCall<{ sha256: string }>(page, 'app.preview', { url: TICKER_APP_REF })
  await hostCall(page, 'app.install', { url: TICKER_APP_REF, sha256: preview.sha256 })
  await expectRunning(page, 'ticker')
}

/** The Ticker's window from the Home Screen, counting (an earlier journey may have paused it). */
async function openTicker(page: Page): Promise<void> {
  await openDesktop(page)
  await installTicker(page)
  await openFromHome(page, 'ticker')
  await expect(ticker(page).getByTestId('ticker-app')).toBeVisible()
  await expect(ticks(page)).toHaveText(/^\d+$/)
  if ((await status(page).textContent()) === 'Paused') {
    await toggle(page).click()
  }
  await expect(status(page)).toHaveText('Counting')
}

const count = async (page: Page): Promise<number> => Number(await ticks(page).textContent())

/** The shell's connection to the host (`window.__vos.host.connection`). */
const connection = (page: Page): Promise<string> =>
  page.evaluate(() => (window as unknown as { __vos: { host: { connection: string } } }).__vos.host.connection)

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDesktop(page)
  if (await isRunning(page, 'ticker')) {
    await hostCall(page, 'app.uninstall', { app: 'ticker', purge: true })
    await expectRunning(page, 'ticker', false)
  }
  await context.close()
})

test.describe('Background work journeys', () => {
  test('BG1: open an app that works on its own and watch it count with no input', async ({ page }) => {
    await openTicker(page)
    const first = await count(page)
    // nobody touches anything: the host pushes each change to the open window
    await expect.poll(() => count(page)).toBeGreaterThan(first)
    const second = await count(page)
    await expect.poll(() => count(page)).toBeGreaterThan(second)
  })

  test('BG2: pause it, the count holds; resume it, the count goes on', async ({ page }) => {
    await openTicker(page)
    await toggle(page).click()
    await expect(status(page)).toHaveText('Paused')
    await expect(toggle(page)).toHaveText('Resume')
    const held = await count(page)
    // five ticks' worth of time: only absence needs a pause, there is nothing to poll for
    await page.waitForTimeout(1000)
    expect(await count(page), 'a paused counter does not move').toBe(held)

    await toggle(page).click()
    await expect(status(page)).toHaveText('Counting')
    await expect.poll(() => count(page)).toBeGreaterThan(held)
  })

  test('BG3: close the window and open it again: the count went on without it', async ({ page }) => {
    await openTicker(page)
    await expect.poll(() => count(page)).toBeGreaterThan(0)
    const atClose = await count(page)
    await chooseLayout(page, 'ticker', 'close')
    await expect(windowOf(page, 'ticker')).toHaveCount(0)

    await openFromHome(page, 'ticker')
    await expect(ticker(page).getByTestId('ticker-app')).toBeVisible()
    // the new window is current at once and the service has been counting all along
    await expect.poll(() => count(page)).toBeGreaterThan(atClose)
  })

  test('BG4: the connection drops and comes back to a current window', async ({ page }) => {
    const sockets: WebSocketRoute[] = []
    let refuse = false
    await page.routeWebSocket(/\/rpc$/, (ws) => {
      if (refuse) {
        ws.close().catch(() => {}) // the host is unreachable: the client keeps trying
        return
      }
      ws.connectToServer()
      sockets.push(ws)
    })
    await openTicker(page)
    await expect.poll(() => count(page)).toBeGreaterThan(0)
    const before = await count(page)
    expect(sockets.length, 'the shell talks to the host over one socket').toBe(1)

    refuse = true
    await sockets[0].close()
    await expect.poll(() => connection(page)).toBe('offline')
    // the app's page stays where it was: nothing reaches it, and it shows the last count it heard
    await expect(ticker(page).getByTestId('ticker-app')).toBeVisible()
    await expect(page.getByTestId('ticker-failed')).toHaveCount(0)

    refuse = false
    await expect.poll(() => connection(page)).toBe('online')
    // the window was resynced: current (beyond what it last had) and still moving with no input
    await expect.poll(() => count(page)).toBeGreaterThan(before)
    const resumed = await count(page)
    await expect.poll(() => count(page)).toBeGreaterThan(resumed)
  })

  test('BG5: a control that fails says so, and the counting goes on', async ({ page }) => {
    await openTicker(page)
    await ticker(page).getByTestId('ticker-fail').click()
    await expect(ticker(page).getByTestId('ticker-notice')).toContainText('always fails')
    await expect(page.getByTestId('ticker-failed')).toHaveCount(0)
    const first = await count(page)
    await expect.poll(() => count(page)).toBeGreaterThan(first)
    // and the window still answers
    await toggle(page).click()
    await expect(status(page)).toHaveText('Paused')
  })
})
