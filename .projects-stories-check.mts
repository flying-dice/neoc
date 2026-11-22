// A one-off check of the Projects stories alone: every story, light and dark, play function, console, axe, overflow.
import AxeBuilder from '@axe-core/playwright'
import { chromium } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const dir = process.argv[2]!
const shots = process.argv[3]!
const server = Bun.serve({
  port: 0,
  async fetch(req) {
    const path = new URL(req.url).pathname
    const file = Bun.file(join(dir, path === '/' ? 'index.html' : path))
    return (await file.exists()) ? new Response(file) : new Response('not found', { status: 404 })
  }
})
const entries = Object.values(JSON.parse(readFileSync(join(dir, 'index.json'), 'utf8')).entries as Record<string, { type: string; id: string; title: string; name: string; tags: string[] }>).filter((e) => e.type === 'story')
const browser = await chromium.launch()
let failed = 0
for (const story of entries) {
  for (const theme of ['Light', 'Dark']) {
    const sizes = story.tags.includes('app') ? [undefined, { width: 320, height: 240 }] : [undefined]
    for (const size of sizes) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: theme === 'Dark' ? 'dark' : 'light', reducedMotion: 'reduce', locale: 'en-GB', timezoneId: 'Europe/London' })
      const page = await context.newPage()
      const problems: string[] = []
      page.on('console', (m) => { if (m.type() === 'error') problems.push(`console: ${m.text().slice(0, 400)}`) })
      page.on('response', (r) => { if (r.status() >= 400) problems.push(`http ${r.status()} ${r.url()}`) })
      page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`))
      const args = size ? `&args=width:${size.width};height:${size.height}` : ''
      await page.goto(`http://localhost:${server.port}/iframe.html?id=${story.id}&viewMode=story&globals=theme:${theme}${args}`)
      const phase = await page.waitForFunction(() => {
        const p = (window as any).__STORYBOOK_PREVIEW__?.currentRender?.phase
        return p === 'finished' || p === 'completed' || p === 'errored' || p === 'aborted' ? p : false
      }, undefined, { timeout: 20_000 }).then((h) => h.jsonValue()).catch(() => 'timeout')
      if (phase !== 'finished' && phase !== 'completed') problems.push(`phase ${phase}: ${(await page.locator('#error-message, #error-stack').allInnerTexts()).join(' ').slice(0, 600)}`)
      await page.evaluate(() => document.fonts.ready)
      const axe = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).include('#storybook-root').analyze().catch((e) => ({ violations: [{ id: 'axe-failed', help: String(e), nodes: [] as { target: string[] }[] }] }))
      for (const v of axe.violations) problems.push(`axe ${v.id}: ${v.help} @ ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`)
      const frame = page.getByTestId('app-window')
      if (await frame.count()) {
        const over = await frame.evaluate((el) => {
          const box = el.getBoundingClientRect()
          const out: string[] = []
          for (const child of el.querySelectorAll('*')) {
            const r = child.getBoundingClientRect()
            if (r.width === 0 || r.height === 0) continue
            const style = getComputedStyle(child)
            if (style.visibility === 'hidden' || style.display === 'none') continue
            if (r.right > box.right + 1 || r.left < box.left - 1) out.push(`${child.tagName.toLowerCase()}.${child.className} ${Math.round(r.left - box.left)}..${Math.round(r.right - box.left)} of ${Math.round(box.width)}`)
          }
          return out.slice(0, 4)
        })
        // a clipped scroller's content is not overflow: only report when the window itself scrolls sideways
        const scrolls = await frame.evaluate((el) => el.scrollWidth > el.clientWidth + 1 || [...el.querySelectorAll('main')].some((m) => m.scrollWidth > m.clientWidth + 1))
        if (scrolls) problems.push(`overflow: ${over.join('; ')}`)
      }
      const name = `${story.title.replace(/\W+/g, '-')}--${story.name.replace(/\W+/g, '-')}-${theme}${size ? '-min' : ''}`
      await page.screenshot({ path: join(shots, `${name}.png`) })
      if (problems.length) { failed++; console.log(`✘ ${story.title} › ${story.name} [${theme}${size ? ' min' : ''}]\n   ${problems.join('\n   ')}`) }
      await context.close()
    }
  }
}
console.log(`${entries.length} stories checked, ${failed} runs with problems`)
await browser.close()
server.stop(true)
process.exit(failed ? 1 : 0)
