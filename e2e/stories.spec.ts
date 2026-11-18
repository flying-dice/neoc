/**
 * Every story in every AgentOS Storybook, in light and dark:
 *  • renders, and its play function (if any) passes,
 *  • logs no errors,
 *  • passes axe (WCAG 2.1 AA),
 *  • nothing scrolls sideways or is cut off,
 *  • matches its visual snapshot.
 * Stories tagged `app` (an app screen in an AppWindow frame with width/height
 * args) are also checked at the smallest window (320x240, clamped to the
 * app's manifest minimum, as a real window is) and a large one.
 *
 * Stories are discovered from each package's storybook-static/index.json,
 * so a new story or a new app's Storybook is covered with no extra code.
 */
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { execFileSync } from 'node:child_process'
import type { Page } from '@playwright/test'
import { expect, expectAccessible, expectNoOverflow, test } from './fixtures'

interface IndexEntry {
  type: 'story' | 'docs'
  id: string
  title: string
  name: string
  tags: string[]
}

const ROOT = join(import.meta.dirname, '..')
const books = execFileSync('bun', ['scripts/storybooks.ts', 'list'], { cwd: ROOT, encoding: 'utf8' })
  .split('\n')
  .filter(Boolean)

function storiesOf(book: string): IndexEntry[] {
  const dir = [join(ROOT, 'apps', book), join(ROOT, 'packages', book), join(ROOT, book)].find((d) => existsSync(join(d, '.storybook'))) ?? join(ROOT, book)
  const index = join(dir, 'storybook-static', 'index.json')
  if (!existsSync(index)) throw new Error(`${book}: no Storybook build. Run \`bun scripts/storybooks.ts build\`.`)
  const { entries } = JSON.parse(readFileSync(index, 'utf8')) as { entries: Record<string, IndexEntry> }
  return Object.values(entries).filter((e) => e.type === 'story')
}

const THEMES = ['Light', 'Dark'] as const
const APP_SIZES = [
  { name: 'min', width: 320, height: 240 },
  { name: 'large', width: 1024, height: 720 }
]

async function gotoStory(
  page: Page,
  book: string,
  id: string,
  theme: (typeof THEMES)[number],
  size?: { width: number; height: number }
): Promise<void> {
  const args = size ? `&args=width:${size.width};height:${size.height}` : ''
  await page.goto(`/${book}/iframe.html?id=${id}&viewMode=story&globals=theme:${theme}${args}`)
  const phase = await page
    .waitForFunction(
      () => {
        const p = (window as unknown as { __STORYBOOK_PREVIEW__?: { currentRender?: { phase?: string } } })
          .__STORYBOOK_PREVIEW__?.currentRender?.phase
        return p === 'finished' || p === 'completed' || p === 'errored' || p === 'aborted' ? p : false
      },
      undefined,
      { timeout: 20_000 }
    )
    .then((h) => h.jsonValue())
  if (phase !== 'finished' && phase !== 'completed') {
    const message = await page.locator('#error-message, #error-stack').allInnerTexts()
    throw new Error(`Story ${id} ${phase}: ${message.join('\n') || 'see the trace'}`)
  }
  // Play functions click programmatically, which leaves a focus ring a pointer wouldn't.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  await page.evaluate(() => document.fonts.ready)
}

test.use({ viewport: { width: 1280, height: 900 } })

for (const book of books) {
  for (const story of storiesOf(book)) {
    test.describe(`${book} › ${story.title}`, () => {
      test(story.name, async ({ page }, testInfo) => {
        const root = page.locator('#storybook-root')
        for (const theme of THEMES) {
          await test.step(theme, async () => {
            await gotoStory(page, book, story.id, theme)
            // a story that shows a deliberately clipped state (a swiped-open row) tags itself `overflow-ok`
            if (!story.tags.includes('overflow-ok')) await expectNoOverflow(root)
            await expectAccessible(page, testInfo, { include: '#storybook-root' })
            await expect(page).toHaveScreenshot([book, `${story.id}-${theme.toLowerCase()}.png`], { fullPage: true })
          })
        }

        if (story.tags.includes('app')) {
          for (const size of APP_SIZES) {
            await test.step(`window ${size.width}x${size.height}`, async () => {
              await gotoStory(page, book, story.id, 'Light', size)
              const frame = page.getByTestId('app-window')
              if (!story.tags.includes('overflow-ok')) await expectNoOverflow(frame)
              await expect(frame).toHaveScreenshot([book, `${story.id}-${size.name}.png`])
            })
          }
        }
      })
    })
  }
}
