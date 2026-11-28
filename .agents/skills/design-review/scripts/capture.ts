#!/usr/bin/env bun
/**
 * Capture a Storybook for a design review: every story, as pictures and as findings against the `auto` rules of the
 * ui-styleguide rulebook (`.agents/skills/ui-styleguide/references/auto-checks.md`; what is covered and how is in
 * `../references/checks.md`).
 *
 *   bun .agents/skills/design-review/scripts/capture.ts <book> [--build] [--story <text>] [--out <dir>] [--no-interact]
 *
 * <book> is a Storybook's folder name, as `bun scripts/storybooks.ts list` prints it: an app (`notes`), a package
 * (`ui`, `kit`) or `client` (the shell). `--build` builds its storybook-static first (it is built when missing).
 * `--story` keeps the stories whose id, title or name contain the text. `--no-interact` skips the keyboard, pointer
 * and Escape checks (faster; their rules are then not covered).
 *
 * The runs (auto-checks.md, Runs), each a fresh load of the story:
 *   default        iPad-sized page (a story tagged `app`: its AppWindow at full, Split View half and Slide Over size),
 *                  light: every rule; then, once per story, the keyboard, pointer and Escape checks
 *   dark           the same sizes in dark: the rules whose result depends on the theme (colour, contrast, materials)
 *   large-text     --text-scale 1.25 on the root, light, first size: TYPE-02 and A11Y-16
 *   smallest       a story tagged `app` in a 320 × 240 window, light: LAYOUT-12
 *   reduced        Reduced Motion (emulated) and Reduce Transparency (class on the root), light, first size: A11Y-15
 *                  and MOTION-03 (c). One load for both settings: neither changes what the other's rule reads.
 * After each load: two frames, then until no finite animation runs or 500 ms.
 *
 * Written to <out> (default test-results/design-review/<book>/):
 *   shots/<story>__<size>__<run>.png   the pictures, to look at one by one
 *   summary.md                         the findings by rule ID, each once, with where it was seen and the picture
 *   styles.md                          every distinct text style drawn, with sample text: the type inventory
 *   findings.json, texts.json          everything, for a script
 */
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { chromium, type CDPSession, type Page } from '@playwright/test'
import { audit, type AuditFinding, type AuditOptions, type AuditResult, type Run, type TextSample } from './audit'

/** The in-page audit, typed. */
const runAudit = (page: Page, o: AuditOptions): Promise<AuditResult> => page.evaluate(audit, o)

interface IndexEntry {
  type: 'story' | 'docs'
  id: string
  title: string
  name: string
  tags?: string[]
}
interface Size {
  name: string
  viewport: { width: number; height: number }
  /** The AppWindow frame's size, for a story tagged `app`. */
  frame?: { width: number; height: number }
}
interface Render {
  story: string
  title: string
  name: string
  size: string
  run: Run
  theme: 'light' | 'dark'
  scale: number
  shot: string
  findings: AuditFinding[]
  texts: TextSample[]
}

const ROOT = resolve(import.meta.dir, '../../../..')
const args = process.argv.slice(2)
const book = args.find((a, i) => !a.startsWith('--') && args[i - 1] !== '--story' && args[i - 1] !== '--out')
if (!book) {
  console.error('usage: capture.ts <book> [--build] [--story <text>] [--out <dir>] [--no-interact]')
  process.exit(2)
}
const option = (name: string): string | undefined => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined)
const only = option('--story')?.toLowerCase()
const interact = !args.includes('--no-interact')
const out = resolve(option('--out') ?? join(ROOT, 'test-results', 'design-review', book))

const dir = [join(ROOT, 'apps', book), join(ROOT, 'packages', book), join(ROOT, book)].find((d) => existsSync(join(d, '.storybook')))
if (!dir) {
  console.error(`${book}: no Storybook (no .storybook/ under apps/, packages/ or the root)`)
  process.exit(2)
}
const rel = relative(ROOT, dir).replaceAll('\\', '/')
const surface: AuditOptions['surface'] = rel.startsWith('apps/') ? 'app' : rel === 'packages/kit' ? 'kit' : 'shell'
const staticDir = join(dir, 'storybook-static')
if (args.includes('--build') || !existsSync(join(staticDir, 'index.json'))) {
  console.log(`building ${rel}'s Storybook…`)
  const built = spawnSync('bun', ['run', 'build-storybook'], { cwd: dir, stdio: 'inherit', shell: process.platform === 'win32' })
  if (built.status !== 0) process.exit(built.status ?? 1)
}

