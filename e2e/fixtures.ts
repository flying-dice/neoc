/**
 * Shared Playwright fixtures and checks for vos UI tests.
 *
 *   import { test, expect, openDesktop, expectAccessible, expectNoOverflow } from '../../e2e/fixtures'
 *
 * Every test gets a frozen clock (9:41) and fails on console errors or
 * uncaught exceptions. Locate things with `getByTestId` only.
 */
import AxeBuilder from '@axe-core/playwright'
import { test as base, expect, type FrameLocator, type Locator, type Page, type TestInfo } from '@playwright/test'

export { expect }

/** 09:41 on a Tuesday, the way Apple likes it. */
export const FROZEN_TIME = new Date('2026-06-09T09:41:00+01:00')

/**
 * Console noise that isn't a defect in the app under test. A pattern is looked for in the message's first line only: a
 * failed play function logs the page's markup after its first line, and a story's markup always says `storybook-root`,
 * so a pattern read against the whole message would hide every "Unable to find an element" a play function throws.
 */
const IGNORED_CONSOLE = [
  /Download the Svelte DevTools/,
  /\[vite\]/,
  /storybook/i,
  // the pairing journeys: a wrong code is a 401, a revoked device's socket is refused
  /status of 401/,
  /WebSocket connection to .* failed/
]

