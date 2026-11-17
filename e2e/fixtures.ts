/**
 * Shared Playwright fixtures and checks for AgentOS UI tests.
 *
 *   import { test, expect, openDesktop, expectAccessible, expectNoOverflow } from '../../../e2e/fixtures'
 *
 * Every test gets a frozen clock (9:41) and fails on console errors or
 * uncaught exceptions. Locate things with `getByTestId` only.
 */
import AxeBuilder from '@axe-core/playwright'
import { test as base, expect, type Locator, type Page, type TestInfo } from '@playwright/test'

export { expect }

/** 09:41 on a Tuesday, the way Apple likes it. */
export const FROZEN_TIME = new Date('2026-06-09T09:41:00+01:00')

/** Console noise that isn't a defect in the app under test. */
const IGNORED_CONSOLE = [/Download the Svelte DevTools/, /\[vite\]/, /storybook/i]

export const test = base.extend<{ errors: string[] }>({
  errors: [
    async ({ page }, use) => {
      const errors: string[] = []
      page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`))
      page.on('console', (m) => {
        if (m.type() === 'error' && !IGNORED_CONSOLE.some((re) => re.test(m.text()))) {
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

// ── Desktop (web mode) ───────────────────────────────────────────────────

export interface DesktopOptions {
  /** App ids to open, in order (default: the desktop's own landing window). */
  open?: string[]
  theme?: 'light' | 'dark'
  scenario?: string
  /** Delay between streamed agent chunks, ms. */
  delay?: number
}

/** Load the real desktop renderer on the in-memory host and wait until it settles. */
export async function openDesktop(page: Page, options: DesktopOptions = {}): Promise<void> {
  const params = new URLSearchParams()
  if (options.open) params.set('open', options.open.join(','))
  if (options.theme) params.set('theme', options.theme)
  if (options.scenario) params.set('scenario', options.scenario)
  params.set('delay', String(options.delay ?? 0))
  await page.goto(`/web.html?${params}`)
  await page.locator('html[data-ready="true"]').waitFor()
  await settle(page)
}

/** Wait for fonts, lazy app bundles and two animation frames. */
export async function settle(page: Page): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.evaluate(() => document.fonts.ready)
  await page.evaluate(
    () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null))))
  )
}

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
      if (['auto', 'scroll'].includes(style.overflowX) && el.scrollWidth > el.clientWidth + 1) {
        out.push(`${label(el)} scrolls horizontally (${el.scrollWidth} > ${el.clientWidth})`)
      }
    }
    for (const el of root.querySelectorAll('[data-testid]')) {
      const r = el.getBoundingClientRect()
      if (!r.width || !r.height || !shown(el)) continue
      // Inside a vertical scroller, being below the fold is fine.
      let scroller: Element | null = el.parentElement
      while (scroller && scroller !== root && !['auto', 'scroll'].includes(getComputedStyle(scroller).overflowY))
        scroller = scroller.parentElement
      const clip = scroller && scroller !== root ? scroller.getBoundingClientRect() : frame
      const horizontally = r.left < clip.left - 1 || r.right > clip.right + 1
      const vertically = scroller && scroller !== root ? false : r.top < frame.top - 1 || r.bottom > frame.bottom + 1
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
