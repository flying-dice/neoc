/**
 * Notification journeys (client/journeys/notifications.md): an app's backend raises a notification and the shell
 * draws it as a banner, on the real host. The sender is the Notifier (crates/vos-host/tests/fixtures/notifier): each
 * button starts background work that notifies two seconds later, so the window can be closed, or another app brought
 * forward, by the time it arrives. It is built outside the registry (e2e/host/start.ts); these specs install it and
 * remove it again, and run one after another, last, because a notification reaches every open page.
 *
 * Timing: nothing here sleeps to wait for something. Every wait is a poll on what the page shows. The one pause, in
 * NO6, is to show that a page did not draw a banner, which no poll can.
 */
import { type Browser, type BrowserContext, type Page } from '@playwright/test'
import { NOTIFIER_APP_REF } from './host/config'
import { chooseLayout, expect, expectAccessible, openDesktop, test, windowOf } from './fixtures'

test.describe.configure({ mode: 'serial' })

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

async function installNotifier(page: Page): Promise<void> {
  if (await page.getByTestId('dock-notifier').count()) return
  const preview = await hostCall<{ sha256: string }>(page, 'app.preview', { url: NOTIFIER_APP_REF })
  await hostCall(page, 'app.install', { url: NOTIFIER_APP_REF, sha256: preview.sha256 })
  await expect(page.getByTestId('dock-notifier')).toBeVisible()
}

/** The shell with the Notifier installed, and its window open and in front. */
async function openNotifier(page: Page, theme?: 'light' | 'dark'): Promise<void> {
  await openDesktop(page, { theme })
  await installNotifier(page)
  await page.getByTestId('dock-notifier').click()
  await expect(page.getByTestId('notifier-sent')).toBeVisible()
}

/** Every banner on the page (the stack's own children, by test id). */
const banners = (page: Page) => page.getByTestId(/^notification-\d+$/)
const bannerParts = (page: Page, part: 'open' | 'dismiss') => page.getByTestId(new RegExp(`^notification-\\d+-${part}$`))

/** Bring another app forward, so the Notifier's window is no longer the one in front. */
async function moveAway(page: Page): Promise<void> {
  await page.getByTestId('dock-files').click()
  await expect(windowOf(page, 'files')).toBeVisible()
}

const frontApp = (page: Page): Promise<string | undefined> =>
  page.evaluate(() => window.__vos.wm.focused?.appId)

/** Start the Notifier's delayed notification, and take the window out of the way as a person would. */
async function notifyWhileAway(page: Page, button: 'later' | 'burst' | 'long' | 'odd' = 'later'): Promise<void> {
  await page.getByTestId(`notifier-${button}`).click()
  await moveAway(page)
}

test.afterAll(async ({ browser }) => {
  const context = await browser.newContext({
    baseURL: test.info().project.use.baseURL,
    storageState: test.info().project.use.storageState as string
  })
  const page = await context.newPage()
  await openDesktop(page)
  if (await page.getByTestId('dock-notifier').count()) {
    await hostCall(page, 'app.uninstall', { app: 'notifier', purge: true })
    await expect(page.getByTestId('dock-notifier')).toHaveCount(0)
  }
  await context.close()
})

