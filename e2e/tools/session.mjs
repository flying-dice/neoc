#!/usr/bin/env node
/**
 * A persistent, headless browser session that an agent drives one command at
 * a time, the way a person uses the app: look at the screen, click what they
 * can see by its label, type, press keys, drag. Used by the UX research panel
 * (.claude/skills/app-design) against web mode (http://localhost:6200/web.html).
 *
 *   node e2e/tools/session.mjs start  <name> [--url URL] [--width 1280] [--height 800] [--theme light|dark]
 *   node e2e/tools/session.mjs look   <name>              screenshot path + accessibility tree
 *   node e2e/tools/session.mjs click  <name> "<label>" [--role button] [--nth 0]
 *   node e2e/tools/session.mjs click  <name> --at 640,400
 *   node e2e/tools/session.mjs dblclick <name> "<label>" | --at x,y
 *   node e2e/tools/session.mjs hover  <name> "<label>" | --at x,y
 *   node e2e/tools/session.mjs type   <name> "text"       (into whatever has focus)
 *   node e2e/tools/session.mjs fill   <name> "<field label>" "text"
 *   node e2e/tools/session.mjs press  <name> Enter|Escape|Tab|Shift+Tab|ArrowDown|…
 *   node e2e/tools/session.mjs drag   <name> 100,200 400,500
 *   node e2e/tools/session.mjs scroll <name> 0,400 [--at x,y]
 *   node e2e/tools/session.mjs resize <name> 1024 768     (the screen, not a window)
 *   node e2e/tools/session.mjs goto   <name> URL
 *   node e2e/tools/session.mjs stop   <name>
 *
 * Labels match accessible names first (buttons, links, tabs, switches, fields…),
 * then visible text. Every command prints what happened; `look` prints the
 * screenshot path (open it with the Read tool) and the accessibility tree.
 * Sessions live in test-results/sessions/<name>/.
 */
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { createServer } from 'node:http'
import { join, resolve } from 'node:path'

const ROOT = resolve(import.meta.dirname, '../..')
const dirOf = (name) => join(ROOT, 'test-results', 'sessions', name)
const DEFAULT_URL = 'http://localhost:6200/web.html'

const [command, name, ...rest] = process.argv.slice(2)
const flag = (key, fallback) => {
  const i = rest.indexOf(`--${key}`)
  if (i === -1) return fallback
  const [, value] = rest.splice(i, 2)
  return value
}
const xy = (s) => s.split(',').map(Number)

if (!command || (!name && command !== 'help')) {
  console.log(readFileSync(new URL(import.meta.url)).toString().split('*/')[0])
  process.exit(command ? 1 : 0)
}

// ── Server (one per session, detached) ──────────────────────────────────