export const test = base.extend<{ errors: string[] }>({
  errors: [
    async ({ page }, use) => {
      const errors: string[] = []
      page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`))
      page.on('console', (m) => {
        const [summary = ''] = m.text().split('\n')
        if (m.type() === 'error' && !IGNORED_CONSOLE.some((re) => re.test(summary))) {
          errors.push(`console.error: ${m.text()}`)
        }
      })
      await page.clock.setFixedTime(FROZEN_TIME)
      await use(errors)
      expect(errors, 'the page logged errors').toEqual([])
    },
    { auto: true }
  ]
})

// ── Desktop (the real shell on the real host) ─────────────────────────────

export interface DesktopOptions {
  /** App ids to open, in order (default: the desktop's own landing window, Discover). */
  open?: string[]
  theme?: 'light' | 'dark'
}

/** The shell's debug handle (`window.__vos`, see client boot.ts). */
interface ShellHandle {
  host: { phase: string; apps: unknown[] }
  wm: {
    windows: { id: string }[]
    focused?: { appId: string }
    close(id: string): void
    open(appId: string): void
    showHome(): void
  }
  apps: Record<string, { minWidth: number; minHeight: number }>
}
declare global {
  interface Window {
    __vos: ShellHandle
  }
}

/**
 * Load the shell, paired (the project's storage state), and wait until the desktop has settled. With `open`,
 * the landing window is replaced by exactly these apps' windows, opened as an icon would (none, `open: []`, leaves the
 * Home Screen on show).
 */
export async function openDesktop(page: Page, options: DesktopOptions = {}): Promise<void> {
  const params = new URLSearchParams()
  if (options.theme) params.set('theme', options.theme)
  await page.goto(`/?${params}`)
  await page.locator('html[data-ready="true"]').waitFor()
  await page.waitForFunction(
    () => window.__vos.host.phase === 'ready' && window.__vos.host.apps.length > 0
  )
  await page.getByTestId('dock').waitFor()
  await settle(page)
  if (options.open) {
    await page.evaluate((ids) => {
      const { wm, apps } = window.__vos
      for (const win of [...wm.windows]) wm.close(win.id)
      for (const id of ids) {
        if (!apps[id]) throw new Error(`Unknown app "${id}". Known: ${Object.keys(apps).join(', ')}`)
        wm.open(id)
      }
    }, options.open)
    await settle(page)
  }
}

/**
 * The Home Screen (the grid of every app on the wallpaper), which is only there while no window is on stage: what is
 * on stage steps aside into the recent-apps strip, closing nothing (the shell's "Show Home Screen").
 */
export async function showHome(page: Page): Promise<void> {
  await page.evaluate(() => window.__vos.wm.showHome())
  await page.getByTestId('home').waitFor()
}

/**
 * Open an app as a person does when it is not in the Dock: from its icon on the Home Screen (going there first if a
 * window covers it). While a Split View waits for its second app the icons are in its empty half, and that is where
 * this clicks. The Dock holds only what is pinned (Discover, Files and Settings unless a test pins more) and the open
 * apps, so `dock-<appId>` is not a way to open any app.
 */
export async function openFromHome(page: Page, appId: string): Promise<void> {
  const icon = page.getByTestId(`home-${appId}`)
  if (!(await page.getByTestId('home').isVisible())) await showHome(page)
  await icon.click()
  await expect(windowOf(page, appId)).toBeVisible()
  await settle(page)
}

/** Whether the shell has this app to open (installed and running): what its icon on the Home Screen follows. */
export const isRunning = (page: Page, appId: string): Promise<boolean> =>
  page.evaluate((id) => Object.hasOwn(window.__vos.apps, id), appId)

/** Wait until the shell has (or no longer has) an app: an install or a removal has reached this page. */
export async function expectRunning(page: Page, appId: string, running = true): Promise<void> {
  await expect.poll(() => isRunning(page, appId), `${appId} is ${running ? '' : 'not '}running`).toBe(running)
}

/** Open an icon's context menu (a secondary click) and choose from it. `icon` is `home-<appId>` or `dock-<appId>`. */
export async function chooseFromAppMenu(page: Page, icon: string, item: 'open' | 'pin' | 'unpin'): Promise<void> {
  await page.getByTestId(icon).click({ button: 'right' })
  await page.getByTestId(`app-menu-${item}`).click()
  await expect(page.getByTestId('app-menu')).toHaveCount(0)
}

/** Wait for fonts, the network and two animation frames. */
export async function settle(page: Page): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.evaluate(() => document.fonts.ready)
  await page.evaluate(
    () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null))))
  )
}

/** The frame of an app that has a client of its own (`<appId>-frame`): locate what the client draws inside it. */
export const frameOf = (page: Page, appId: string): FrameLocator => page.frameLocator(`[data-testid="${appId}-frame"]`)

/** The window for an app (`window-<appId>`). */
export const windowOf = (page: Page, appId: string): Locator => page.getByTestId(`window-${appId}`)

/** Drag a window's resize corner so its frame becomes `width` x `height` (clamped to its minimum). */
export async function resizeWindow(page: Page, appId: string, width: number, height: number): Promise<void> {
  const win = windowOf(page, appId)
  const box = (await win.boundingBox())!
  const handle = (await page.getByTestId(`window-${appId}-resize`).boundingBox())!
  const sx = handle.x + handle.width / 2
  const sy = handle.y + handle.height / 2
  await page.mouse.move(sx, sy)
  await page.mouse.down()
  await page.mouse.move(sx + (width - box.width), sy + (height - box.height), { steps: 6 })
  await page.mouse.up()
  // Park the pointer so nothing (the Dock, a hover state) stays highlighted.
  await page.mouse.move(1, 1)
  // A drag that meets the minimum size or the screen's edge shows a label for about two seconds: wait it out, or a
  // snapshot taken now would catch it half faded.
  const note = page.getByTestId(`window-${appId}-resize-note`)
  await expect(note).toHaveText('', { timeout: 8000 })
  await expect(note).toHaveCSS('opacity', '0')
  await settle(page)
}

/** Choose a layout from a window's multitasking menu. */
export async function chooseLayout(
  page: Page,
  appId: string,
  item: 'fullscreen' | 'split-left' | 'split-right' | 'slideover' | 'floating' | 'minimize' | 'close'
): Promise<void> {
  await page.getByTestId(`window-${appId}-multitasking`).click()
  await page.getByTestId(`window-${appId}-menu-${item}`).click()
  await settle(page)
}

// ── Checks ───────────────────────────────────────────────────────────────

/** WCAG 2.1 A/AA with axe. Scope with `include` (a CSS selector). Attaches the full report. */
export async function expectAccessible(
  page: Page,
  testInfo: TestInfo,
  options: { include?: string; disableRules?: string[] } = {}
): Promise<void> {
  let builder = new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
  if (options.include) builder = builder.include(options.include)
  if (options.disableRules?.length) builder = builder.disableRules(options.disableRules)
  const results = await builder.analyze()
  await testInfo.attach('axe-results.json', { body: JSON.stringify(results.violations, null, 2), contentType: 'application/json' })
  const summary = results.violations.map(
    (v) =>
      `${v.impact?.toUpperCase()} ${v.id}: ${v.help}\n` +
      v.nodes
        .slice(0, 5)
        .map((n) => `    ${n.target.join(' ')}\n      ${n.failureSummary?.split('\n').join('\n      ')}`)
        .join('\n')
  )
  expect(summary, 'accessibility violations (axe, WCAG 2.1 AA)').toEqual([])
}

/**
 * Layout check for a container (a window, or a story's app frame):
 *  • nothing scrolls horizontally,
 *  • every element with a test id that is visible lies fully inside the container.
 */
export async function expectNoOverflow(container: Locator): Promise<void> {
  const problems = await container.evaluate((root) => {
    const out: string[] = []
    const frame = root.getBoundingClientRect()
    const label = (el: Element): string =>
      el.getAttribute('data-testid') ?? `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}`
    // Deliberately hidden UI (an auto-hidden strip or Dock, an inert layer) isn't "cut off".
    const shown = (el: Element): boolean => {
      if (el.closest('[inert], [aria-hidden="true"]')) return false
      for (let n: Element | null = el; n; n = n.parentElement) {
        const cs = getComputedStyle(n)
        if (cs.opacity === '0' || cs.visibility === 'hidden' || cs.display === 'none') return false
      }
      return true
    }
    for (const el of [root, ...root.querySelectorAll('*')]) {
      const style = getComputedStyle(el)
      if (style.display === 'none' || style.visibility === 'hidden') continue
      // A pager (scroll-snap on x: TabView pages, carousels) is meant to scroll sideways, and so is a tray that says it
      // (`data-scrolls-x`: the Dock when more apps than fit are installed).
      const pager = style.scrollSnapType.startsWith('x') || el.hasAttribute('data-scrolls-x')
      if (!pager && ['auto', 'scroll'].includes(style.overflowX) && el.scrollWidth > el.clientWidth + 1) {
        out.push(`${label(el)} scrolls horizontally (${el.scrollWidth} > ${el.clientWidth})`)
      }
    }
    for (const el of root.querySelectorAll('[data-testid]')) {
      const r = el.getBoundingClientRect()
      if (!r.width || !r.height || !shown(el)) continue
      // Visually hidden helpers (a 1x1 live region) are not content.
      if (r.width <= 1 && r.height <= 1) continue
      // Inside a scroller, being past its edge is fine: below the fold (vertical), on another page (horizontal).
      const scrolls = (n: Element, axis: 'overflowX' | 'overflowY'): boolean =>
        ['auto', 'scroll'].includes(getComputedStyle(n)[axis])
      let scroller: Element | null = el.parentElement
      while (scroller && scroller !== root && !scrolls(scroller, 'overflowY') && !scrolls(scroller, 'overflowX'))
        scroller = scroller.parentElement
      const clip = scroller && scroller !== root ? scroller.getBoundingClientRect() : frame
      const inScroller = !!scroller && scroller !== root
      const horizontally =
        !(inScroller && scrolls(scroller!, 'overflowX')) && (r.left < clip.left - 1 || r.right > clip.right + 1)
      const vertically = inScroller ? false : r.top < frame.top - 1 || r.bottom > frame.bottom + 1
      if (horizontally || vertically) {
        out.push(
          `${label(el)} is cut off (${Math.round(r.left)},${Math.round(r.top)} ${Math.round(r.width)}x${Math.round(r.height)} ` +
            `outside ${Math.round(clip.left)},${Math.round(clip.top)} ${Math.round(clip.width)}x${Math.round(clip.height)})`
        )
      }
    }
    return out
  })
  expect(problems, 'layout problems').toEqual([])
}