const { entries } = JSON.parse(readFileSync(join(staticDir, 'index.json'), 'utf8')) as { entries: Record<string, IndexEntry> }
const stories = Object.values(entries).filter((e) => e.type === 'story' && (!only || `${e.id} ${e.title} ${e.name}`.toLowerCase().includes(only)))
if (!stories.length) {
  console.error(`${book}: no stories${only ? ` matching "${only}"` : ''}`)
  process.exit(2)
}

// rule titles, for the summary
const titles = new Map<string, string>()
const index = join(ROOT, '.agents/skills/ui-styleguide/references/index.md')
if (existsSync(index)) for (const m of readFileSync(index, 'utf8').matchAll(/^\| ([A-Z0-9]+-\d+) \| ([^|]+) \|/gm)) titles.set(m[1], m[2].trim())

const server = Bun.serve({
  port: 0,
  fetch(req) {
    const path = decodeURIComponent(new URL(req.url).pathname)
    const file = Bun.file(join(staticDir, path === '/' ? 'index.html' : path))
    return file.size ? new Response(file) : new Response('not found', { status: 404 })
  }
})

const IPAD: Size = { name: 'ipad', viewport: { width: 1180, height: 820 } }
const APP_SIZES: Size[] = [
  { name: 'full', viewport: { width: 1280, height: 900 }, frame: { width: 1180, height: 780 } },
  { name: 'half', viewport: { width: 1280, height: 900 }, frame: { width: 590, height: 780 } },
  { name: 'slideover', viewport: { width: 1280, height: 900 }, frame: { width: 375, height: 700 } }
]
const SMALLEST: Size = { name: 'smallest', viewport: { width: 1280, height: 900 }, frame: { width: 320, height: 240 } }
const LARGE_TEXT = 1.25

/** Before any page script: browser dialogs are recorded (MODAL-02), and so are the elements given a contextmenu listener (MENU-02). */
function instrument(): void {
  const w = window as unknown as Record<string, unknown>
  const state = { dialogs: [] as string[], contextmenu: [] as EventTarget[], focusAtOpen: null as Element | null, blank: null as boolean | null }
  w.__designReview = state
  for (const name of ['alert', 'confirm', 'prompt']) {
    w[name] = (message?: unknown): unknown => {
      state.dialogs.push(`${name}(${String(message ?? '').slice(0, 60)})`)
      return name === 'confirm' ? false : name === 'prompt' ? null : undefined
    }
  }
  const add = EventTarget.prototype.addEventListener
  EventTarget.prototype.addEventListener = function (this: EventTarget, type: string, listener: EventListenerOrEventListenerObject | null, opts?: boolean | AddEventListenerOptions): void {
    if (type === 'contextmenu' && this instanceof Element) state.contextmenu.push(this)
    return add.call(this, type, listener, opts)
  }
}

/** Two frames, then until no finite animation runs (document and shadow roots) or 500 ms. */
async function settle(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const frames = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())))
    await frames()
    const roots: (Document | ShadowRoot)[] = [document]
    const walk = (r: Document | ShadowRoot | Element): void => {
      for (const el of Array.from(r.querySelectorAll('*'))) if (el.shadowRoot) {
        roots.push(el.shadowRoot)
        walk(el.shadowRoot)
      }
    }
    walk(document)
    const t0 = performance.now()
    while (performance.now() - t0 < 500) {
      const running = roots.flatMap((r) => r.getAnimations()).filter((a) => a.playState === 'running' && Number(a.effect?.getComputedTiming().endTime ?? 0) < Infinity)
      if (!running.length) break
      await new Promise((r) => setTimeout(r, 25))
    }
  })
}

/** Accessible names of the elements `audit({ mode: 'mark' })` tagged, from Chromium's accessibility tree. */
async function axNames(cdp: CDPSession): Promise<Record<string, string>> {
  const { nodes } = (await cdp.send('Accessibility.getFullAXTree')) as { nodes: { backendDOMNodeId?: number; ignored?: boolean; name?: { value?: unknown } }[] }
  const names = new Map<number, string>()
  for (const n of nodes) if (n.backendDOMNodeId !== undefined) names.set(n.backendDOMNodeId, n.ignored ? '' : String(n.name?.value ?? ''))
  interface DomNode {
    backendNodeId: number
    attributes?: string[]
    children?: DomNode[]
    shadowRoots?: DomNode[]
    contentDocument?: DomNode
  }
  const { root } = (await cdp.send('DOM.getDocument', { depth: -1, pierce: true })) as { root: DomNode }
  const out: Record<string, string> = {}
  const walk = (node: DomNode): void => {
    const a = node.attributes
    if (a) for (let i = 0; i < a.length; i += 2) if (a[i] === 'data-dr-n' && names.has(node.backendNodeId)) out[a[i + 1]] = names.get(node.backendNodeId)!
    for (const c of node.children ?? []) walk(c)
    for (const c of node.shadowRoots ?? []) walk(c)
  }
  walk(root)
  return out
}