if (command === '__serve') {
  const { chromium } = await import('@playwright/test')
  const dir = dirOf(name)
  const width = Number(flag('width', 1280))
  const height = Number(flag('height', 800))
  const theme = flag('theme', 'light')
  const url = flag('url', DEFAULT_URL)
  const browser = await chromium.launch()
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: 'reduce' })
  const page = await context.newPage()
  const consoleErrors = []
  page.on('console', (m) => m.type() === 'error' && consoleErrors.push(m.text()))
  page.on('pageerror', (e) => consoleErrors.push(e.message))
  await page.goto(url)
  await page.waitForLoadState('networkidle')
  let shot = 0

  const ROLES = ['button', 'link', 'tab', 'menuitem', 'switch', 'checkbox', 'radio', 'option', 'textbox', 'searchbox', 'combobox', 'slider', 'treeitem', 'dialog', 'region']

  async function find(label, role, nth = 0) {
    const candidates = role ? [page.getByRole(role, { name: label })] : [
      ...ROLES.map((r) => page.getByRole(r, { name: label, exact: true })),
      ...ROLES.map((r) => page.getByRole(r, { name: label })),
      page.getByLabel(label),
      page.getByText(label, { exact: true }),
      page.getByText(label)
    ]
    for (const loc of candidates) {
      const visible = []
      for (const el of await loc.all()) if (await el.isVisible().catch(() => false)) visible.push(el)
      if (visible.length) {
        if (nth >= visible.length) throw new Error(`only ${visible.length} visible matches for "${label}"`)
        return { el: visible[nth], count: visible.length }
      }
    }
    throw new Error(`nothing visible matches "${label}"${role ? ` with role ${role}` : ''}. Use \`look\` to see what is on screen.`)
  }

  const settle = async () => {
    await page.waitForLoadState('networkidle').catch(() => {})
    await page.waitForTimeout(150)
  }

  const actions = {
    async look() {
      const path = join(dir, `screen-${String(++shot).padStart(3, '0')}.png`)
      await page.screenshot({ path })
      const tree = await page.locator('body').ariaSnapshot()
      const errors = consoleErrors.splice(0)
      return `screenshot: ${path}\nscreen: ${page.viewportSize().width}x${page.viewportSize().height}\n` +
        (errors.length ? `console errors:\n  ${errors.join('\n  ')}\n` : '') + `accessibility tree:\n${tree}`
    },
    async click({ label, role, nth, at, double }) {
      if (at) {
        const [x, y] = xy(at)
        await (double ? page.mouse.dblclick(x, y) : page.mouse.click(x, y))
        await settle()
        return `${double ? 'double-' : ''}clicked at ${x},${y}`
      }
      const { el, count } = await find(label, role, Number(nth ?? 0))
      await (double ? el.dblclick() : el.click())
      await settle()
      return `${double ? 'double-' : ''}clicked "${label}"${count > 1 ? ` (match ${Number(nth ?? 0) + 1} of ${count}; use --nth to pick another)` : ''}`
    },
    async hover({ label, role, at }) {
      if (at) {
        const [x, y] = xy(at)
        await page.mouse.move(x, y)
      } else await (await find(label, role)).el.hover()
      await settle()
      return `hovering ${at ?? `"${label}"`}`
    },
    async type({ text }) {
      await page.keyboard.type(text, { delay: 15 })
      await settle()
      return `typed "${text}"`
    },
    async fill({ label, text }) {
      const { el } = await find(label)
      await el.fill(text)
      await settle()
      return `filled "${label}" with "${text}"`
    },
    async press({ key }) {
      await page.keyboard.press(key)
      await settle()
      const focused = await page.evaluate(() => {
        const el = document.activeElement
        return el && el !== document.body ? `${el.tagName.toLowerCase()} "${el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 40) ?? ''}"` : 'nothing'
      })
      return `pressed ${key}; focus is on ${focused}`
    },
    async drag({ from, to }) {
      const [x1, y1] = xy(from)
      const [x2, y2] = xy(to)
      await page.mouse.move(x1, y1)
      await page.mouse.down()
      await page.mouse.move(x2, y2, { steps: 12 })
      await page.mouse.up()
      await settle()
      return `dragged ${from} → ${to}`
    },
    async scroll({ delta, at }) {
      const [dx, dy] = xy(delta)
      if (at) await page.mouse.move(...xy(at))
      await page.mouse.wheel(dx, dy)
      await settle()
      return `scrolled ${delta}`
    },
    async resize({ width, height }) {
      await page.setViewportSize({ width: Number(width), height: Number(height) })
      await settle()
      return `screen is now ${width}x${height}`
    },
    async goto({ url }) {
      await page.goto(url)
      await settle()
      return `opened ${url}`
    }
  }

  const server = createServer(async (req, res) => {
    let body = ''
    for await (const chunk of req) body += chunk
    const { action, ...params } = JSON.parse(body || '{}')
    try {
      if (action === 'stop') {
        res.end(JSON.stringify({ ok: true, out: 'session stopped' }))
        await browser.close()
        server.close()
        return
      }
      const out = await actions[action](params)
      res.end(JSON.stringify({ ok: true, out }))
    } catch (error) {
      res.end(JSON.stringify({ ok: false, out: String(error.message ?? error) }))
    }
  })
  server.listen(0, '127.0.0.1', () => {
    writeFileSync(join(dir, 'port'), String(server.address().port))
  })
} else if (command === 'start') {
  // ── Client commands ────────────────────────────────────────────────────
  const dir = dirOf(name)
  rmSync(dir, { recursive: true, force: true })
  mkdirSync(dir, { recursive: true })
  const child = spawn(process.execPath, [import.meta.filename, '__serve', name, ...rest], {
    cwd: ROOT,
    detached: true,
    stdio: 'ignore'
  })
  child.unref()
  for (let i = 0; i < 100 && !existsSync(join(dir, 'port')); i++) await new Promise((r) => setTimeout(r, 100))
  if (!existsSync(join(dir, 'port'))) {
    console.error(`session "${name}" did not start (is web mode running at ${DEFAULT_URL}?)`)
    process.exit(1)
  }
  console.log(`session "${name}" started. Next: node e2e/tools/session.mjs look ${name}`)
} else {
  const portFile = join(dirOf(name), 'port')
  if (!existsSync(portFile)) {
    console.error(`no session "${name}". Start one: node e2e/tools/session.mjs start ${name}`)
    process.exit(1)
  }
  const params = { action: command }
  switch (command) {
    case 'click':
    case 'dblclick':
    case 'hover':
      Object.assign(params, { action: command === 'dblclick' ? 'click' : command, double: command === 'dblclick', at: flag('at'), role: flag('role'), nth: flag('nth'), label: rest[0] })
      break
    case 'type':
      params.text = rest[0]
      break
    case 'fill':
      Object.assign(params, { label: rest[0], text: rest[1] })
      break
    case 'press':
      params.key = rest[0]
      break
    case 'drag':
      Object.assign(params, { from: rest[0], to: rest[1] })
      break
    case 'scroll':
      Object.assign(params, { at: flag('at'), delta: rest[0] })
      break
    case 'resize':
      Object.assign(params, { width: rest[0], height: rest[1] })
      break
    case 'goto':
      params.url = rest[0]
      break
  }
  const port = readFileSync(portFile, 'utf8')
  const response = await fetch(`http://127.0.0.1:${port}`, { method: 'POST', body: JSON.stringify(params) }).catch(() => null)
  if (!response) {
    console.error(`session "${name}" is not responding; start it again`)
    process.exit(1)
  }
  const { ok, out } = await response.json()
  if (command === 'stop') rmSync(portFile, { force: true })
  ;(ok ? console.log : console.error)(out)
  process.exit(ok ? 0 : 1)
}