test.describe('Notification journeys', () => {
  test('NO1: an app notifies from its background work after its window is closed, and the person dismisses it', async ({ page }) => {
    await openNotifier(page)
    await page.getByTestId('notifier-later').click()
    await chooseLayout(page, 'notifier', 'close')
    await expect(windowOf(page, 'notifier')).toHaveCount(0)

    // the app has no window, and a banner appears all the same, naming the app the shell knows it by
    await expect(banners(page)).toHaveCount(1)
    await expect(banners(page).first()).toContainText('Notifier')
    await expect(banners(page).first()).toContainText('Check finished')
    await expect(banners(page).first()).toContainText('Nothing needed your attention.')
    // and it is announced, in the region that is always there
    await expect(page.getByTestId('notifications-live')).toContainText('Notifier: Check finished. Nothing needed your attention.')

    await bannerParts(page, 'dismiss').first().click()
    await expect(banners(page)).toHaveCount(0)
    await expect(windowOf(page, 'notifier')).toHaveCount(0) // dismissing opens nothing
  })

  test('NO2: choosing the banner opens the app that sent it', async ({ page }) => {
    await openNotifier(page)
    await notifyWhileAway(page)
    await expect(banners(page)).toHaveCount(1)
    expect(await frontApp(page)).toBe('files')

    await bannerParts(page, 'open').first().click()
    await expect(banners(page)).toHaveCount(0)
    await expect(windowOf(page, 'notifier')).toBeVisible()
    expect(await frontApp(page)).toBe('notifier')
    // the keys went with it: the window, not the page, has focus
    await expect(page.getByTestId('notifier-later')).toBeVisible()
    expect(await page.evaluate(() => document.activeElement?.closest('[role="dialog"]')?.getAttribute('data-testid'))).toBe(
      'window-notifier'
    )
  })

  test('NO3: by keyboard alone: go to the banner, dismiss it with Escape, and open the app with Enter', async ({ page }) => {
    await openNotifier(page)
    await notifyWhileAway(page)
    await expect(banners(page)).toHaveCount(1)

    await page.keyboard.press('Alt+Shift+N')
    await expect(bannerParts(page, 'open').first()).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(banners(page)).toHaveCount(0)
    await expect(bannerParts(page, 'open')).toHaveCount(0)

    // the second one: the same way in, and Enter opens the app
    await page.getByTestId('dock-notifier').click()
    await page.getByTestId('notifier-later').click()
    await moveAway(page)
    await expect(banners(page)).toHaveCount(1)
    await page.keyboard.press('Alt+Shift+N')
    await expect(bannerParts(page, 'open').first()).toBeFocused()
    await page.keyboard.press('Enter')
    await expect(banners(page)).toHaveCount(0)
    expect(await frontApp(page)).toBe('notifier')
  })

  test('NO4: three at most, newest first; long text and odd text stay on their lines and are text', async ({ page }) => {
    await openNotifier(page)
    await notifyWhileAway(page, 'burst')
    // four were raised: the oldest was pushed out, the other three are stacked newest first
    await expect(banners(page)).toHaveCount(3)
    await expect(banners(page).nth(0)).toContainText('Message 4')
    await expect(banners(page).nth(1)).toContainText('Message 3')
    await expect(banners(page).nth(2)).toContainText('Message 2')
    for (let i = 0; i < 3; i++) await bannerParts(page, 'dismiss').first().click()
    await expect(banners(page)).toHaveCount(0)

    // long text: the card stays small
    await page.getByTestId('dock-notifier').click()
    await notifyWhileAway(page, 'long')
    await expect(banners(page)).toHaveCount(1)
    const long = (await banners(page).first().boundingBox())!
    expect(long.height, 'a long body is cut, not grown').toBeLessThan(200)
    await bannerParts(page, 'dismiss').first().click()

    // odd text: one line for the title (the break, the override and the zero-width space are gone); the body keeps its
    // line break and shows markup as the characters it is
    await page.getByTestId('dock-notifier').click()
    await notifyWhileAway(page, 'odd')
    await expect(banners(page)).toHaveCount(1)
    const odd = banners(page).first()
    await expect(odd).toContainText('Odd title gnp.exe')
    await expect(odd).toContainText('second line <b>not bold</b>')
    expect(await odd.locator('b').count(), 'the body is not markup').toBe(0)
    expect(await odd.evaluate((el) => /[‮​\n]/.test(el.querySelector('.title')?.textContent ?? ''))).toBe(false)
  })

  test('NO5: a banner reads well in light and dark at a small window, and has no accessibility violations', async ({ page }, testInfo) => {
    for (const theme of ['light', 'dark'] as const) {
      await page.setViewportSize({ width: 360, height: 640 })
      await openNotifier(page, theme)
      await notifyWhileAway(page, 'long')
      await expect(banners(page)).toHaveCount(1)
      await expectAccessible(page, testInfo, { include: '[data-testid="notifications"]' })
      const box = (await banners(page).first().boundingBox())!
      expect(box.x).toBeGreaterThanOrEqual(0)
      expect(box.x + box.width).toBeLessThanOrEqual(360)
      await bannerParts(page, 'dismiss').first().click()
    }
  })

  test('NO6: two clients at once: only the one where the app is not in front draws the banner', async ({ page, browser }) => {
    const other = await secondClient(browser, test.info().project.use)
    try {
      const second = await other.newPage()
      await openDesktop(second)
      await openNotifier(page)

      // the first client has the Notifier in front: the second, with no window of it, shows the banner
      await page.getByTestId('notifier-later').click()
      await expect(banners(second)).toHaveCount(1)
      await expect(banners(second).first()).toContainText('Notifier')
      await page.waitForTimeout(500) // the same message has reached the first client by now; there is nothing to poll for
      await expect(banners(page)).toHaveCount(0)
      await bannerParts(second, 'dismiss').first().click()

      // the first client moves on: now both show it
      await page.getByTestId('notifier-later').click()
      await moveAway(page)
      await expect(banners(page)).toHaveCount(1)
      await expect(banners(second)).toHaveCount(1)
    } finally {
      await other.close()
    }
  })
})

/** A second browser (a second paired client of the same host), as a person's second device would be. */
async function secondClient(browser: Browser, use: { baseURL?: string; storageState?: unknown }): Promise<BrowserContext> {
  return browser.newContext({
    baseURL: use.baseURL,
    storageState: use.storageState as string,
    reducedMotion: 'reduce',
    viewport: { width: 1280, height: 800 }
  })
}