interface Plan {
  size: Size
  run: Run
}

async function show(page: Page, story: IndexEntry, plan: Plan, first: boolean, opts: AuditOptions): Promise<void> {
  const { size, run } = plan
  await page.setViewportSize(size.viewport)
  await page.emulateMedia({ reducedMotion: run === 'reduced' ? 'reduce' : 'no-preference' })
  const frame = size.frame ? `&args=width:${size.frame.width};height:${size.frame.height}` : ''
  const themeName = run === 'dark' ? 'Dark' : 'Light'
  await page.goto(`http://localhost:${server.port}/iframe.html?id=${story.id}&viewMode=story&globals=theme:${themeName}${frame}`)
  await page.waitForFunction(
    () => {
      const p = (window as unknown as { __STORYBOOK_PREVIEW__?: { currentRender?: { phase?: string } } }).__STORYBOOK_PREVIEW__?.currentRender?.phase
      return p === 'finished' || p === 'completed' || p === 'errored' || p === 'aborted'
    },
    undefined,
    { timeout: 20_000 }
  )
  if (first) {
    // FEED-09 200 ms after the view is shown; what has the focus 300 ms after it opened (CTRL-04, MODAL-12, MODAL-13)
    await page.waitForTimeout(200)
    if (opts.app) {
      const r = await runAudit(page, { ...opts, mode: 'blank' })
      await page.evaluate((b) => {
        const st = (window as unknown as { __designReview?: { blank: boolean | null } }).__designReview
        if (st) st.blank = b
      }, r.blank ?? null)
    }
    await page.waitForTimeout(100)
  }
  const keepFocus = await page.evaluate((record) => {
    let at = document.activeElement
    while (at?.shadowRoot?.activeElement) at = at.shadowRoot.activeElement
    const st = (window as unknown as { __designReview?: { focusAtOpen: Element | null } }).__designReview
    if (record && st) st.focusAtOpen = at === document.body ? null : at
    // the focus stays in a modal that took it (MODAL-04 starts from there)
    for (let e: Element | null = at; e; e = e.assignedSlot ?? e.parentElement ?? ((e.getRootNode() as ShadowRoot).host ?? null)) {
      if (e.matches('dialog[open], [role=dialog], [role=alertdialog]')) return true
    }
    return false
  }, first)
  if (!story.tags?.includes('keep-focus') && !keepFocus) await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  if (run === 'large-text') await page.evaluate((s) => document.documentElement.style.setProperty('--text-scale', String(s)), LARGE_TEXT)
  if (run === 'reduced') await page.evaluate(() => document.documentElement.classList.add('reduce-transparency'))
  // ask for the bundled face even where a story draws no text in it, so the audit can tell "missing" from "unused"
  await page.evaluate(() => document.fonts.load('17px InterVariable').then(() => document.fonts.ready, () => document.fonts.ready))
  await settle(page)
}

const finding = (check: string, severity: AuditFinding['severity'], element: string, text: string, detail: string, box = { x: 0, y: 0, w: 0, h: 0 }): AuditFinding => ({ check, severity, element, text, detail, box })

/** The checks that operate the page: keyboard, pointer, Escape. Once per story, after the default run's audit. */
async function operate(page: Page, opts: AuditOptions): Promise<AuditFinding[]> {
  const found: AuditFinding[] = []
  const step = async (name: string, fn: () => Promise<void>): Promise<void> => {
    try {
      await fn()
    } catch (e) {
      found.push(finding('capture', 'info', 'page', name, `could not run: ${(e as Error).message.split('\n')[0]}`))
    }
  }
  // what is open at load: modals, popovers, menus (MODAL-04, MODAL-05, INPUT-09)
  const openAtLoad = await page.evaluate(() => {
    const all: Element[] = []
    const walk = (r: Document | ShadowRoot): void => {
      for (const el of Array.from(r.querySelectorAll('*'))) {
        all.push(el)
        if (el.shadowRoot) walk(el.shadowRoot)
      }
    }
    walk(document)
    const shown = (e: Element): boolean => e.getClientRects().length > 0 && getComputedStyle(e).visibility === 'visible'
    let n = 0
    const out: { id: string; kind: 'alert' | 'sheet' | 'popover' | 'menu'; kit: boolean }[] = []
    for (const e of all) {
      if (!shown(e)) continue
      const role = e.getAttribute('role')
      let kind: 'alert' | 'sheet' | 'popover' | 'menu' | null = null
      if (role === 'alertdialog') kind = 'alert'
      else if ((e.localName === 'dialog' && e.matches(':modal')) || (role === 'dialog' && e.getAttribute('aria-modal') === 'true')) kind = 'sheet'
      else if (role === 'menu') kind = 'menu'
      else if (e.matches(':popover-open') || (role === 'dialog' && !e.classList.contains('ios-window') && /absolute|fixed/.test(getComputedStyle(e).position))) kind = 'popover'
      if (!kind) continue
      const id = `m${n++}`
      e.setAttribute('data-dr-open', id)
      const host = (e.getRootNode() as ShadowRoot).host
      out.push({ id, kind, kit: host?.localName === 'ui-alert' })
    }
    return out
  })

  // A11Y-11, A11Y-14, MODAL-04: the Tab walk
  await step('tab walk', async () => {
    const stops: { key: string; element: string; text: string; inModal: boolean; detail: string; box: AuditFinding['box'] }[] = []
    let nulls = 0
    for (let i = 0; i < 300; i++) {
      await page.keyboard.press('Tab')
      const r = await runAudit(page, { ...opts, mode: 'focus' })
      const f = r.focus ?? null
      if (!f) {
        if (++nulls >= 2) break
        continue
      }
      nulls = 0
      if (stops.length && f.key === stops[0].key) break
      stops.push(f)
    }
    const reported = new Set<string>()
    for (const s of stops) {
      if (!s.detail || reported.has(s.element)) continue
      reported.add(s.element)
      found.push(finding('A11Y-11', 'error', s.element, s.text, s.detail, s.box))
    }
    if (stops.length >= 300 - 1) {
      const last = stops.slice(-100)
      const keys = new Set(last.map((s) => s.key))
      if (keys.size <= 20 && !keys.has(stops[0].key)) found.push(finding('A11Y-14', 'error', last[0].element, last[0].text, `Tab cycles among ${keys.size} elements (${[...new Set(last.map((s) => s.element))].slice(0, 3).join('; ')}) and never leaves them`))
    }
    if (openAtLoad.some((m) => m.kind === 'alert' || m.kind === 'sheet')) {
      const out20 = stops.slice(0, 20).find((s) => !s.inModal)
      if (out20) found.push(finding('MODAL-04', 'error', out20.element, out20.text, 'Tab moves the focus out of the open modal'))
      const inside = await page.evaluate(() => {
        const f = (window as unknown as { __designReview?: { focusAtOpen: Element | null } }).__designReview?.focusAtOpen
        for (let e: Element | null = f ?? null; e; e = e.assignedSlot ?? e.parentElement ?? ((e.getRootNode() as ShadowRoot).host ?? null)) if (e.matches('dialog[open], [role=dialog], [role=alertdialog]')) return true
        return false
      })
      if (!inside) found.push(finding('MODAL-04', 'error', 'modal', '', 'the focus is not inside the open modal 300 ms after it opened'))
    }
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  })

  // A11Y-18, INPUT-02: hover
  await step('hover', async () => {
    const r = await runAudit(page, { ...opts, mode: 'hover-list' })
    for (const c of r.hover ?? []) {
      const read = async (): Promise<{ visible: boolean; scale: number }> => (await runAudit(page, { ...opts, mode: 'hover-read', target: c.n })).read ?? { visible: false, scale: 1 }
      if (c.kind === 'INPUT-02') {
        await page.mouse.move(0, 0)
        const rest = await read()
        await page.locator(`[data-dr-h="${c.hover}"]`).first().hover({ force: true, timeout: 1000 })
        await page.waitForTimeout(30)
        const hov = await read()
        if (hov.scale > 1.001 && hov.scale > rest.scale + 0.001) found.push(finding('INPUT-02', 'error', c.element, c.text, `hover scales it to ${hov.scale.toFixed(3)}; hover highlights, it does not grow`))
      } else {
        await page.locator(`[data-dr-h="${c.hover}"]`).first().hover({ force: true, timeout: 1000 })
        await page.waitForTimeout(30)
        const hov = await read()
        if (!hov.visible) continue
        await page.mouse.move(0, 0)
        await page.locator(`[data-dr-h="${c.focus}"]`).first().focus({ timeout: 1000 })
        await page.waitForTimeout(30)
        const foc = await read()
        if (!foc.visible) found.push(finding('A11Y-18', 'error', c.element, c.text, 'shown on hover but not when it has the focus: a keyboard cannot reach it seen'))
        await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
      }
    }
    await page.mouse.move(0, 0)
  })

  // CTRL-10: toggle buttons
  await step('toggle buttons', async () => {
    const r = await runAudit(page, { ...opts, mode: 'ctrl10' })
    found.push(...r.findings)
  })

  // MODAL-05, INPUT-09: Escape on what is open at load
  if (openAtLoad.length) {
    await step('escape', async () => {
      await page.evaluate(() => {
        const w = window as unknown as { __drConfirmed?: boolean }
        w.__drConfirmed = false
        const walk = (r: Document | ShadowRoot): void => {
          for (const el of Array.from(r.querySelectorAll('*'))) {
            if (el.localName === 'ui-alert') el.addEventListener('confirm', () => (w.__drConfirmed = true))
            if (el.shadowRoot) walk(el.shadowRoot)
          }
        }
        walk(document)
      })
      const top = openAtLoad[openAtLoad.length - 1]
      // a person's focus is in what is open: where it was 300 ms after opening, or its first control
      await page.evaluate((id) => {
        const find = (r: Document | ShadowRoot): Element | null => {
          const hit = r.querySelector(`[data-dr-open="${id}"]`)
          if (hit) return hit
          for (const el of Array.from(r.querySelectorAll('*'))) if (el.shadowRoot) {
            const h = find(el.shadowRoot)
            if (h) return h
          }
          return null
        }
        const surface = find(document)
        if (!surface) return
        const st = (window as unknown as { __designReview?: { focusAtOpen: Element | null } }).__designReview
        const f = st?.focusAtOpen
        let inside = false
        for (let e: Element | null = f ?? null; e; e = e.assignedSlot ?? e.parentElement ?? ((e.getRootNode() as ShadowRoot).host ?? null)) if (e === surface) inside = true
        if (f && inside && f instanceof HTMLElement) return f.focus()
        const walk = (r: Element | ShadowRoot): HTMLElement | null => {
          for (const el of Array.from(r.querySelectorAll('*'))) {
            if (el instanceof HTMLElement && el.matches('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"]), [role=menuitem]') && el.getClientRects().length) return el
            if (el.shadowRoot) {
              const h = walk(el.shadowRoot)
              if (h) return h
            }
          }
          return null
        }
        const target = walk(surface)
        if (target) target.focus()
        else if (surface instanceof HTMLElement) surface.focus()
      }, top.id)
      await page.keyboard.press('Escape')
      await page.waitForTimeout(300)
      const state = await page.evaluate((ids) => {
        const find = (id: string): Element | null => {
          const walk = (r: Document | ShadowRoot): Element | null => {
            const hit = r.querySelector(`[data-dr-open="${id}"]`)
            if (hit) return hit
            for (const el of Array.from(r.querySelectorAll('*'))) if (el.shadowRoot) {
              const h = walk(el.shadowRoot)
              if (h) return h
            }
            return null
          }
          return walk(document)
        }
        const open: Record<string, boolean> = {}
        for (const id of ids) {
          const e = find(id)
          open[id] = !!e && e.isConnected && e.getClientRects().length > 0 && getComputedStyle(e).visibility === 'visible'
        }
        return { open, confirmed: (window as unknown as { __drConfirmed?: boolean }).__drConfirmed === true }
      }, openAtLoad.map((m) => m.id))
      if (top.kind !== 'sheet') {
        if (state.open[top.id]) found.push(finding('MODAL-05', 'error', `${top.kind} [data-dr-open=${top.id}]`, '', `the ${top.kind} is still open 300 ms after Escape`))
        if (state.confirmed) found.push(finding('MODAL-05', 'error', `${top.kind} [data-dr-open=${top.id}]`, '', 'Escape ran the alert\'s confirm action; Escape means Cancel'))
        const sheet = openAtLoad.find((m) => m.kind === 'sheet')
        if (sheet && (top.kind === 'menu' || top.kind === 'popover') && !state.open[sheet.id]) found.push(finding('INPUT-09', 'error', `sheet [data-dr-open=${sheet.id}]`, '', 'one Escape on a menu over a sheet closed the sheet too'))
      }
    })
  }

  // INPUT-10: focus follows the view in a collapsed split view
  await step('split view', async () => {
    const ok = await page.evaluate(() => {
      const sv = document.querySelector('ui-split-view[collapsed]')
      if (!sv || !sv.getClientRects().length) return false
      const front = sv.shadowRoot?.querySelector('slot.front') as HTMLSlotElement | null
      const rows = (front?.assignedElements({ flatten: true }) ?? []).flatMap((e) => (e.localName === 'ui-row' ? [e] : Array.from(e.querySelectorAll('ui-row'))))
      const row = rows.find((r) => (r as unknown as { focusable?: boolean }).focusable) as (Element & { focusRow?: () => void }) | undefined
      if (!row?.focusRow) return false
      row.setAttribute('data-dr-split', '')
      row.focusRow()
      return true
    })
    if (!ok) return
    await page.keyboard.press('Enter')
    await page.waitForTimeout(300)
    const r = await page.evaluate(() => {
      const sv = document.querySelector('ui-split-view[collapsed]')
      let at = document.activeElement
      while (at?.shadowRoot?.activeElement) at = at.shadowRoot.activeElement
      if (!sv || !at || at === document.body) return 'the focus fell to the page body'
      const front = sv.shadowRoot?.querySelector('slot.front') as HTMLSlotElement | null
      const inFront = (front?.assignedElements({ flatten: true }) ?? []).some((e) => {
        for (let x: Element | null = at; x; x = x.assignedSlot ?? x.parentElement ?? ((x.getRootNode() as ShadowRoot).host ?? null)) if (x === e) return true
        return false
      })
      return inFront ? '' : `the focus stayed outside the column now in front (${front?.name || 'detail'})`
    })
    if (r) found.push(finding('INPUT-10', 'error', 'ui-split-view[collapsed]', '', `after Enter on a row: ${r}`))
  })

  // FORM-25: a banner is for the window, not for input
  await step('forms', async () => {
    const n = await page.evaluate(() => {
      const forms = Array.from(document.querySelectorAll('form, ui-list')).filter((f) => f.getClientRects().length)
      let k = 0
      for (const f of forms) {
        const field = f.querySelector('ui-text-field[required], input[required], ui-text-field[type=email], ui-text-field[type=url], input[type=email], input[type=url]') as (HTMLElement & { value?: string }) | null
        if (!field) continue
        field.setAttribute('data-dr-form', String(k++))
      }
      return k
    })
    for (let k = 0; k < Math.min(n, 3); k++) {
      const before = await page.evaluate((i) => {
        const banners = Array.from(document.querySelectorAll('ui-banner')).filter((b) => b.getClientRects().length).length
        const field = document.querySelector(`[data-dr-form="${i}"]`) as (HTMLElement & { value?: string; type?: string }) | null
        if (!field) return -1
        const type = field.getAttribute('type') ?? field.type ?? 'text'
        const bad = type === 'email' ? 'not-an-email' : type === 'url' ? 'not a url' : ''
        const inner = (field.shadowRoot?.querySelector('input') ?? field) as HTMLInputElement
        field.value = bad
        inner.value = bad
        inner.dispatchEvent(new Event('input', { bubbles: true, composed: true }))
        inner.focus()
        return banners
      }, k)
      if (before < 0) continue
      await page.keyboard.press('Enter')
      await page.waitForTimeout(1000)
      const after = await page.evaluate(() => Array.from(document.querySelectorAll('ui-banner')).filter((b) => b.getClientRects().length).length)
      if (after > before) found.push(finding('FORM-25', 'error', `[data-dr-form=${k}]`, '', 'submitting a form with an invalid field raised a banner; the field and its section footer say it'))
    }
  })

  // MENU-14: last, it can open the app's own menus
  await step('edit menu', async () => {
    const r = await runAudit(page, { ...opts, mode: 'menu14' })
    found.push(...r.findings)
  })
  return found
}

// a fresh capture, beside what lint-source.ts wrote there
for (const name of ['shots', 'summary.md', 'styles.md', 'findings.json', 'texts.json']) rmSync(join(out, name), { recursive: true, force: true })
mkdirSync(join(out, 'shots'), { recursive: true })
const browser = await chromium.launch()
const page = await browser.newPage({ deviceScaleFactor: 1 })
await page.addInitScript(instrument)
const cdp = await page.context().newCDPSession(page)
await cdp.send('DOM.enable')
await cdp.send('Accessibility.enable')
const renders: Render[] = []
const failed: string[] = []
for (const story of stories) {
  const app = !!story.tags?.includes('app')
  const sizes = app ? APP_SIZES : [IPAD]
  const plan: Plan[] = []
  for (const size of sizes) for (const run of ['default', 'dark'] as const) plan.push({ size, run })
  plan.push({ size: sizes[0], run: 'large-text' })
  if (app) plan.push({ size: SMALLEST, run: 'smallest' })
  plan.push({ size: sizes[0], run: 'reduced' })
  let first = true
  let played = false
  for (const p of plan) {
    const label = `${story.id}__${p.size.name}__${p.run === 'default' ? 'light' : p.run}`
    const opts: AuditOptions = { run: p.run, app, surface, played }
    try {
      await show(page, story, p, first, opts)
      if (first) {
        // a play function that operates the page: its layout shifts follow (synthetic) input
        played = await page.evaluate(() => /userEvent|fireEvent|\.click\(|keyboard|\.type\(/.test(String((window as unknown as { __STORYBOOK_PREVIEW__?: { currentRender?: { story?: { moduleExport?: { play?: unknown } } } } }).__STORYBOOK_PREVIEW__?.currentRender?.story?.moduleExport?.play ?? '')))
        opts.played = played
      }
      const shot = join(out, 'shots', `${label}.png`)
      const target = p.size.frame ? page.getByTestId('app-window').first() : page.locator('#storybook-root')
      await target.screenshot({ path: shot, animations: 'disabled', timeout: 5000 }).catch(() => page.screenshot({ path: shot, fullPage: true }))
      let names: Record<string, string> | undefined
      if (p.run === 'default' || p.run === 'dark') {
        await runAudit(page, { ...opts, mode: 'mark' })
        names = await axNames(cdp).catch(() => undefined)
      }
      const result = await runAudit(page, { ...opts, mode: 'check', names })
      if (first && interact) result.findings.push(...(await operate(page, { ...opts, names })))
      renders.push({ story: story.id, title: story.title, name: story.name, size: p.size.name, run: p.run, theme: p.run === 'dark' ? 'dark' : 'light', scale: p.run === 'large-text' ? LARGE_TEXT : 1, shot, findings: result.findings, texts: result.texts })
    } catch (e) {
      failed.push(`${label}: ${(e as Error).message.split('\n')[0]}`)
    }
    first = false
  }
  process.stdout.write('.')
}
await browser.close()
server.stop()
console.log()

// ---- cross-run rules -------------------------------------------------------------------------------------------
const fkey = (f: AuditFinding): string => `${f.check}|${f.element}|${f.text}`
for (const r of renders) {
  if (r.run !== 'large-text' && r.run !== 'smallest') continue
  const base = renders.find((x) => x.story === r.story && x.run === 'default' && x.size === (r.run === 'smallest' ? 'full' : r.size))
  if (!base) continue
  const seen = new Set(base.findings.map(fkey))
  const kept: AuditFinding[] = []
  for (const f of r.findings) {
    const overflow = r.run === 'large-text' ? /^TYPE-2[678]$/.test(f.check) : /^TYPE-2[78]$/.test(f.check)
    if (!overflow) {
      kept.push(f)
      continue
    }
    if (seen.has(fkey(f))) continue // reported in the default run already
    kept.push({ ...f, check: r.run === 'large-text' ? 'A11Y-16' : 'LAYOUT-12', detail: `${r.run === 'large-text' ? `at text scale ${LARGE_TEXT}` : 'in a 320 × 240 window'}, not at the default: ${f.check} ${f.detail}` })
  }
  r.findings = kept
  // TYPE-02: every text grows with the Text Size setting
  if (r.run === 'large-text') {
    for (const t of r.texts) {
      const before = base.texts.find((n) => n.element === t.element && n.text === t.text)
      if (before && Math.abs(t.size - LARGE_TEXT * before.size) > 0.5) r.findings.push(finding('TYPE-02', 'error', t.element, t.text, `${before.size}px at text scale 1 and ${t.size}px at ${LARGE_TEXT}; expected ${+(LARGE_TEXT * before.size).toFixed(2)}px`))
    }
  }
}

writeFileSync(join(out, 'findings.json'), JSON.stringify(renders.map(({ texts: _, ...r }) => r), null, 1))
writeFileSync(join(out, 'texts.json'), JSON.stringify(renders.map((r) => ({ story: r.story, size: r.size, run: r.run, scale: r.scale, texts: r.texts })), null, 1))

// ---- summary.md: each finding once (rule + element + detail), with every render it was seen in --------------------
const SEVERITY = { error: 0, warn: 1, info: 2 }
interface Grouped {
  f: AuditFinding
  where: string[]
  shot: string
}
const groups = new Map<string, Map<string, Grouped>>()
for (const r of renders) {
  for (const f of r.findings) {
    const byCheck = groups.get(f.check) ?? new Map<string, Grouped>()
    groups.set(f.check, byCheck)
    const k = `${f.element}|${f.detail.replace(/[\d.]+(px|:1| ms)?/g, 'N')}|${f.text}`
    const g = byCheck.get(k) ?? { f, where: [], shot: relative(out, r.shot).replaceAll('\\', '/') }
    const at = `${r.story} (${r.size}, ${r.run === 'default' ? 'light' : r.run})`
    if (!g.where.includes(at)) g.where.push(at)
    byCheck.set(k, g)
  }
}
const lines = [
  `# Design review capture: ${book}`,
  '',
  `${stories.length} stories, ${renders.length} renders, at ${new Date().toISOString()}. Pictures in \`shots/\`. Each check is a rule ID of \`.agents/skills/ui-styleguide/references/index.md\`; what the machine covers, and what it leaves to you, is in \`.agents/skills/design-review/references/checks.md\`. \`warn\` marks a finding to confirm by looking (a **review** entry); \`error\` a failure of the rule as specified.`,
  ''
]
if (failed.length) lines.push('## Renders that failed', '', ...failed.map((f) => `- ${f}`), '')
const ruleOrder = (id: string): string => id.replace(/\d+/, (d) => d.padStart(3, '0'))
const order = [...groups.entries()].sort(([a, ga], [b, gb]) => SEVERITY[[...ga.values()][0].f.severity] - SEVERITY[[...gb.values()][0].f.severity] || ruleOrder(a).localeCompare(ruleOrder(b)))
lines.push('## Findings by rule', '', '| rule | title | severity | distinct |', '|---|---|---|---|')
for (const [check, byCheck] of order) lines.push(`| ${check} | ${titles.get(check) ?? ''} | ${[...byCheck.values()][0].f.severity} | ${byCheck.size} |`)
lines.push('')
for (const [check, byCheck] of order) {
  lines.push(`## ${check}${titles.has(check) ? ` ${titles.get(check)}` : ''}`, '')
  const items = [...byCheck.values()].sort((a, b) => SEVERITY[a.f.severity] - SEVERITY[b.f.severity])
  for (const g of items.slice(0, 60)) {
    const seen = g.where.length > 3 ? `${g.where.slice(0, 3).join('; ')} and ${g.where.length - 3} more` : g.where.join('; ')
    lines.push(`- **${g.f.severity}** \`${g.f.element}\`${g.f.text ? ` “${g.f.text}”` : ''}: ${g.f.detail}. Seen in ${seen}. Picture: \`${g.shot}\``)
  }
  if (items.length > 60) lines.push(`- … and ${items.length - 60} more in findings.json`)
  lines.push('')
}
writeFileSync(join(out, 'summary.md'), lines.join('\n'))

// ---- styles.md: the type inventory at text scale 1, light (colours are named by token) ------------------------------
const styles = new Map<string, { t: TextSample; count: number; samples: Set<string>; stories: Set<string> }>()
for (const r of renders.filter((x) => x.run === 'default')) {
  for (const t of r.texts) {
    const family = t.family.split(',')[0].replace(/["']/g, '').trim()
    const k = [family, t.size, t.lineHeight, t.weight, t.tracking, t.colorToken || t.color, t.transform, t.style].join('|')
    const s = styles.get(k) ?? { t, count: 0, samples: new Set<string>(), stories: new Set<string>() }
    s.count++
    if (s.samples.size < 4) s.samples.add(t.text)
    s.stories.add(r.story)
    styles.set(k, s)
  }
}
const styleLines = [
  `# Text styles drawn in ${book} (light, text scale 1)`,
  '',
  'Every distinct combination of family, size, line height, weight, tracking, colour, case and slant. A style with no ramp name is off the type ramp; a colour with no token name is off the palette. Few styles, each with a clear role, is the goal.',
  '',
  '| ramp | family | size / line | weight | tracking | colour | case/slant | runs | samples |',
  '|---|---|---|---|---|---|---|---|---|'
]
for (const s of [...styles.values()].sort((a, b) => b.t.size - a.t.size || b.count - a.count)) {
  const t = s.t
  const family = t.family.split(',')[0].replace(/["']/g, '').trim()
  styleLines.push(
    `| ${t.ramp || '**off-ramp**'} | ${family} | ${t.size}/${t.lineHeight} | ${t.weight} | ${t.tracking} | ${t.colorToken || `**${t.color}**`} | ${[t.transform !== 'none' ? t.transform : '', t.style !== 'normal' ? t.style : ''].filter(Boolean).join(' ') || '-'} | ${s.count} | ${[...s.samples].map((x) => `“${x.replaceAll('|', '/')}”`).join(', ')} |`
  )
}
writeFileSync(join(out, 'styles.md'), styleLines.join('\n'))

const errors = renders.reduce((n, r) => n + r.findings.filter((f) => f.severity === 'error').length, 0)
console.log(`${stories.length} stories, ${renders.length} renders, ${errors} error findings${failed.length ? `, ${failed.length} failed renders` : ''}`)
console.log(`summary: ${relative(ROOT, join(out, 'summary.md'))}\nstyles:  ${relative(ROOT, join(out, 'styles.md'))}`)
