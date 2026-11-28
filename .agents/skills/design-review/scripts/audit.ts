/**
 * The in-page audit: run in a rendered story (`page.evaluate(audit, options)`), it walks every element, open shadow
 * roots included, and tests it against the `auto` rules of the ui-styleguide rulebook, as specified in
 * `.agents/skills/ui-styleguide/references/auto-checks.md`. Every finding's `check` is a rule ID (`TYPE-25`); its
 * detail is the measured value against the expected one. `../references/checks.md` says which rules are covered, in
 * which run, and what a reviewer must still look at.
 *
 * Modes (one function, so the colour and tree helpers are shared):
 * - `mark`: tags the elements whose accessible name is needed with `data-dr-n`, for capture.ts to read their names
 *   from Chromium's accessibility tree (CDP); the names come back in `names` for `check`.
 * - `check`: the static audit of one run (findings and the text inventory).
 * - `blank`: FEED-09's snapshot, 200 ms after the view is shown.
 * - `focus`: A11Y-11 for the element that has the focus now (capture.ts presses Tab between calls).
 * - `hover-list` / `hover-read`: the candidates of A11Y-18 and INPUT-02, and their state while hovered.
 * - `ctrl10` / `menu14`: CTRL-10 and MENU-14, which operate the page (run last).
 *
 * It is self-contained (Playwright serialises the function): no imports, no helpers outside it.
 */
export type Run = 'default' | 'dark' | 'large-text' | 'smallest' | 'reduced'

export interface AuditOptions {
  run: Run
  /** A story tagged `app`: a view in an AppWindow frame, so the window-level and composition rules apply. */
  app: boolean
  /** What the book is: an app's (`apps/*`), the kit's (`packages/kit`) or the shell's (`packages/ui`, `client`, …). */
  surface: 'app' | 'shell' | 'kit'
  mode?: 'mark' | 'check' | 'blank' | 'focus' | 'hover-list' | 'hover-read' | 'ctrl10' | 'menu14'
  /** Accessible names from the accessibility tree, by `data-dr-n`. */
  names?: Record<string, string>
  /** `hover-read`: the index of the candidate to read. */
  target?: string
  /** The story's play function operates the page (its layout shifts follow input): FEED-10 is not judged. */
  played?: boolean
}

export interface AuditFinding {
  /** The rule ID. */
  check: string
  severity: 'error' | 'warn' | 'info'
  /** Where: a path through hosts and shadow roots (`ui-section » ui-row » span.title`). */
  element: string
  /** The text it holds (or the control's name), cut to 80 characters. */
  text: string
  detail: string
  box: { x: number; y: number; w: number; h: number }
}

/** One run of text as drawn: what the style inventory (styles.md) and the Text Size comparison are built from. */
export interface TextSample {
  element: string
  text: string
  family: string
  size: number
  lineHeight: string
  weight: number
  tracking: number
  color: string
  colorToken: string
  transform: string
  style: string
  /** The ramp style the size matches at this scale, or '' when it matches none. */
  ramp: string
}

export interface HoverCandidate {
  /** Its index for `hover-read`. */
  n: string
  /** The `data-dr-h` of the element to hover: the candidate's parent (A11Y-18) or itself (INPUT-02). */
  hover: string
  /** The `data-dr-h` of the candidate, to focus it. */
  focus: string
  kind: 'A11Y-18' | 'INPUT-02'
  element: string
  text: string
}

export interface AuditResult {
  findings: AuditFinding[]
  texts: TextSample[]
  marked?: number
  /** `focus`: the focused element, whether its ring passes, and a key to see the walk come back to the start. */
  focus?: { key: string; element: string; text: string; inModal: boolean; detail: string; box: AuditFinding['box'] } | null
  hover?: HoverCandidate[]
  read?: { visible: boolean; scale: number }
  /** `blank`: whether the content root held nothing to look at. */
  blank?: boolean
}

export async function audit(o: AuditOptions): Promise<AuditResult> {
  const mode = o.mode ?? 'check'
  const run = o.run
  const findings: AuditFinding[] = []
  const texts: TextSample[] = []
  const W = window as unknown as {
    __designReview?: { dialogs: string[]; contextmenu: EventTarget[]; focusAtOpen: Element | null; blank: boolean | null; hover?: Element[] }
  }
  const dr = W.__designReview ?? { dialogs: [], contextmenu: [], focusAtOpen: null, blank: null }
  const root = document.documentElement
  /** s: the text scale. */
  const s = parseFloat(getComputedStyle(root).getPropertyValue('--text-scale')) || 1

  // ---- the ramp (auto-checks.md, Definitions) ------------------------------------------------------------------
  // the tracking is the page's own --tracking-* token, so the table follows tokens.css
  const tracking = (token: string): number => parseFloat(getComputedStyle(root).getPropertyValue(`--tracking-${token}`)) || 0
  const RAMP: [number, number, number, string][] = [
    [34, 41, tracking('large-title'), 'large-title'],
    [28, 34, tracking('title1'), 'title1'],
    [22, 28, tracking('title2'), 'title2'],
    [20, 25, tracking('title3'), 'title3'],
    [17, 22, tracking('body'), 'body/headline'],
    [16, 21, tracking('callout'), 'callout'],
    [15, 20, tracking('subheadline'), 'subheadline'],
    [13, 18, tracking('footnote'), 'footnote'],
    [12, 16, tracking('caption'), 'caption1'],
    [11, 13, tracking('caption'), 'caption2']
  ]
  const styleOf = (fontSize: number): [number, number, number, string] | undefined => RAMP.find(([z]) => Math.abs(z - fontSize / s) <= 0.25)

  // ---- which rules run in this run ------------------------------------------------------------------------------
  /** Rules whose result depends on the theme: they also run in the dark run. */
  const COLOUR = new Set(['COLOR-02', 'COLOR-03', 'COLOR-04', 'COLOR-05', 'COLOR-11', 'COLOR-12', 'COLOR-13', 'COLOR-14', 'COLOR-15', 'COLOR-16', 'COLOR-17', 'MOTION-03'])
  /** Rules about a view in its window: only for a story tagged `app` (a component showcase is not a view). */
  const VIEW = new Set(['LAYOUT-01', 'LAYOUT-12', 'LAYOUT-17', 'NAV-06', 'CTRL-03', 'CTRL-07', 'FEED-09', 'FEED-10', 'FEED-17', 'MODAL-01', 'MOTION-01', 'COLOR-10', 'FORM-02'])
  /** Copy and colour use are the product's in an app's book and in a view; a component library's showcase labels and swatches are the story's. */
  const productCopy = o.surface === 'app' || o.app
  function on(id: string): boolean {
    if (VIEW.has(id) && !o.app) return false
    if ((id.startsWith('WRITE-') || id === 'COLOR-06' || id === 'COLOR-08') && !productCopy) return false
    if (run === 'default') return id !== 'COLOR-11' && id !== 'A11Y-15' && id !== 'LAYOUT-12'
    if (run === 'dark') return COLOUR.has(id)
    if (run === 'large-text') return id === 'TYPE-26' || id === 'TYPE-27' || id === 'TYPE-28'
    if (run === 'smallest') return id === 'TYPE-27' || id === 'TYPE-28' || id === 'LAYOUT-12'
    return id === 'A11Y-15' || id === 'MOTION-03'
  }

  // ---- computed style, cached (the static pass changes nothing) ------------------------------------------------
  const csCache = new Map<Element, CSSStyleDeclaration>()
  function cs(el: Element): CSSStyleDeclaration {
    let c = csCache.get(el)
    if (!c) {
      c = getComputedStyle(el)
      csCache.set(el, c)
    }
    return c
  }
  const px = (v: string): number => {
    const n = parseFloat(v)
    return Number.isNaN(n) ? 0 : n
  }

  // ---- colour normalisation through a canvas ---------------------------------------------------------------------
  type RGBA = [number, number, number, number]
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = 1
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!
  const rgbaCache = new Map<string, RGBA>()
  function rgba(value: string): RGBA {
    const hit = rgbaCache.get(value)
    if (hit) return hit
    ctx.clearRect(0, 0, 1, 1)
    ctx.fillStyle = 'rgba(0,0,0,0)'
    ctx.fillStyle = value
    ctx.fillRect(0, 0, 1, 1)
    const d = ctx.getImageData(0, 0, 1, 1).data
    const out: RGBA = [d[0], d[1], d[2], d[3] / 255]
    rgbaCache.set(value, out)
    return out
  }
  /** Equal: alpha within 0.02 and every premultiplied channel within 2. */
  function same(a: RGBA, b: RGBA): boolean {
    if (Math.abs(a[3] - b[3]) > 0.02) return false
    for (let i = 0; i < 3; i++) if (Math.abs(a[i] * a[3] - b[i] * b[3]) > 2) return false
    return true
  }
  const hex = (c: RGBA): string => (c[3] >= 0.995 ? '#' + c.slice(0, 3).map((v) => Math.round(v).toString(16).padStart(2, '0')).join('') : `rgb(${Math.round(c[0])} ${Math.round(c[1])} ${Math.round(c[2])} / ${+c[3].toFixed(2)})`)

  // ---- token values per theme ---------------------------------------------------------------------------------
  const COLOR_TOKENS = [
    '--ios-blue', '--ios-green', '--ios-indigo', '--ios-orange', '--ios-pink', '--ios-purple', '--ios-red', '--ios-teal', '--ios-yellow', '--ios-mint', '--ios-cyan', '--ios-brown',
    '--ios-gray', '--ios-gray2', '--ios-gray3', '--ios-gray4', '--ios-gray5', '--ios-gray6', '--window-grip', '--focus-ring', '--tint', '--tint-fill',
    '--label', '--label-secondary', '--label-tertiary', '--label-quaternary', '--placeholder', '--link', '--separator', '--separator-opaque',
    '--fill', '--fill-secondary', '--fill-tertiary', '--fill-quaternary', '--bg', '--bg-secondary', '--bg-tertiary', '--bg-grouped', '--bg-grouped-secondary',
    '--bg-grouped-tertiary', '--bg-elevated', '--bg-grouped-elevated', '--bg-grouped-elevated-secondary', '--material-ultrathin', '--material-thin',
    '--material-regular', '--material-thick', '--material-chrome', '--dimming', '--switch-off', '--segment-thumb', '--knob'
  ]
  const SYSTEM = ['--ios-blue', '--ios-green', '--ios-indigo', '--ios-orange', '--ios-pink', '--ios-purple', '--ios-red', '--ios-teal', '--ios-yellow', '--ios-mint', '--ios-cyan', '--ios-brown', '--ios-gray', '--ios-gray2', '--ios-gray3', '--ios-gray4', '--ios-gray5', '--ios-gray6']
  const SHADOWS = ['--shadow-control', '--shadow-card', '--shadow-popover', '--shadow-window', '--shadow-slideover', '--shadow-window-inactive']
  type Theme = 'light' | 'dark'
  const tokens: Record<Theme, Map<string, RGBA>> = { light: new Map(), dark: new Map() }
  const shadowValues: string[] = []
  let materialBlur = 'none'
  const probeWrap = document.createElement('div')
  probeWrap.style.cssText = 'position:absolute;left:-9999px;top:0;width:1px;height:1px;overflow:hidden'
  document.body.appendChild(probeWrap)
  for (const theme of ['light', 'dark'] as const) {
    const wrap = document.createElement('div')
    wrap.setAttribute('data-theme', theme)
    const probe = document.createElement('div')
    wrap.appendChild(probe)
    probeWrap.appendChild(wrap)
    for (const name of COLOR_TOKENS) {
      probe.style.color = `var(${name})`
      tokens[theme].set(name, rgba(getComputedStyle(probe).color))
    }
    tokens[theme].set('transparent', [0, 0, 0, 0])
    tokens[theme].set('white', [255, 255, 255, 1])
    if (theme === 'light') {
      for (const name of SHADOWS) {
        probe.style.boxShadow = `var(${name})`
        shadowValues.push(getComputedStyle(probe).boxShadow)
      }
      probe.style.boxShadow = ''
      probe.style.backdropFilter = 'var(--material-blur)'
      materialBlur = getComputedStyle(probe).backdropFilter
    }
    wrap.remove()
  }
  // a subtree may re-tint (--tint): the tint and its fill at an element, resolved per value
  const tintCache = new Map<string, [RGBA, RGBA]>()
  function tintsAt(el: Element): [RGBA, RGBA] {
    const theme = themeOf(el)
    const spec = cs(el).getPropertyValue('--tint').trim()
    const k = theme + '|' + spec
    let hit = tintCache.get(k)
    if (!hit) {
      const wrap = document.createElement('div')
      wrap.setAttribute('data-theme', theme)
      if (spec) wrap.style.setProperty('--tint', spec)
      const probe = document.createElement('div')
      wrap.appendChild(probe)
      probeWrap.appendChild(wrap)
      probe.style.color = 'var(--tint)'
      const t = rgba(getComputedStyle(probe).color)
      probe.style.color = 'light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black))'
      const f = rgba(getComputedStyle(probe).color)
      wrap.remove()
      hit = [t, f]
      tintCache.set(k, hit)
    }
    return hit
  }
  const pageTheme: Theme = root.getAttribute('data-theme') === 'dark' ? 'dark' : root.getAttribute('data-theme') === 'light' ? 'light' : matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  function themeOf(el: Element): Theme {
    const scheme = cs(el).colorScheme
    if (scheme.includes('dark') && !scheme.includes('light')) return 'dark'
    if (scheme.includes('light') && !scheme.includes('dark')) return 'light'
    return pageTheme
  }
  /** The value of a token in the element's theme. */
  const tok = (el: Element, name: string): RGBA => tokens[themeOf(el)].get(name) ?? [0, 0, 0, 0]
  /** Whether a colour equals one of the named tokens at the element (--tint and --tint-fill as re-tinted there too). */
  function is(c: RGBA, el: Element, names: string[]): boolean {
    for (const n of names) {
      if (same(c, tok(el, n))) return true
      if (n === '--tint' && same(c, tintsAt(el)[0])) return true
      if (n === '--tint-fill' && same(c, tintsAt(el)[1])) return true
    }
    return false
  }
  /** Every token a colour equals at the element ('' when none). */
  function tokenNames(c: RGBA, el: Element): string[] {
    const out: string[] = []
    for (const [n, v] of tokens[themeOf(el)]) if (same(c, v)) out.push(n)
    if (!out.includes('--tint') && same(c, tintsAt(el)[0])) out.push('--tint')
    if (!out.includes('--tint-fill') && same(c, tintsAt(el)[1])) out.push('--tint-fill')
    return out
  }
  const LABELS = ['--label', '--label-secondary', '--label-tertiary', '--label-quaternary']
  const BG_FILL_SEP = COLOR_TOKENS.filter((n) => /^--(bg|fill|separator)/.test(n))
  const MATERIAL_OK = COLOR_TOKENS.filter((n) => /^--(material|fill|separator|dimming)/.test(n))
  const SEPARATORS = ['--separator', '--separator-opaque', '--ios-red', '--focus-ring']

  // ---- the flat tree ------------------------------------------------------------------------------------------
  function parentOf(el: Element): Element | null {
    if (el.assignedSlot) return el.assignedSlot
    if (el.parentElement) return el.parentElement
    const r = el.getRootNode()
    return r instanceof ShadowRoot ? r.host : null
  }
  /** el or a flat-tree ancestor matches the selector. */
  function inside(el: Element | null, sel: string): Element | null {
    for (let at = el; at; at = parentOf(at)) if (at.matches(sel)) return at
    return null
  }
  /** a contains b in the composed tree. */
  function contains(a: Element, b: Element): boolean {
    for (let at: Element | null = b; at; at = parentOf(at)) if (at === a) return true
    return false
  }
  const kitInternal = (el: Element): boolean => {
    const r = el.getRootNode()
    return r instanceof ShadowRoot && r.host.localName.startsWith('ui-')
  }
  function describe(el: Element): string {
    const parts: string[] = []
    let at: Element | null = el
    while (at && parts.length < 6) {
      const tag = at.localName
      const id = at.getAttribute('data-testid')
      const cls = typeof at.className === 'string' && at.className ? at.className.trim().split(/\s+/).filter((c) => !c.startsWith('svelte-') && c !== 's-' && !/^s-[\w-]{6,}$/.test(c)).slice(0, 2).join('.') : ''
      parts.unshift(id ? `${tag}[data-testid=${id}]` : `${tag}${cls ? '.' + cls : ''}`)
      if (id) break
      at = parentOf(at)
      if (at && (at.id === 'storybook-root' || at === document.body)) break
    }
    return parts.join(' » ')
  }
  function boxOf(el: Element | DOMRect): AuditFinding['box'] {
    const r = el instanceof Element ? el.getBoundingClientRect() : el
    return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) }
  }
  const clip80 = (t: string): string => {
    const c = t.replace(/\s+/g, ' ').trim()
    return c.length > 80 ? c.slice(0, 77) + '…' : c
  }
  const r1 = (n: number): number => Math.round(n * 100) / 100
  function add(check: string, severity: AuditFinding['severity'], el: Element, text: string, detail: string, box?: DOMRect): void {
    if (!on(check)) return
    findings.push({ check, severity, element: describe(el), text: clip80(text), detail, box: boxOf(box ?? el) })
  }

  // the bundled font loaded (capture waits for document.fonts.ready), or every text run is in a stand-in face
  if (run === 'default' && ![...document.fonts].some((f) => f.family.replace(/["']/g, '') === 'InterVariable' && f.status === 'loaded')) {
    add('TYPE-23', 'error', root, '', 'the bundled InterVariable font has not loaded: the system font stands in for it')
  }

  // ---- the elements -------------------------------------------------------------------------------------------
  const storyRoot = document.querySelector('#storybook-root') ?? document.body
  const appWindow = document.querySelector('[data-testid=app-window]')
  /** The window content root: the AppWindow's content box for a story tagged app, else the story's root. */
  const contentRoot: Element = (o.app && appWindow ? Array.from(appWindow.children).find((c) => c.classList.contains('content')) : null) ?? storyRoot
  const all: Element[] = []
  function collect(r: Document | ShadowRoot | Element): void {
    const walker = document.createTreeWalker(r, NodeFilter.SHOW_ELEMENT, {
      acceptNode: (n) => {
        const e = n as Element
        if (e === probeWrap) return NodeFilter.FILTER_REJECT
        if (e.parentElement === document.body && (e.id.startsWith('storybook-') ? e.id !== 'storybook-root' : /(^|\s)sb-/.test(e.className.toString()))) return NodeFilter.FILTER_REJECT
        if (e.localName === 'script' || e.localName === 'style' || e.localName === 'template') return NodeFilter.FILTER_REJECT
        return NodeFilter.FILTER_ACCEPT
      }
    })
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const el = n as Element
      all.push(el)
      if (el.shadowRoot) collect(el.shadowRoot)
    }
  }
  /** The story's own frame (the AppWindow and its wallpaper) is scaffolding, not the subject. */
  const scaffold = new Set<Element>()
  if (o.app && contentRoot !== storyRoot) for (let at: Element | null = contentRoot; at && at !== storyRoot; at = parentOf(at)) scaffold.add(at)
  scaffold.add(storyRoot)
  // a showcase story's own frame: a top-level wrapper it styles inline as a mock screen or desktop
  if (!o.app) for (const c of Array.from(storyRoot.children)) if (/background|border-radius/.test(c.getAttribute('style') ?? '')) scaffold.add(c)
  /** App element: not kit internal; in the kit's own book only its hosts (the story's scaffolding is not the subject). */
  function isA(el: Element): boolean {
    if (kitInternal(el) || scaffold.has(el)) return false
    if (o.surface === 'kit') return el.localName.startsWith('ui-')
    return true
  }

  // visible(): drawn, not hidden by opacity/display on the flat-tree chain, not a visually-hidden clip, not clipped away
  const chainMemo = new Map<Element, boolean>()
  function chainShown(el: Element | null): boolean {
    if (!el) return true
    const m = chainMemo.get(el)
    if (m !== undefined) return m
    const c = cs(el)
    const srOnly = ((c.position === 'absolute' || c.position === 'fixed') && /^rect\(0px,? 0px,? 0px,? 0px\)$/.test(c.clip)) || /inset\((50|100)%\)/.test(c.clipPath)
    const v = c.display !== 'none' && c.opacity !== '0' && !srOnly && chainShown(parentOf(el))
    chainMemo.set(el, v)
    return v
  }
  type Box = { l: number; t: number; r: number; b: number }
  const clipMemo = new Map<Element, Box>()
  const INF: Box = { l: -1e9, t: -1e9, r: 1e9, b: 1e9 }
  function topLayer(el: Element): boolean {
    try {
      return (el.localName === 'dialog' && el.matches(':modal')) || el.matches(':popover-open')
    } catch {
      return false
    }
  }
  /** The region an element can paint into, from its ancestors' `overflow: hidden|clip` (scrollers do not hide). */
  function clipOf(el: Element): Box {
    const m = clipMemo.get(el)
    if (m) return m
    let out = INF
    const c = cs(el)
    if (!(c.position === 'fixed' || topLayer(el))) {
      const p = parentOf(el)
      if (p) {
        out = clipOf(p)
        const pc = cs(p)
        // what a scroller holds can be scrolled into view: its ancestors' clips do not hide it on that axis
        if (/auto|scroll/.test(pc.overflowX)) out = { ...out, l: INF.l, r: INF.r }
        if (/auto|scroll/.test(pc.overflowY)) out = { ...out, t: INF.t, b: INF.b }
        const hx = pc.overflowX === 'hidden' || pc.overflowX === 'clip'
        const hy = pc.overflowY === 'hidden' || pc.overflowY === 'clip'
        if ((hx || hy) && pc.display !== 'contents') {
          const r = p.getBoundingClientRect()
          const pad = paddingBox(p, r)
          out = { l: hx ? Math.max(out.l, pad.l) : out.l, r: hx ? Math.min(out.r, pad.r) : out.r, t: hy ? Math.max(out.t, pad.t) : out.t, b: hy ? Math.min(out.b, pad.b) : out.b }
        }
      }
    }
    clipMemo.set(el, out)
    return out
  }
  /** Where an element can be seen now: its ancestors' clips, scrollers included (what is scrolled away is not there). */
  const viewMemo = new Map<Element, Box>()
  function viewClip(el: Element): Box {
    const m = viewMemo.get(el)
    if (m) return m
    let out = INF
    const c = cs(el)
    if (!(c.position === 'fixed' || topLayer(el))) {
      const p = parentOf(el)
      if (p) {
        out = viewClip(p)
        const pc = cs(p)
        const cx = pc.overflowX !== 'visible'
        const cy = pc.overflowY !== 'visible'
        if ((cx || cy) && pc.display !== 'contents') {
          const pad = paddingBox(p)
          out = { l: cx ? Math.max(out.l, pad.l) : out.l, r: cx ? Math.min(out.r, pad.r) : out.r, t: cy ? Math.max(out.t, pad.t) : out.t, b: cy ? Math.min(out.b, pad.b) : out.b }
        }
      }
    }
    viewMemo.set(el, out)
    return out
  }
  /** A rect cut to a box, or null when nothing of it is left. */
  function cutTo(r: DOMRect, k: Box): DOMRect | null {
    const l = Math.max(r.left, k.l)
    const t = Math.max(r.top, k.t)
    const rr = Math.min(r.right, k.r)
    const b = Math.min(r.bottom, k.b)
    return rr - l >= 1 && b - t >= 1 ? new DOMRect(l, t, rr - l, b - t) : null
  }
  function paddingBox(el: Element, r = el.getBoundingClientRect()): Box {
    const c = cs(el)
    return { l: r.left + px(c.borderLeftWidth), t: r.top + px(c.borderTopWidth), r: r.right - px(c.borderRightWidth), b: r.bottom - px(c.borderBottomWidth) }
  }
  const visMemo = new Map<Element, boolean>()
  function visible(el: Element): boolean {
    const m = visMemo.get(el)
    if (m !== undefined) return m
    let v = false
    if (el.getClientRects().length > 0 && cs(el).visibility === 'visible' && chainShown(el)) {
      const r = el.getBoundingClientRect()
      if (!(r.width <= 1 && r.height <= 1) && r.width * r.height > 0) {
        const k = clipOf(el)
        v = r.right > k.l + 0.5 && r.left < k.r - 0.5 && r.bottom > k.t + 0.5 && r.top < k.b - 0.5
      } else if (el.localName === 'svg' || el instanceof SVGElement) v = false
    }
    visMemo.set(el, v)
    return v
  }

  // ---- disabled, interactive, kit hosts ------------------------------------------------------------------------
  const K = 'ui-button, ui-toggle, ui-check, ui-picker, ui-text-field, ui-text-area, ui-row[tappable]'
  const I_SEL =
    'a[href], button, input:not([type=hidden]), select, textarea, summary, [role=button], [role=link], [role=checkbox], [role=switch], [role=radio], [role=tab], [role=menuitem], [role=menuitemcheckbox], [role=menuitemradio], [role=option], [role=slider], [role=spinbutton], [role=combobox], [role=treeitem], [contenteditable=""], [contenteditable=true], [tabindex]:not([tabindex="-1"])'
  const isK = (el: Element): boolean => el.matches(K)
  const hostOf = (el: Element): Element | null => {
    const r = el.getRootNode()
    return r instanceof ShadowRoot ? r.host : null
  }
  function innerTarget(host: Element): Element | null {
    return host.shadowRoot?.querySelector('button, input, textarea, select, [role=switch]') ?? null
  }
  function disabledSelf(el: Element): boolean {
    if (el.getAttribute('aria-disabled') === 'true') return true
    try {
      if (el.matches(':disabled')) return true
    } catch {
      /* not a form element */
    }
    return isK(el) && (el.hasAttribute('disabled') || (innerTarget(el)?.matches(':disabled') ?? false))
  }
  /** Inside a disabled control (WCAG exempts inactive components), or inert. */
  function disabledChain(el: Element): boolean {
    for (let at: Element | null = el; at; at = parentOf(at)) {
      if (at.hasAttribute('inert') || at.getAttribute('aria-disabled') === 'true' || (at.hasAttribute('disabled') && (at.localName.startsWith('ui-') || /^(button|input|select|textarea|fieldset|option|optgroup)$/.test(at.localName)))) return true
      try {
        if (at.matches(':disabled')) return true
      } catch {
        /* not a form element */
      }
    }
    return false
  }
  /** Matches the interactive selector, enabled or not (for "inside an interactive element"). */
  const interactiveish = (el: Element): boolean => el.matches(I_SEL) || isK(el)
  const insideInteractive = (el: Element): boolean => {
    for (let at: Element | null = el; at; at = parentOf(at)) if (interactiveish(at)) return true
    return false
  }
  function hitBox(el: Element): DOMRect {
    const r = el.getBoundingClientRect()
    const host = hostOf(el)
    if (host?.localName === 'ui-segmented' && el.getAttribute('role') === 'radio') {
      const h = host.getBoundingClientRect()
      return new DOMRect(r.left, h.top, r.width, h.height)
    }
    return r
  }

  // ---- roles and names ----------------------------------------------------------------------------------------
  function roleOf(el: Element): string {
    const explicit = (el.getAttribute('role') ?? '').trim().split(/\s+/)[0]
    if (explicit) return explicit
    if (isK(el)) {
      const t = innerTarget(el)
      return t ? roleOf(t) : ''
    }
    const tag = el.localName
    if (tag === 'button' || tag === 'summary') return 'button'
    if (tag === 'a') return el.hasAttribute('href') ? 'link' : ''
    if (tag === 'select') return 'combobox'
    if (tag === 'textarea') return 'textbox'
    if (tag === 'input') {
      const t = (el as HTMLInputElement).type
      return t === 'checkbox' ? 'checkbox' : t === 'radio' ? 'radio' : t === 'range' ? 'slider' : t === 'number' ? 'spinbutton' : ['button', 'submit', 'reset', 'image'].includes(t) ? 'button' : 'textbox'
    }
    if (/^h[1-6]$/.test(tag)) return 'heading'
    if (tag === 'img') return 'img'
    if (tag === 'dialog') return 'dialog'
    return ''
  }
  const collapse = (t: string): string => t.replace(/\s+/g, ' ').trim()
  /** The text a person sees in a subtree, through slots and shadow roots. */
  function rendered(node: Node): string {
    if (node.nodeType === Node.TEXT_NODE) {
      const slot = (node as Text).assignedSlot
      const p = node.parentElement
      if (!slot && p?.shadowRoot) return '' // not drawn: its host shows no slot for it
      return slot || (p && visible(p)) ? node.textContent ?? '' : ''
    }
    if (node instanceof HTMLSlotElement) {
      const assigned = node.assignedNodes({ flatten: true })
      return (assigned.length ? assigned : Array.from(node.childNodes)).map(rendered).join(' ')
    }
    if (node instanceof Element) {
      if (/^(script|style|template)$/.test(node.localName)) return ''
      if (cs(node).display === 'none') return ''
      if (node.shadowRoot) return Array.from(node.shadowRoot.childNodes).map(rendered).join(' ')
      if (node instanceof HTMLInputElement || node instanceof HTMLTextAreaElement) return ''
    }
    return Array.from(node.childNodes).map(rendered).join(' ')
  }
  /** Visible text: for a kit host, its default slot's assigned nodes (a row: its title); else what it shows. */
  function visibleText(el: Element): string {
    if (el.localName === 'ui-row') {
      const slot = el.shadowRoot?.querySelector('slot:not([name])') as HTMLSlotElement | null
      const assigned = slot?.assignedNodes({ flatten: true }) ?? []
      if (assigned.length) return collapse(assigned.map(rendered).join(' '))
      const t = el.shadowRoot?.querySelector('.title')
      return collapse(t ? t.textContent ?? '' : '')
    }
    if (isK(el)) {
      const slot = el.shadowRoot?.querySelector('slot:not([name])') as HTMLSlotElement | null
      return collapse((slot?.assignedNodes({ flatten: true }) ?? []).map(rendered).join(' '))
    }
    return collapse(rendered(el))
  }
  function fallbackName(el: Element): string {
    const label = el.getAttribute('aria-label')
    if (label && label.trim()) return label
    const by = el.getAttribute('aria-labelledby')
    if (by) {
      const r = el.getRootNode() as Document | ShadowRoot
      const t = by.split(/\s+/).map((id) => r.getElementById?.(id)?.textContent ?? '').join(' ')
      if (t.trim()) return t
    }
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
      if (el.labels && el.labels.length) return el.labels[0].textContent ?? ''
      return el.getAttribute('title') ?? el.getAttribute('placeholder') ?? ''
    }
    if (el.localName === 'img') return el.getAttribute('alt') ?? ''
    if (el.localName === 'svg') return el.querySelector('title')?.textContent ?? ''
    return rendered(el) || el.getAttribute('title') || ''
  }
  /** The accessible name (Chromium's, from the accessibility tree via capture.ts); a kit host's is its inner target's. */
  function nameOf(el: Element): string {
    const t = isK(el) ? innerTarget(el) ?? el : el
    const n = t.getAttribute('data-dr-n')
    if (n && o.names && n in o.names) return collapse(o.names[n])
    return collapse(fallbackName(t))
  }
  /** Icon-only: no visible text, and a visible ui-icon or svg among its descendants (a kit host: its slotted ones). */
  function iconOnly(el: Element): boolean {
    if (visibleText(el)) return false
    const scope: Element[] = isK(el) ? Array.from(el.children) : [el]
    for (const s0 of scope) {
      const list = s0 === el ? descendants(el) : [s0, ...descendants(s0)]
      if (list.some((d) => (d.localName === 'ui-icon' || d.localName === 'svg') && visible(d))) return true // ui-icon, the shell's Icon (svg.ios-icon), an svg
    }
    return false
  }
  /** Composed descendants (shadow roots and slotted content once, in light-DOM position). */
  function descendants(el: Element): Element[] {
    const out: Element[] = []
    const walk = (r: Element | ShadowRoot): void => {
      for (const c of Array.from(r.children)) {
        out.push(c)
        if (c.shadowRoot) walk(c.shadowRoot)
        walk(c)
      }
    }
    if (el.shadowRoot) walk(el.shadowRoot)
    walk(el)
    return out
  }
  // ---- focus (A11Y-11, per Tab stop) ---------------------------------------------------------------------------
  if (mode === 'focus') {
    const f = deepActive()
    if (!f) {
      probeWrap.remove()
      return { findings, texts, focus: null }
    }
    const host = hostOf(f)
    const H = host && isK(host) ? host : null
    const row = inside(f, 'ui-row')
    const face = row?.shadowRoot?.querySelector('.face') ?? null
    // where a component draws its ring: the element, its kit host, its row's face, or a part of it (an app icon's plate)
    // or a card around it (a notification's) a few levels out
    const self = H ?? f
    const around: Element[] = []
    for (let at = parentOf(self), k = 0; at && k < 3; at = parentOf(at), k++) around.push(at)
    const cands = [f, H, face, ...descendants(self).filter((d) => visible(d)).slice(0, 40), ...around].filter((x): x is Element => !!x)
    /** A slider's ring on its thumb: a pseudo-element getComputedStyle cannot read, so from the style rules that match. */
    const thumbOutline = (el: Element): string => {
      const sheets: CSSStyleSheet[] = [...Array.from(document.styleSheets)]
      const r0 = el.getRootNode()
      if (r0 instanceof ShadowRoot) sheets.push(...Array.from(r0.styleSheets), ...r0.adoptedStyleSheets)
      const walk = (rules: CSSRuleList): string => {
        for (const rule of Array.from(rules)) {
          if (rule instanceof CSSStyleRule && rule.selectorText.includes('::-webkit-slider-thumb')) {
            for (const part of rule.selectorText.split(/,(?![^(]*\))/)) {
              if (!part.includes('::-webkit-slider-thumb')) continue
              try {
                if (el.matches(part.replace('::-webkit-slider-thumb', '').trim() || '*') && rule.style.getPropertyValue('outline')) return rule.style.getPropertyValue('outline')
              } catch {
                /* a selector this element cannot be matched with */
              }
            }
          } else if ('cssRules' in rule && (rule as CSSGroupingRule).cssRules) {
            const hit = walk((rule as CSSGroupingRule).cssRules)
            if (hit) return hit
          }
        }
        return ''
      }
      for (const sh of sheets) {
        try {
          const hit = walk(sh.cssRules)
          if (hit) return hit
        } catch {
          /* a sheet from another origin */
        }
      }
      return ''
    }
    let ok = false
    let cut = ''
    let measured = ''
    for (const e of cands) {
      // a slider draws its ring on its thumb; a component may draw it on ::after or ::before (the window's resize arc)
      let c = getComputedStyle(e)
      for (const pseudo of ['::after', '::before']) {
        const pc = getComputedStyle(e, pseudo)
        if (c.outlineStyle === 'none' && pc.content !== 'none' && pc.outlineStyle !== 'none') c = pc
      }
      if (e instanceof HTMLInputElement && e.type === 'range' && c.outlineStyle === 'none') {
        const ring = thumbOutline(e)
        if (ring) {
          const probe = document.createElement('div')
          probe.style.outline = ring
          probeWrap.appendChild(probe)
          c = getComputedStyle(probe)
        }
      }
      if (c.outlineStyle === 'none' || px(c.outlineWidth) < 2) continue
      const col = rgba(c.outlineColor)
      measured = `${c.outlineWidth} ${c.outlineStyle} ${hex(col)}`
      if (!is(col, e, ['--focus-ring', '--label'])) continue
      let cutHere = ''
      const r = e.getBoundingClientRect()
      const g = px(c.outlineOffset) + px(c.outlineWidth)
      const ring: Box = { l: r.left - g, t: r.top - g, r: r.right + g, b: r.bottom + g }
      for (let at = parentOf(e); at; at = parentOf(at)) {
        const ac = getComputedStyle(at)
        if (ac.overflowX === 'visible' && ac.overflowY === 'visible') continue
        if (ac.display === 'contents') continue
        const p = paddingBox(at)
        const over2 = Math.max(p.l - ring.l, ring.r - p.r, p.t - ring.t, ring.b - p.b)
        if (over2 > 1) {
          cutHere = `ring cut by ${describe(at)} (${r1(over2)}px)`
          break
        }
      }
      // one whole ring (on the element, its kit host or its row's face) is enough
      if (!cutHere) {
        ok = true
        break
      }
      if (!cut) cut = cutHere
    }
    probeWrap.remove()
    const detail = ok ? '' : cut || (measured ? `focus ring ${measured}; expected ≥2px in --focus-ring or --label` : 'no focus ring (outline none or under 2px) on the element, its kit host or its row face')
    return { findings, texts, focus: { key: describe(f) + '#' + (f.getAttribute('data-dr-n') ?? ''), element: describe(H ?? f), text: clip80(nameOf(H ?? f)), inModal: !!inside(f, 'dialog, [role=dialog], [role=alertdialog]'), detail, box: boxOf(H ?? f) } }
  }

  // ---- hover-read (A11Y-18, INPUT-02): the state of one candidate while hovered ---------------------------------
  if (mode === 'hover-read') {
    probeWrap.remove()
    const el = W.__designReview?.hover?.[Number(o.target)]
    if (!el) return { findings, texts, read: { visible: false, scale: 1 } }
    for (const a of el.getAnimations()) {
      try {
        a.finish()
      } catch {
        /* infinite */
      }
    }
    const c = getComputedStyle(el)
    let opa = 1
    for (let at: Element | null = el; at; at = parentOf(at)) opa *= Number(getComputedStyle(at).opacity)
    const m = c.transform === 'none' ? new DOMMatrix() : new DOMMatrix(c.transform)
    const sc = Math.max(Math.hypot(m.a, m.b), Math.hypot(m.c, m.d)) * (c.scale && c.scale !== 'none' ? Math.max(...c.scale.split(/\s+/).map(Number)) : 1)
    return { findings, texts, read: { visible: c.visibility === 'visible' && opa > 0.05, scale: sc } }
  }

  // ---- the walk -----------------------------------------------------------------------------------------------
  collect(document.body)
  const shadowRoots: ShadowRoot[] = all.filter((e) => e.shadowRoot).map((e) => e.shadowRoot!)

  // a modal dialog makes the rest of the page inert
  const modalOpen = all.filter((e) => visible(e) && (topLayer(e) && e.localName === 'dialog' ? e.matches(':modal') : e.matches('[role=dialog][aria-modal=true], [role=alertdialog][aria-modal=true]')))
  const topModal = modalOpen.length ? modalOpen[modalOpen.length - 1] : null
  const blockedByModal = (el: Element): boolean => !!topModal && !contains(topModal, el)
  /** I: the interactive elements, one target per control (a kit control's internals are its host). */
  const I: Element[] = []
  {
    const seen = new Set<Element>()
    for (const el of all) {
      if (!el.matches(I_SEL) && !isK(el)) continue
      let t = el
      const host = hostOf(el)
      if (host && isK(host) && kitInternal(el)) t = host
      if (seen.has(t)) continue
      seen.add(t)
      if (!visible(t) || disabledSelf(t) || disabledChain(t) || blockedByModal(t)) continue
      I.push(t)
    }
  }
  const Iset = new Set(I)
  /** Every element that is or holds an interactive element. */
  const holdsI = new Set<Element>()
  for (const i of I) for (let at: Element | null = i; at && !holdsI.has(at); at = parentOf(at)) holdsI.add(at)
  const ID = new Map<Element, number>()
  all.forEach((e, i) => ID.set(e, i))

  // ---- mark: the elements whose accessible name is needed ------------------------------------------------------
  if (mode === 'mark') {
    let n = 0
    for (const el of all) el.removeAttribute('data-dr-n')
    for (const el of all) {
      const need = el.matches(I_SEL) || el.matches('a, dialog, [role=dialog], [role=alertdialog], svg, img, h1, h2, h3, h4, h5, h6, [role=heading], [role=menu], input, textarea, select') || (hostOf(el) && isK(hostOf(el)!) && innerTarget(hostOf(el)!) === el)
      if (need && el.getClientRects().length) el.setAttribute('data-dr-n', String(n++))
    }
    probeWrap.remove()
    return { findings, texts, marked: n }
  }

  // ---- surfaces ---------------------------------------------------------------------------------------------
  const BAR_SEL = 'header, [role=toolbar], .ios-navbar .bar'
  const bars: Element[] = []
  {
    const cands = all.filter((e) => e.matches(BAR_SEL) && visible(e) && (descendants(e).some((d) => Iset.has(d) || /^h[12]$/.test(d.localName)) || I.some((i) => contains(e, i))))
    for (const c of cands) if (!cands.some((d) => d !== c && contains(d, c))) bars.push(c)
  }
  const barItems = (bar: Element): Element[] => I.filter((i) => contains(bar, i))
  const alerts = all.filter((e) => e.getAttribute('role') === 'alertdialog' && visible(e))
  const modals = modalOpen
  const isAlert = (e: Element): boolean => e.getAttribute('role') === 'alertdialog'
  const sheets = modals.filter((m) => !isAlert(m))
  const popovers = all.filter((e) => {
    if (!visible(e) || e.classList.contains('ios-window')) return false // a window is not a popover
    try {
      if (e.matches(':popover-open')) return true
    } catch {
      /* no popover support */
    }
    return e.getAttribute('role') === 'dialog' && !e.hasAttribute('aria-modal') && !(e.localName === 'dialog' && e.matches(':modal')) && (cs(e).position === 'absolute' || cs(e).position === 'fixed')
  })
  const menus = all.filter((e) => e.getAttribute('role') === 'menu' && visible(e))
  /** A context menu: the shell's ContextMenu (a menu in its press-catching layer), or a menu named for one. */
  const isContextMenu = (m: Element): boolean => (m.parentElement?.classList.contains('layer') && m.classList.contains('menu')) || /context/i.test(m.getAttribute('data-testid') ?? '')
  const menuItems = (m: Element): Element[] => descendants(m).filter((d) => /^menuitem/.test(d.getAttribute('role') ?? '') && visible(d))
  /** A layer: what is drawn above the normal flow (overlap is only judged inside one layer). */
  function layerOf(el: Element): Element | null {
    for (let at: Element | null = el; at; at = parentOf(at)) {
      if (topLayer(at)) return at
      const role = at.getAttribute('role')
      if (role === 'menu' || role === 'dialog' || role === 'alertdialog' || role === 'tooltip') return at
      const c = cs(at)
      if (c.position === 'fixed' || c.position === 'sticky' || (c.position === 'absolute' && c.zIndex !== 'auto')) return at
    }
    return null
  }
  function deepActive(): Element | null {
    let at = document.activeElement
    while (at?.shadowRoot?.activeElement) at = at.shadowRoot.activeElement
    return at === document.body ? null : at
  }

  // ---- text elements (T) --------------------------------------------------------------------------------------
  interface Txt {
    el: Element
    /** Whose computed style draws the text: the element, or for slotted text its slot. */
    st: Element
    c: CSSStyleDeclaration
    nodes: Text[]
    rects: DOMRect[]
    text: string
    lines: number
  }
  const T: Txt[] = []
  const tOf = new Map<Element, Txt>()
  for (const el of all) {
    const nodes: Text[] = []
    let st: Element = el
    for (const child of Array.from(el.childNodes)) {
      if (child.nodeType !== Node.TEXT_NODE || !(child.textContent ?? '').trim()) continue
      if (el.shadowRoot) {
        const slot = (child as Text).assignedSlot
        if (!slot) continue // not drawn: the host shows no slot for it
        st = slot
      }
      nodes.push(child as Text)
    }
    if (!nodes.length || !visible(el)) continue
    if (st !== el && !chainShown(st)) continue
    const rects: DOMRect[] = []
    const range = document.createRange()
    for (const n of nodes) {
      range.selectNodeContents(n)
      for (const r of Array.from(range.getClientRects())) if (r.width > 0.5 && r.height > 0.5) rects.push(r)
    }
    if (!rects.length) continue
    const tops = new Set(rects.map((r) => Math.round(r.top / 4)))
    const t: Txt = { el, st, c: cs(st), nodes, rects, text: collapse(nodes.map((n) => n.textContent ?? '').join(' ')), lines: tops.size }
    T.push(t)
    tOf.set(el, t)
  }
  /** The parts of a text a person sees: cut by its own box when it clips (an ellipsis) and by every clipping ancestor. */
  const seenMemo = new Map<Txt, DOMRect[]>()
  function seen(t: Txt): DOMRect[] {
    let out = seenMemo.get(t)
    if (out) return out
    let k = viewClip(t.st)
    if (t.st === t.el) {
      const c = cs(t.el)
      if (c.overflowX !== 'visible' || c.overflowY !== 'visible') {
        const pad = paddingBox(t.el)
        k = { l: c.overflowX !== 'visible' ? Math.max(k.l, pad.l) : k.l, r: c.overflowX !== 'visible' ? Math.min(k.r, pad.r) : k.r, t: c.overflowY !== 'visible' ? Math.max(k.t, pad.t) : k.t, b: c.overflowY !== 'visible' ? Math.min(k.b, pad.b) : k.b }
      }
    }
    out = t.rects.map((r) => cutTo(r, k)).filter((r): r is DOMRect => !!r)
    seenMemo.set(t, out)
    return out
  }
  /** The part of a hit box that can be reached now. */
  const seenHit = (el: Element): DOMRect | null => cutTo(hitBox(el), viewClip(el))
  /** The same parts, scrolled into view: cut by clipping ancestors and its own box, not by scrollers. */
  const localMemo = new Map<Txt, DOMRect[]>()
  function local(t: Txt): DOMRect[] {
    let out = localMemo.get(t)
    if (out) return out
    let k = clipOf(t.st)
    if (t.st === t.el) {
      const c = cs(t.el)
      if (c.overflowX !== 'visible' || c.overflowY !== 'visible') {
        const pad = paddingBox(t.el)
        k = { l: c.overflowX !== 'visible' ? Math.max(k.l, pad.l) : k.l, r: c.overflowX !== 'visible' ? Math.min(k.r, pad.r) : k.r, t: c.overflowY !== 'visible' ? Math.max(k.t, pad.t) : k.t, b: c.overflowY !== 'visible' ? Math.min(k.b, pad.b) : k.b }
      }
    }
    out = t.rects.map((r) => cutTo(r, k)).filter((r): r is DOMRect => !!r)
    localMemo.set(t, out)
    return out
  }
  const localHit = (el: Element): DOMRect | null => cutTo(hitBox(el), clipOf(el))
  /** The nearest scroll container: two things in one scroller move together; across scrollers only what is in view meets. */
  const scrollMemo = new Map<Element, Element | null>()
  function scrollerOf(el: Element): Element | null {
    if (scrollMemo.has(el)) return scrollMemo.get(el)!
    let out: Element | null = null
    for (let at = parentOf(el); at; at = parentOf(at)) {
      const c = cs(at)
      if (/auto|scroll/.test(c.overflowX + ' ' + c.overflowY)) {
        out = at
        break
      }
    }
    scrollMemo.set(el, out)
    return out
  }
  const isLarge = (t: Txt): boolean => px(t.c.fontSize) >= 24 || (px(t.c.fontSize) >= 18.66 && Number(t.c.fontWeight) >= 700)
  const firstFamily = (c: CSSStyleDeclaration): string => c.fontFamily.split(',')[0].trim().replace(/["']/g, '')
  const NOT_INTERFACE = 'input, textarea, select, [contenteditable]:not([contenteditable=false]), code, pre, kbd, samp, [role=log], [role=textbox], canvas, .xterm'
  const interfaceText = T.filter((t) => isA(t.el) && !inside(t.el, NOT_INTERFACE))

  // ---- composited background ----------------------------------------------------------------------------------
  /**
   * What is behind an element: the flat-tree ancestors' backgrounds, composited until one is opaque. A gradient gives
   * one candidate per colour stop; an image, a backdrop filter (a material over moving content) or no opaque layer at all
   * makes it unknown.
   */
  function backdrop(el: Element): { color: RGBA; known: boolean; cands: RGBA[] } {
    const layers: RGBA[][] = []
    let known = true
    let opaque = false
    for (let at: Element | null = el; at; at = parentOf(at)) {
      const c = cs(at)
      if (c.backdropFilter && c.backdropFilter !== 'none') known = false
      if (c.backgroundImage !== 'none') {
        const stops = /url\(|image-set|element\(|cross-fade/.test(c.backgroundImage) ? [] : [...c.backgroundImage.matchAll(/(rgba?|color|oklab|oklch|lab|lch|hsla?|hwb)\([^()]*\)|#[0-9a-f]{3,8}\b|\btransparent\b/gi)].map((m) => rgba(m[0]))
        if (stops.length) layers.push(stops)
        else known = false
        if (stops.length && stops.every((x) => x[3] >= 0.999)) {
          opaque = true
          break
        }
      }
      const b = rgba(c.backgroundColor)
      if (b[3] > 0) layers.push([b])
      if (b[3] >= 0.999) {
        opaque = true
        break
      }
    }
    if (!opaque) known = false
    let cands: RGBA[] = [themeOf(el) === 'dark' ? [0, 0, 0, 1] : [255, 255, 255, 1]]
    for (const layer of layers.reverse()) {
      const next: RGBA[] = []
      for (const base of cands) for (const b of layer) next.push([b[0] * b[3] + base[0] * (1 - b[3]), b[1] * b[3] + base[1] * (1 - b[3]), b[2] * b[3] + base[2] * (1 - b[3]), 1])
      cands = next.slice(0, 16)
    }
    return { color: cands[0], known, cands }
  }
  /** Contrast of a colour over what is behind an element: pass, fail (known) or review (unknown, or a gradient that is both). */
  function judge(fg: RGBA, el: Element, need: number): { verdict: 'pass' | 'fail' | 'review'; ratio: number; bg: RGBA } {
    const bd = backdrop(el)
    const ratios = bd.cands.map((c) => contrast(fg, c))
    const lo = ratios.indexOf(Math.min(...ratios))
    const hi = ratios.indexOf(Math.max(...ratios))
    if (ratios[lo] >= need - 0.005) return { verdict: 'pass', ratio: ratios[lo], bg: bd.cands[lo] }
    if (bd.known && ratios[hi] < need - 0.005) return { verdict: 'fail', ratio: ratios[hi], bg: bd.cands[hi] }
    return { verdict: 'review', ratio: ratios[lo], bg: bd.cands[lo] }
  }
  /**
   * The shell is a design system as the kit is: it may derive a colour from a token (COLOR-01: "a color-mix() of tokens
   * done by the kit"), a token mixed toward black or white or at a lower alpha. In an app that is no token value.
   */
  function oklab(c: RGBA): [number, number, number] {
    const lin = (v: number): number => {
      const x = v / 255
      return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4
    }
    const [r, g, b] = [lin(c[0]), lin(c[1]), lin(c[2])]
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
    const q = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
    return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * q, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * q, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * q]
  }
  function derived(c: RGBA, el: Element): boolean {
    if (o.surface !== 'shell') return false
    const pal = [...tokens[themeOf(el)].values(), ...tintsAt(el)].filter((v) => v[3] >= 0.999)
    if (c[3] < 0.995) {
      const tol = c[3] < 0.3 ? 6 : 3
      return pal.some((v) => Math.abs(v[0] - c[0]) <= tol && Math.abs(v[1] - c[1]) <= tol && Math.abs(v[2] - c[2]) <= tol)
    }
    const C = oklab(c)
    for (const v of pal) {
      const P = oklab(v)
      for (const E of [[0, 0, 0], [1, 0, 0]]) {
        const d = [E[0] - P[0], E[1] - P[1], E[2] - P[2]]
        const dd = d[0] ** 2 + d[1] ** 2 + d[2] ** 2
        if (dd < 1e-6) continue
        const w = [C[0] - P[0], C[1] - P[1], C[2] - P[2]]
        const t = (w[0] * d[0] + w[1] * d[1] + w[2] * d[2]) / dd
        if (t <= 0.01 || t >= 0.99) continue
        if (Math.hypot(w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]) < 0.008) return true
      }
    }
    return false
  }
  /** The first ancestor (not el itself) with an opaque background. */
  function opaqueAncestor(el: Element): RGBA | null {
    for (let at = parentOf(el); at; at = parentOf(at)) {
      const b = rgba(cs(at).backgroundColor)
      if (b[3] >= 0.999) return b
    }
    return null
  }
  function lum(c: RGBA): number {
    const f = (v: number): number => {
      const x = v / 255
      return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4
    }
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
  }
  const over = (fg: RGBA, bg: RGBA): RGBA => [fg[0] * fg[3] + bg[0] * (1 - fg[3]), fg[1] * fg[3] + bg[1] * (1 - fg[3]), fg[2] * fg[3] + bg[2] * (1 - fg[3]), 1]
  function contrast(fg: RGBA, bg: RGBA): number {
    const a = lum(over(fg, bg))
    const b = lum(bg)
    return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)
  }
  function opacityChain(el: Element): number {
    let o2 = 1
    for (let at: Element | null = el; at; at = parentOf(at)) o2 *= Number(cs(at).opacity)
    return o2
  }

  // ---- blank (FEED-09 at 200 ms) -------------------------------------------------------------------------------
  if (mode === 'blank') {
    probeWrap.remove()
    const blank = !T.some((t) => contains(contentRoot, t.el)) && !all.some((e) => (e.localName === 'ui-spinner' || e.localName === 'ui-empty-state') && contains(contentRoot, e) && visible(e))
    return { findings, texts, blank }
  }

  // ---- hover (A11Y-18, INPUT-02) -------------------------------------------------------------------------------
  if (mode === 'hover-list') {
    probeWrap.remove()
    const out: HoverCandidate[] = []
    const list: Element[] = []
    let next = 0
    const mark = (e: Element): string => {
      let n = e.getAttribute('data-dr-h')
      if (!n) {
        n = String(next++)
        e.setAttribute('data-dr-h', n)
      }
      return n
    }
    const push = (el: Element, hover: Element, kind: HoverCandidate['kind']): void => {
      list.push(el)
      out.push({ n: String(list.length - 1), hover: mark(hover), focus: mark(el), kind, element: describe(el), text: clip80(nameOf(el)) })
    }
    // A11Y-18: an interactive element hidden at rest (opacity 0 or visibility hidden), still laid out
    for (const el of all) {
      if (!(el.matches(I_SEL) || isK(el)) || (kitInternal(el) && isK(hostOf(el)!))) continue
      if (!el.getClientRects().length || disabledSelf(el) || disabledChain(el)) continue
      if ((el as HTMLElement).tabIndex < 0) continue // not reachable by focus anyway: not a hover-only control
      let hidden = cs(el).visibility === 'hidden'
      for (let at: Element | null = el; at && !hidden; at = parentOf(at)) if (cs(at).opacity === '0') hidden = true
      if (!hidden || !chainDisplayed(el)) continue
      const p = parentOf(el)
      if (p) push(el, p, 'A11Y-18')
    }
    // INPUT-02: interactive elements in a list or a bar
    let count = 0
    for (const el of I) {
      if (count >= 40) break
      if (!(inside(el, 'ui-list, [role=list], [role=listbox], ul, ol, [role=grid], [role=tree]') || bars.some((b) => contains(b, el)))) continue
      push(el, el, 'INPUT-02')
      count++
    }
    if (W.__designReview) W.__designReview.hover = list
    return { findings, texts, hover: out }
  }
  function chainDisplayed(el: Element): boolean {
    for (let at: Element | null = el; at; at = parentOf(at)) if (cs(at).display === 'none') return false
    return true
  }
  // ---- CTRL-10: a toggle button changes its fill --------------------------------------------------------------------
  if (mode === 'ctrl10') {
    probeWrap.remove()
    const frame = (): Promise<void> => new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(() => res())))
    for (const el of all.filter((e) => e.hasAttribute('aria-pressed') && visible(e) && !disabledChain(e))) {
      const host = hostOf(el)
      const before = el.getAttribute('aria-pressed')
      const fill = (): RGBA => {
        for (const a of el.getAnimations()) {
          try {
            a.finish()
          } catch {
            /* infinite */
          }
        }
        return rgba(getComputedStyle(el).backgroundColor)
      }
      const a = fill()
      ;(el as HTMLElement).click()
      await frame()
      const flipped = el.getAttribute('aria-pressed') !== before
      // a click the story does not answer leaves both states unknown: nothing to compare
      if (!flipped) continue
      const b = fill()
      const label = nameOf(host && isK(host) ? host : el)
      if (same(a, b)) findings.push({ check: 'CTRL-10', severity: 'error', element: describe(host && isK(host) ? host : el), text: clip80(label), detail: `background ${hex(a)} with aria-pressed ${before} and ${hex(b)} with ${el.getAttribute('aria-pressed')}: the fill does not change`, box: boxOf(el) })
      // restore
      if (el.getAttribute('aria-pressed') !== before) {
        ;(el as HTMLElement).click()
        await frame()
      }
    }
    return { findings, texts }
  }

  // ---- MENU-14: text keeps its edit menu ---------------------------------------------------------------------------
  if (mode === 'menu14') {
    probeWrap.remove()
    const fire = (el: Element): boolean => {
      const r = el.getBoundingClientRect()
      const e = new MouseEvent('contextmenu', { cancelable: true, bubbles: true, composed: true, clientX: r.left + Math.min(4, r.width / 2), clientY: r.top + r.height / 2, button: 2 })
      el.dispatchEvent(e)
      return e.defaultPrevented
    }
    const fields = all.filter((e) => e.matches('input:not([type=hidden]):not([type=checkbox]):not([type=radio]):not([type=range]):not([type=button]):not([type=submit]), textarea, [contenteditable]:not([contenteditable=false])') && visible(e))
    for (const el of fields) {
      if (fire(el)) {
        const host = hostOf(el)
        const at = host && isK(host) ? host : el
        findings.push({ check: 'MENU-14', severity: 'error', element: describe(at), text: clip80(nameOf(at)), detail: 'a contextmenu event on the field is prevented: its edit menu is gone', box: boxOf(at) })
      }
    }
    let shown = 0
    const sample = T.filter((t) => !inside(t.el, 'input, textarea, [contenteditable]')).slice(0, 60)
    const sel = getSelection()
    for (const t of sample) {
      if (shown >= 5) break
      const range = document.createRange()
      range.selectNodeContents(t.nodes[0])
      sel?.removeAllRanges()
      sel?.addRange(range)
      if (fire(t.el)) {
        findings.push({ check: 'MENU-14', severity: 'error', element: describe(t.el), text: clip80(t.text), detail: 'a contextmenu event on selected text is prevented: Copy and Look Up are gone', box: boxOf(t.el) })
        shown++
      }
    }
    sel?.removeAllRanges()
    return { findings, texts }
  }

  // =================================================================================================================
  // check: the static audit
  // =================================================================================================================

  // ---- the text inventory --------------------------------------------------------------------------------------
  for (const t of T) {
    const c = t.c
    const size = px(c.fontSize)
    const step = styleOf(size)
    const col = rgba(c.color)
    texts.push({
      element: describe(t.el),
      text: clip80(t.text),
      family: c.fontFamily,
      size,
      lineHeight: c.lineHeight,
      weight: Number(c.fontWeight),
      tracking: c.letterSpacing === 'normal' ? 0 : px(c.letterSpacing),
      color: c.color,
      colorToken: col[3] === 0 ? 'transparent' : (tokenNames(col, t.st).find((n) => /^--(label|link|placeholder|ios-|tint|focus)/.test(n)) ?? tokenNames(col, t.st)[0] ?? ''),
      transform: c.textTransform,
      style: c.fontStyle,
      ramp: step ? step[3] : ''
    })
  }

  function overflowChecks(): void {
    // TYPE-26, TYPE-27: clipped and spilled text
    for (const t of T) {
      const el = t.el
      if (!(el instanceof HTMLElement)) continue
      const c = cs(el)
      const r = el.getBoundingClientRect()
      const pad = paddingBox(el, r)
      const beyond = Math.max(...t.rects.map((q) => Math.max(pad.l - q.left, q.right - pad.r)))
      if (c.overflowX !== 'visible') {
        if (el.scrollWidth > el.clientWidth + 1 && c.textOverflow !== 'ellipsis' && beyond > 1 && t.st === el) add('TYPE-26', 'error', el, t.text, `cut off: ${el.scrollWidth}px of text in ${el.clientWidth}px, text-overflow ${c.textOverflow}; expected an ellipsis`)
      } else if (el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 1 && beyond > 1 && t.st === el) {
        add('TYPE-27', 'error', el, t.text, `spills ${Math.round(beyond)}px out of its ${el.clientWidth}px box (overflow visible)`)
        continue
      }
      // the nearest clipping ancestor: hidden or clip (a scroller shows the rest by scrolling)
      for (let at: Element | null = el; at; at = parentOf(at)) {
        const ac = cs(at)
        if (ac.display === 'contents') continue
        if (ac.overflowX === 'visible' && ac.overflowY === 'visible') continue
        const hx = ac.overflowX === 'hidden' || ac.overflowX === 'clip'
        const hy = ac.overflowY === 'hidden' || ac.overflowY === 'clip'
        if (!hx && !hy) break
        if (at === el && hx) break // TYPE-26's
        if (ac.textOverflow === 'ellipsis' || (ac.getPropertyValue('-webkit-line-clamp') || 'none') !== 'none') break
        const p = paddingBox(at)
        const lh = t.c.lineHeight === 'normal' ? px(t.c.fontSize) * 1.2 : px(t.c.lineHeight)
        for (const q of t.rects) {
          const inX = q.right > p.l && q.left < p.r
          const inY = q.bottom > p.t && q.top < p.b
          if (!inX || !inY) continue // clipped away whole, not spilled
          const tolY = 1 + Math.max(0, (q.height - lh) / 2)
          const dx = hx ? Math.max(p.l - q.left, q.right - p.r) : 0
          const dy = hy ? Math.max(p.t - q.top, q.bottom - p.b) : 0
          if (dx > 1 || dy > tolY) {
            add('TYPE-27', 'error', el, t.text, `${dx > 1 ? `${Math.round(dx)}px` : `${Math.round(dy)}px`} of the text is cut off by ${describe(at)} (overflow ${ac.overflowX}/${ac.overflowY})`, q)
            break
          }
        }
        break
      }
    }

    // TYPE-28: text over text or controls (within one layer)
    if (on('TYPE-28')) {
      interface Piece {
        t: Txt
        /** Scrolled into view (cut by clips, not scrollers), and as seen now. */
        r: DOMRect
        v: DOMRect | null
        layer: Element | null
        scroller: Element | null
      }
      const pieces: Piece[] = []
      for (const t of T) {
        const layer = layerOf(t.el)
        const scroller = scrollerOf(t.st)
        const view = seen(t)
        for (const r of local(t)) pieces.push({ t, r, v: view.find((x) => x.left >= r.left - 0.5 && x.right <= r.right + 0.5 && x.top >= r.top - 0.5 && x.bottom <= r.bottom + 0.5) ?? null, layer, scroller })
      }
      const hit = (a: DOMRect, b: DOMRect): boolean => Math.min(a.right, b.right) - Math.max(a.left, b.left) > 1 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 1
      type Box2 = { r: DOMRect; v: DOMRect | null; scroller: Element | null }
      const meet = (a: Box2, b: Box2): boolean => (a.scroller === b.scroller ? hit(a.r, b.r) : !!a.v && !!b.v && hit(a.v, b.v))
      const reported = new Set<Element>()
      pieces.sort((a, b) => a.r.top - b.r.top)
      for (let i = 0; i < pieces.length; i++) {
        const a = pieces[i]
        if (reported.has(a.t.el)) continue
        for (let j = i + 1; j < pieces.length; j++) {
          const b = pieces[j]
          if (b.r.top >= a.r.bottom - 1) break
          if (a.t === b.t || a.layer !== b.layer) continue
          if (contains(a.t.el, b.t.el) || contains(b.t.el, a.t.el)) continue
          if (meet(a, b)) {
            reported.add(a.t.el)
            add('TYPE-28', 'error', a.t.el, a.t.text, `overlaps the text “${clip80(b.t.text).slice(0, 40)}” of ${describe(b.t.el)}`, a.r)
            break
          }
        }
      }
      const labelsOf = (i: Element): Element[] => {
        const out: Element[] = []
        const target = isK(i) ? innerTarget(i) : i
        if (target && 'labels' in target) for (const l of Array.from((target as HTMLInputElement).labels ?? [])) out.push(l)
        for (const e of [i, target]) {
          const by = e?.getAttribute('aria-labelledby')
          if (by && e) for (const id of by.split(/\s+/)) {
            const l = (e.getRootNode() as Document | ShadowRoot).getElementById?.(id)
            if (l) out.push(l)
          }
        }
        return out
      }
      for (const i of I) {
        const hl = localHit(i)
        if (!hl) continue
        const hb: Box2 = { r: hl, v: seenHit(i), scroller: scrollerOf(i) }
        const layer = layerOf(i)
        const labels = labelsOf(i)
        for (const p of pieces) {
          if (reported.has(p.t.el) || p.layer !== layer) continue
          if (contains(i, p.t.el) || contains(p.t.el, i) || labels.some((l) => contains(l, p.t.el))) continue
          if (meet(p, hb)) {
            reported.add(p.t.el)
            add('TYPE-28', 'error', p.t.el, p.t.text, `overlaps the hit box of ${describe(i)} (${Math.round(hl.width)}×${Math.round(hl.height)})`, p.r)
          }
        }
      }
    }
  }
  function colourChecks(): void {
    // WRITE-31 placeholders; COLOR-17
    for (const f of all.filter((e) => (e.localName === 'input' || e.localName === 'textarea') && visible(e))) {
      const ph = (f.getAttribute('placeholder') ?? '').trim()
      if (!ph) continue
      const host = hostOf(f)
      const at = host && isK(host) ? host : f
      const nm = nameOf(f)
      if (!nm || nm === ph) add('WRITE-31', 'error', at, ph, nm ? `the accessible name is the placeholder “${ph}”; give the field a label` : 'no accessible name besides the placeholder')
      if (!(f as HTMLInputElement).value && !disabledChain(f)) {
        const pc = getComputedStyle(f, '::placeholder')
        const fg = rgba(pc.color)
        const j = judge([fg[0], fg[1], fg[2], fg[3] * opacityChain(f)], f, 4.5)
        if (j.verdict !== 'pass') add('COLOR-17', j.verdict === 'fail' ? 'error' : 'warn', at, ph, `placeholder ${r1(j.ratio)}:1 (${hex(fg)} on ${hex(j.bg)}); expected 4.5:1${j.verdict === 'fail' ? '' : ' (background unknown: judge it in the picture)'}`)
      }
    }

    // ---- colour ------------------------------------------------------------------------------------------------
    for (const el of all) {
      if (!visible(el)) continue
      const c = cs(el)
      const t = tOf.get(el)
      const A = isA(el)
      const props: [string, RGBA][] = []
      if (t) props.push(['color', rgba(t.c.color)])
      props.push(['background-color', rgba(c.backgroundColor)])
      for (const sd of ['top', 'right', 'bottom', 'left']) {
        if (px(c.getPropertyValue(`border-${sd}-width`)) > 0 && c.getPropertyValue(`border-${sd}-style`) !== 'none') props.push([`border-${sd}-color`, rgba(c.getPropertyValue(`border-${sd}-color`))])
      }
      if (c.outlineStyle !== 'none' && px(c.outlineWidth) > 0) props.push(['outline-color', rgba(c.outlineColor)])
      if (el instanceof SVGElement) {
        if (c.fill && c.fill !== 'none' && !c.fill.startsWith('url(')) props.push(['fill', rgba(c.fill)])
        if (c.stroke && c.stroke !== 'none' && !c.stroke.startsWith('url(')) props.push(['stroke', rgba(c.stroke)])
      }
      // COLOR-05: Apple's default red and blue, anywhere
      for (const [p, v] of props) {
        if (v[3] === 0) continue
        const h = hex(v).toLowerCase()
        if (['#ff3b30', '#ff453a', '#007aff', '#0a84ff'].includes(h)) add('COLOR-05', 'error', el, t?.text ?? '', `${p} ${h}: Apple's default; expected --ios-red / --ios-blue (the accessible variants)`)
      }
      if (!A) continue
      // a slotted text is coloured by the kit element that draws it (its slot), not by the app
      const tA = t && isA(t.st) ? t : undefined
      // COLOR-02: every colour a token value
      const seenProps = new Set<string>()
      for (const [p, v] of props) {
        if (v[3] === 0 || (p === 'color' && !tA)) continue
        const k = p.startsWith('border') ? 'border' : p
        if (seenProps.has(k)) continue
        if (!tokenNames(v, el).length && !derived(v, el)) {
          seenProps.add(k)
          add('COLOR-02', 'error', el, t?.text ?? '', `${p} ${hex(v)} is no colour token of the ${themeOf(el)} theme`)
        }
      }
      const bg = rgba(c.backgroundColor)
      const r = el.getBoundingClientRect()
      // COLOR-04, COLOR-03, COLOR-06, COLOR-08 on text
      if (tA) {
        const t = tA
        const col = rgba(t.c.color)
        const white = same(col, [255, 255, 255, 1])
        const black = same(col, [0, 0, 0, 1])
        if (!white && !black && is(col, t.st, BG_FILL_SEP) && !is(col, t.st, LABELS)) add('COLOR-04', 'error', el, t.text, `text colour ${tokenNames(col, t.st).filter((n) => BG_FILL_SEP.includes(n)).join(' = ')}: a background, fill or separator token as text`)
        if (!is(col, t.st, ['--label', '--label-secondary', '--tint', '--link', '--ios-red', '--label-tertiary']) && !derived(col, t.st)) {
          const bd = backdrop(t.st)
          const behind = bd.color
          const onFill = white && bd.cands.every((b) => is(b, el, ['--tint', '--tint-fill', ...SYSTEM]))
          // white over a picture (a wallpaper) or a material: whether it is on a tint or system colour cannot be read
          if (!onFill) add('COLOR-03', white && !bd.known ? 'warn' : 'error', el, t.text, `text colour ${hex(col)}${tokenNames(col, t.st).length ? ` (${tokenNames(col, t.st)[0]})` : ''}${white ? ` on ${hex(behind)}` : ''}; expected --label, --label-secondary, --tint, --link, --ios-red, or white on a tint or system colour`)
        }
        if (is(col, t.st, ['--tint', '--tint-fill']) && !tintOk(el)) add('COLOR-06', 'error', el, t.text, `tinted text (${hex(col)}) on something that does not act`)
        if (is(col, t.st, ['--ios-red']) && !redOk(el)) add('COLOR-08', 'error', el, t.text, `red text (${hex(col)}) that is not destructive or an error`)
      }
      if (bg[3] > 0) {
        if (r.width >= 24 && r.height >= 24 && is(bg, el, LABELS) && !is(bg, el, COLOR_TOKENS.filter((n) => !LABELS.includes(n) && n !== '--placeholder')) && !inside(el, 'canvas, video, .xterm') && !el.querySelector('canvas, video, .xterm')) add('COLOR-04', 'error', el, '', `background ${tokenNames(bg, el).filter((n) => LABELS.includes(n)).join(' = ')} on a ${Math.round(r.width)}×${Math.round(r.height)} element: a label token as a background`)
        if (is(bg, el, ['--tint', '--tint-fill']) && !tintOk(el)) add('COLOR-06', 'error', el, '', `tinted background (${hex(bg)}) on something that does not act`)
        if (is(bg, el, ['--ios-red']) && !redOk(el)) add('COLOR-08', 'error', el, '', `red background (${hex(bg)}) that is not destructive or an error`)
        if (r.width * r.height > 40000 && is(bg, el, COLOR_TOKENS.filter((n) => n.startsWith('--fill')))) add('COLOR-13', 'error', el, '', `${tokenNames(bg, el).find((n) => n.startsWith('--fill'))} on a ${Math.round(r.width)}×${Math.round(r.height)} area; fills are for small shapes`)
      }
      // COLOR-12: lines
      for (const sd of ['top', 'right', 'bottom', 'left']) {
        const w = px(c.getPropertyValue(`border-${sd}-width`))
        if (w <= 0 || w > 2 || c.getPropertyValue(`border-${sd}-style`) === 'none') continue
        const bc = rgba(c.getPropertyValue(`border-${sd}-color`))
        if (bc[3] === 0) continue
        if (!is(bc, el, SEPARATORS)) {
          add('COLOR-12', 'error', el, '', `border-${sd} ${w}px ${hex(bc)}; expected --separator or --separator-opaque`)
          break
        }
      }
      if (bg[3] > 0 && ((r.height <= 2 && r.width >= 24) || (r.width <= 2 && r.height >= 24)) && !is(bg, el, SEPARATORS)) add('COLOR-12', 'error', el, '', `a ${Math.round(r.width)}×${r1(r.height)} line in ${hex(bg)}; expected --separator or --separator-opaque`)
      // COLOR-20: a forced theme around chrome
      if (el !== root && (el.hasAttribute('data-theme') || ((c.colorScheme === 'light' || c.colorScheme === 'dark') && c.colorScheme !== cs(root).colorScheme && cs(parentOf(el) ?? root).colorScheme !== c.colorScheme))) {
        const d = descendants(el)
        const chrome = d.find((x) => x.localName === 'ui-list' || x.localName === 'ui-button' || bars.includes(x))
        if (chrome) add('COLOR-20', 'error', el, '', `${el.hasAttribute('data-theme') ? `data-theme="${el.getAttribute('data-theme')}"` : `color-scheme ${c.colorScheme}`} forced around ${describe(chrome)}; chrome follows the person's theme`)
      }
    }
    function tintOk(el: Element): boolean {
      return insideInteractive(el) || !!inside(el, 'ui-segmented, ui-toggle, ui-check, ui-row[action], ui-row[selected], [aria-selected=true], ui-meter')
    }
    function redOk(el: Element): boolean {
      return !!inside(el, '[destructive], .destructive, [footer-tone=error], [invalid], [aria-invalid=true], [role=alert], ui-banner[tone=error], svg[role=img]') || (kitInternal(el) && hostOf(el)?.localName === 'ui-row')
    }

    // COLOR-14, COLOR-15: contrast
    for (const t of T) {
      if (disabledChain(t.el)) continue
      const fg = rgba(t.c.color)
      const large = isLarge(t)
      const need = large ? 3 : 4.5
      const j = judge([fg[0], fg[1], fg[2], fg[3] * opacityChain(t.st)], t.st, need)
      if (j.verdict !== 'pass') add(large ? 'COLOR-15' : 'COLOR-14', j.verdict === 'fail' ? 'error' : 'warn', t.el, t.text, `${r1(j.ratio)}:1 (${hex(fg)} on ${hex(j.bg)}); expected ${need}:1${j.verdict === 'fail' ? '' : ' (over an image, a material, a gradient or nothing opaque: judge it in the picture)'}`)
    }
  // COLOR-16: meaningful glyphs
    for (const svg of all.filter((e) => e.localName === 'svg' && visible(e))) {
      if (disabledChain(svg)) continue
      const named = !!nameOf(svg) && svg.getAttribute('aria-hidden') !== 'true'
      const owner = I.find((i) => contains(i, svg) && iconOnly(i) && descendants(i).filter((d) => d.localName === 'svg' && visible(d)).length === 1)
      if (!named && !owner) continue
      const c = cs(svg)
      let paint = c.stroke && c.stroke !== 'none' ? c.stroke : c.fill
      if (!paint || paint === 'none' || paint.startsWith('url(')) {
        const shape = svg.querySelector('path, circle, rect, line, polyline, polygon, ellipse')
        if (shape) {
          const sc = cs(shape)
          paint = sc.stroke && sc.stroke !== 'none' ? sc.stroke : sc.fill
        }
      }
      if (!paint || paint === 'none' || paint.startsWith('url(')) continue
      const fg = rgba(paint)
      const j = judge([fg[0], fg[1], fg[2], fg[3] * opacityChain(svg)], svg, 3)
      if (j.verdict !== 'pass') add('COLOR-16', j.verdict === 'fail' ? 'error' : 'warn', owner ?? svg, nameOf(owner ?? svg), `glyph ${r1(j.ratio)}:1 (${hex(fg)} on ${hex(j.bg)}); expected 3:1${j.verdict === 'fail' ? '' : ' (over an image, a material, a gradient or nothing opaque: judge it in the picture)'}`)
    }
  }
  function darkSurfaces(): void {
    // COLOR-11: elevated surfaces in dark
    if (run === 'dark') {
      const surfaces: Element[] = [...modals, ...popovers]
      if (o.app) surfaces.push(contentRoot)
      for (const w of all.filter((e) => e.classList.contains('ios-window') && visible(e))) {
        const content = Array.from(w.children).find((c) => /-content$/.test(c.getAttribute('data-testid') ?? ''))
        if (content) surfaces.push(content)
      }
      for (const sf of surfaces) {
        const sr = sf.getBoundingClientRect()
        let b = rgba(cs(sf).backgroundColor)
        let at: Element = sf
        if (b[3] < 0.999) {
          const cover = descendants(sf).find((d) => {
            const dr2 = d.getBoundingClientRect()
            return visible(d) && rgba(cs(d).backgroundColor)[3] >= 0.999 && dr2.width * dr2.height >= 0.8 * sr.width * sr.height
          })
          if (!cover) continue
          b = rgba(cs(cover).backgroundColor)
          at = cover
        }
        if (!same(b, [0, 0, 0, 1])) continue
        const media = descendants(sf).filter((d) => d.matches('canvas, video, .xterm') && visible(d))
        const area = media.reduce((n, d) => n + d.getBoundingClientRect().width * d.getBoundingClientRect().height, 0)
        if (area >= 0.8 * sr.width * sr.height) continue
        add('COLOR-11', 'error', at, '', `${sf === contentRoot ? 'the window content' : 'a modal surface'} is #000000 in dark; expected --bg-elevated or --bg-grouped-elevated`)
      }
    }
  }
  function motionChecks(): void {
    // ---- motion and materials ------------------------------------------------------------------------------------
    for (const el of all) {
      if (!isA(el) || !visible(el)) continue
      const c = cs(el)
      const bf = c.backdropFilter
      const floating = bars.some((b) => contains(b, el)) || popovers.some((p) => contains(p, el)) || menus.some((m) => contains(m, el)) || modals.some((m) => contains(m, el)) || !!inside(el, '[role=menu], [role=dialog], [role=alertdialog], dialog')
      if (on('MOTION-01') && bf && bf !== 'none' && contains(contentRoot, el) && !floating) add('MOTION-01', 'error', el, '', `backdrop-filter ${bf} on content that does not float`)
      if (run === 'default') {
        if (bf && bf !== 'none' && bf !== materialBlur) add('MOTION-03', 'error', el, '', `backdrop-filter ${bf}; expected --material-blur (${materialBlur})`)
      }
      if (run === 'default' || run === 'dark') {
        const b = rgba(c.backgroundColor)
        if (b[3] > 0.005 && b[3] < 0.995 && !is(b, el, MATERIAL_OK) && !derived(b, el)) add('MOTION-03', 'error', el, '', `translucent background ${hex(b)}; expected a --material-*, --fill*, --separator* or --dimming value`)
      }
      if (run === 'reduced' && bf && bf !== 'none') add('MOTION-03', 'error', el, '', `backdrop-filter ${bf} under Reduce Transparency; expected none`)
      // MOTION-06, MOTION-07: durations and easing
      if (run === 'default') {
        const props = c.transitionProperty.split(',').map((x) => x.trim())
        const durs = c.transitionDuration.split(',').map((x) => x.trim())
        const eases = c.transitionTimingFunction.split(/,(?![^(]*\))/).map((x) => x.trim())
        if (c.transitionProperty !== 'none') {
          let d6 = false
          let d7 = false
          props.forEach((p, k) => {
            const d = durs[k % durs.length]
            if (!d6 && !['0s', '0.18s', '0.3s', '0.45s'].includes(d)) {
              d6 = true
              add('MOTION-06', 'error', el, '', `transition of ${p} lasts ${d}; expected 0.18s, 0.3s or 0.45s (--duration-*)`)
            }
            const e = eases[k % eases.length]
            if (!d7 && px(d) > 0 && /^(all|transform|translate|scale|rotate|width|height|top|left|right|bottom|inset)$/.test(p) && e !== 'cubic-bezier(0.32, 0.72, 0, 1)' && e !== 'cubic-bezier(0.34, 1.36, 0.64, 1)') {
              d7 = true
              add('MOTION-07', 'error', el, '', `${p} moves with ${e}; expected --ease-ios or --ease-spring`)
            }
          })
        }
        if (c.animationName !== 'none') {
          const ad = c.animationDuration.split(',').map((x) => x.trim())
          const loops = c.animationIterationCount.split(',').map((x) => x.trim())
          for (const [k, d] of ad.entries()) if (loops[k % loops.length] !== 'infinite' && !['0s', '0.18s', '0.3s', '0.45s'].includes(d)) {
            add('MOTION-06', 'error', el, '', `animation ${c.animationName} lasts ${d}; expected 0.18s, 0.3s or 0.45s`)
            break
          }
        }
      }
    }
  }
  function reducedChecks(): void {
    // A11Y-15: nothing moves under Reduce Motion
    if (run === 'reduced') {
      const excluded = (el: Element): boolean => !!inside(el, 'ui-spinner, .ios-spinner')
      for (const el of all) {
        if (excluded(el)) continue
        const c = cs(el)
        const t = c.transitionDuration.split(',').map((x) => px(x))
        const a = c.animationName !== 'none' ? c.animationDuration.split(',').map((x) => px(x)) : []
        const tp = c.transitionProperty
        if (tp !== 'none' && t.some((x) => x > 0)) add('A11Y-15', 'error', el, '', `transition ${c.transitionProperty} ${c.transitionDuration} under Reduce Motion; expected 0s`)
        else if (a.some((x) => x > 0)) add('A11Y-15', 'error', el, '', `animation ${c.animationName} ${c.animationDuration} under Reduce Motion; expected 0s`)
        if (c.scrollBehavior === 'smooth' && (el === root || /auto|scroll/.test(c.overflowY + c.overflowX))) add('A11Y-15', 'error', el, '', 'scroll-behavior smooth under Reduce Motion')
      }
      const anims = [...document.getAnimations(), ...shadowRoots.flatMap((sr) => sr.getAnimations())]
      for (const an of new Set(anims)) {
        const target = (an.effect as KeyframeEffect | null)?.target
        const dur = Number(an.effect?.getTiming().duration ?? 0)
        if (!target || excluded(target) || !(dur > 0)) continue
        add('A11Y-15', 'error', target, '', `a running animation of ${Math.round(dur)} ms under Reduce Motion`)
      }
    }

    // MOTION-03 (c): opaque surfaces under Reduce Transparency
    if (run === 'reduced') {
      for (const sf of [...bars, ...popovers, ...menus, ...modals]) {
        const layer = [sf, ...descendants(sf)].find((d) => rgba(cs(d).backgroundColor)[3] > 0.005)
        if (!layer) continue
        const b = rgba(cs(layer).backgroundColor)
        if (b[3] < 0.995) add('MOTION-03', 'error', layer, '', `a ${bars.includes(sf) ? 'bar' : 'floating surface'} at alpha ${r1(b[3])} under Reduce Transparency; expected opaque`)
      }
    }
  }
  if (run === 'large-text' || run === 'smallest') {
    overflowChecks()
    // LAYOUT-12: the content root holds at its smallest
    if (run === 'smallest' && contentRoot instanceof HTMLElement && contentRoot.scrollWidth > contentRoot.clientWidth + 1) add('LAYOUT-12', 'error', contentRoot, '', `the content is ${contentRoot.scrollWidth}px wide in a ${contentRoot.clientWidth}px window: it scrolls or cuts sideways`)
    probeWrap.remove()
    return { findings, texts }
  }
  if (run === 'reduced') {
    reducedChecks()
    motionChecks()
    probeWrap.remove()
    return { findings, texts }
  }
  if (run === 'dark') {
    colourChecks()
    darkSurfaces()
    motionChecks()
    probeWrap.remove()
    return { findings, texts }
  }
  // ---- typography over T --------------------------------------------------------------------------------------
  const largeTitles: Txt[] = []
  for (const t of T) {
    const c = t.c
    const el = t.el
    const size = px(c.fontSize)
    const weight = Number(c.fontWeight)
    const step = styleOf(size)
    const text = t.text
    if (!step) add('TYPE-01', 'error', el, text, `font-size ${r1(size)}px${s !== 1 ? ` (${r1(size / s)}px at scale 1)` : ''}; expected a ramp size: 34, 28, 22, 20, 17, 16, 15, 13, 12 or 11`)
    if (step) {
      if (c.lineHeight === 'normal') add('TYPE-03', 'error', el, text, `line-height normal; expected ${r1(step[1] * s)}px for ${step[0]}px text`)
      else if (Math.abs(px(c.lineHeight) / s - step[1]) > 0.5) add('TYPE-03', 'error', el, text, `line-height ${r1(px(c.lineHeight))}px; expected ${r1(step[1] * s)}px for ${step[0]}px text`)
      const ls = c.letterSpacing === 'normal' ? 0 : px(c.letterSpacing)
      if (Math.abs(ls - step[2]) > 0.02) add('TYPE-25', 'error', el, text, `letter-spacing ${r1(ls)}px; expected ${step[2]}px for ${step[0]}px text`)
      if (step[0] <= 20 && weight === 700) add('TYPE-21', 'error', el, text, `${step[0]}px text at weight 700; at 20px and under emphasis is 600`)
      if (step[0] >= 22 && (weight === 500 || weight === 600)) add('TYPE-21', 'error', el, text, `${step[0]}px text at weight ${weight}; at 22px and over emphasis is 700`)
      if (step[0] === 34) {
        if (weight !== 700) add('TYPE-07', 'error', el, text, `Large Title (34px) at weight ${weight}; expected 700`)
        largeTitles.push(t)
      }
    }
    if (run === 'default' && size < 10.75) add('TYPE-04', 'error', el, text, `font-size ${r1(size)}px; nothing is smaller than Caption 2 (11px)`)
    if (![400, 500, 600, 700].includes(weight)) add('TYPE-20', 'error', el, text, `font-weight ${weight}; expected 400, 500, 600 or 700`)
    const fam = firstFamily(c)
    if (fam !== 'InterVariable' && fam !== 'ui-monospace') add('TYPE-23', 'error', el, text, `first font family "${fam}"; expected InterVariable (--font-text, --font-display) or ui-monospace`)
    if (fam === 'ui-monospace' && !inside(el, 'code, pre, kbd, samp, textarea, [role=log], canvas, .xterm')) add('TYPE-24', 'warn', el, text, 'monospace outside code, pre, kbd, samp or a log: is it code or a machine value?')
    if (c.textAlign === 'center' && t.lines >= 2 && !inside(el, '[role=alertdialog], ui-empty-state, ui-button, button')) add('TYPE-33', 'warn', el, text, `${t.lines} lines centred; text aligns to the leading edge`)
    if (c.textAlign === 'justify') add('TYPE-34', 'error', el, text, 'text-align justify')
    if (size <= 20 * s + 0.01 && t.lines >= 2 && el instanceof HTMLElement && el.clientWidth > 0) {
      const ec = cs(el)
      const w = el.clientWidth - px(ec.paddingLeft) - px(ec.paddingRight)
      if (w > 40 * size) add('TYPE-36', 'error', el, text, `measure ${Math.round(w)}px for ${r1(size)}px text (${r1(w / size)} em); keep it at most 40 em (${Math.round(40 * size)}px)`)
    }
    if (c.textTransform === 'uppercase') {
      if (size > 13 * s + 0.25) add('TYPE-37', 'error', el, text, `uppercase at ${r1(size)}px; capitals only at 13px and under`)
      else if (text.length > 40) add('TYPE-37', 'error', el, text, `uppercase text of ${text.length} characters; capitals only for labels of 40 or fewer`)
      else if (t.lines >= 2) add('TYPE-37', 'error', el, text, `uppercase text on ${t.lines} lines; capitals only for one-line labels`)
    }
    if (c.fontVariantCaps !== 'normal') add('TYPE-39', 'error', el, text, `font-variant-caps ${c.fontVariantCaps}; expected normal`)
    if ((c.fontStyle === 'italic' || c.fontStyle.startsWith('oblique')) && !inside(el, 'em, i, cite, blockquote, [contenteditable]:not([contenteditable=false])')) add('TYPE-40', 'error', el, text, `font-style ${c.fontStyle} outside em, i, cite, blockquote or editable content`)
    if (c.textDecorationLine.includes('underline') && !inside(el, 'a[href]')) add('TYPE-41', 'error', el, text, 'underlined text that is not a link')
    const col = rgba(c.color)
    if (same(col, tok(t.st, '--label-tertiary')) && !disabledChain(el)) add('TYPE-43', 'error', el, text, `colour --label-tertiary (${hex(col)}) on text that is read; tertiary is for disabled text and glyphs`)
    const opa = opacityChain(t.st)
    if (opa < 0.999 && !disabledChain(el)) {
      let animated = false
      for (let at: Element | null = t.st; at && !animated; at = parentOf(at)) if (at.getAnimations({ subtree: false }).length) animated = true
      if (!animated) add('TYPE-44', 'error', el, text, `opacity ${r1(opa)} on the text or its ancestors; secondary text is --label-secondary, not opacity`)
    }
  }
  if (largeTitles.length > 1) for (const t of largeTitles.slice(1)) add('TYPE-07', 'warn', t.el, t.text, `${largeTitles.length} Large Titles in the window: is there one per view?`)

  overflowChecks()

  // TYPE-08, TYPE-18, NAV-*, INPUT-03, CTRL-28: bars
  for (const bar of bars) {
    const items = barItems(bar)
    const br = bar.getBoundingClientRect()
    const h1s = descendants(bar).filter((d) => d.localName === 'h1' && visible(d))
    for (const h of h1s) {
      const ht = tOf.get(h) ?? T.find((t) => contains(h, t.el))
      const c = ht ? ht.c : cs(h)
      const step = styleOf(px(c.fontSize))
      const name = collapse(h.textContent ?? '')
      if (!step || step[0] !== 17 || Number(c.fontWeight) !== 600) add('TYPE-08', 'error', h, name, `bar title ${r1(px(c.fontSize))}px at weight ${c.fontWeight}; expected Headline (17px, 600)`)
      const hr = h.getBoundingClientRect()
      const left = items.some((i) => hitBox(i).right <= hr.left + 1)
      const right = items.some((i) => hitBox(i).left >= hr.right - 1)
      const off = Math.abs((hr.left + hr.right) / 2 - (br.left + br.right) / 2)
      if (left && right && off > 2) add('TYPE-08', 'error', h, name, `bar title centre ${Math.round(off)}px off the bar's centre, with items on both sides; expected centred`)
      if (name.length > 24) add('NAV-07', 'warn', h, name, `bar title of ${name.length} characters; a title is short (24 or fewer)`)
      const fails = titleCase(name)
      if (fails.length) add('WRITE-01', 'error', h, name, `title not in title case: ${fails.map((w) => `“${w}”`).join(', ')}`)
    }
    const semibold = items.filter((i) => visibleText(i) && textWeight(i) === 600)
    if (semibold.length >= 2) add('TYPE-18', 'error', bar, semibold.map(visibleText).join(', '), `${semibold.length} semibold items (${semibold.map((i) => `“${visibleText(i)}”`).join(', ')}); only the confirming item is semibold`)
    for (const i of items) {
      if (i.localName === 'ui-button') {
        const v = i.getAttribute('variant') ?? 'filled'
        if (v !== 'plain') add('NAV-10', 'error', i, nameOf(i), `ui-button variant ${v} in a bar; bar items are plain`)
      } else if (!kitInternal(i) && !isK(i)) {
        const c = cs(i)
        const bg = rgba(c.backgroundColor)
        const border = ['Top', 'Right', 'Bottom', 'Left'].some((sd) => px(c.getPropertyValue(`border-${sd.toLowerCase()}-width`)) > 0 && c.getPropertyValue(`border-${sd.toLowerCase()}-style`) !== 'none' && rgba(c.getPropertyValue(`border-${sd.toLowerCase()}-color`))[3] > 0)
        if (bg[3] > 0 || border) add('NAV-10', 'error', i, nameOf(i), `${bg[3] > 0 ? `background ${hex(bg)}` : 'a visible border'} on a bar item at rest; bar items are borderless`)
      }
    }
    const named = (re: RegExp): Element[] => items.filter((i) => re.test(nameOf(i)))
    const cancel = named(/^Cancel$/)
    const confirm = named(/^(Done|Save|Add|Send)$/)
    for (const c of cancel) for (const d of confirm) if (hitBox(c).left > hitBox(d).left) add('NAV-11', 'error', c, 'Cancel', `Cancel (x ${Math.round(hitBox(c).left)}) trails “${nameOf(d)}” (x ${Math.round(hitBox(d).left)}); Cancel leads`)
    const centre = (br.left + br.right) / 2
    for (const i of items) {
      const nm = nameOf(i)
      const plus = iconOnly(i) && descendants(i).some((d) => d.localName === 'ui-icon' && d.getAttribute('name') === 'plus')
      if (/^(Edit|Add)$/.test(nm) || plus) {
        const h = hitBox(i)
        if ((h.left + h.right) / 2 < centre) add('NAV-12', 'error', i, nm, `“${nm || '+'}” sits in the leading half of the bar; Edit and Add trail`)
      }
    }
    if (items.length > 5) add('NAV-13', 'warn', bar, '', `${items.length} items in one bar; keep it to five or fewer, grouped`)
    const sorted = [...items].sort((a, b) => hitBox(a).left - hitBox(b).left)
    for (let k = 1; k < sorted.length; k++) {
      const a = hitBox(sorted[k - 1])
      const b = hitBox(sorted[k])
      const dist = (b.left + b.right) / 2 - (a.left + a.right) / 2
      const gap = b.left - a.right
      if (dist < 44 && gap > 1 && Math.abs((a.top + a.bottom) / 2 - (b.top + b.bottom) / 2) < 22) add('INPUT-03', 'error', sorted[k], nameOf(sorted[k]), `${Math.round(gap)}px dead gap to “${nameOf(sorted[k - 1])}” (centres ${Math.round(dist)}px apart); bar hit boxes meet`)
    }
    const icons = descendants(bar).filter((d) => d.localName === 'ui-icon' && d.parentElement && items.includes(d.parentElement) && visible(d))
    const sizes = new Set(icons.map((e) => `${(e as unknown as { size: number }).size}/${(e as unknown as { weight: number }).weight}`))
    if (icons.length > 1 && sizes.size > 1) add('CTRL-28', 'error', bar, '', `bar glyphs at ${[...sizes].join(', ')} (size/weight); one size and weight per bar`)
  }
  function textWeight(el: Element): number {
    const ts = T.find((t) => contains(el, t.el) || (t.st !== t.el && contains(el, t.st)))
    if (ts) return Number(ts.c.fontWeight)
    const inner = isK(el) ? innerTarget(el) : null
    return Number(cs(inner ?? el).fontWeight)
  }
  function titleCase(str: string): string[] {
    const SMALL = new Set(['a', 'an', 'the', 'and', 'but', 'or', 'nor', 'for', 'so', 'yet', 'as', 'at', 'by', 'from', 'in', 'into', 'of', 'off', 'on', 'onto', 'out', 'over', 'per', 'to', 'up', 'via', 'with'])
    const words = str.split(/\s+/).map((w) => w.replace(/^[“”"'’()…:.!?]+|[“”"'’()…:.!?]+$/g, '')).filter(Boolean)
    return words.filter((w, i) => {
      if (!/^\p{Ll}/u.test(w)) return false
      if (/^[a-z]+[A-Z]/.test(w)) return false
      if (i === 0 || i === words.length - 1) return true
      if (/\d/.test(w)) return false
      if ((w.match(/\p{L}/gu) ?? []).length < 4) return false
      return !SMALL.has(w.toLowerCase())
    })
  }

  // TYPE-09: row titles
  for (const row of all.filter((e) => e.localName === 'ui-row')) {
    const title = row.shadowRoot?.querySelector('.title')
    if (!title || !visible(title)) continue
    const c = cs(title)
    const step = styleOf(px(c.fontSize))
    if (!step || step[0] !== 17 || Number(c.fontWeight) !== 400) add('TYPE-09', 'error', title, title.textContent ?? '', `row title ${r1(px(c.fontSize))}px at weight ${c.fontWeight}; expected Body (17px, 400)`)
  }

  // TYPE-16: timestamps quieter than the row title
  for (const t of T) {
    if (!/\b\d{1,2}:\d{2}\b|\byesterday\b|\bago\b/i.test(t.text)) continue
    const row = inside(t.el, 'ui-row, [role=listitem], [role=option], [role=row]')
    if (!row) continue
    const titleEl = row.localName === 'ui-row' ? row.shadowRoot?.querySelector('.title') : null
    const title = titleEl && visible(titleEl) ? { c: cs(titleEl), el: titleEl } : (() => {
      const f = T.find((x) => contains(row, x.el))
      return f ? { c: f.c, el: f.el } : null
    })()
    if (!title || title.el === t.el) continue
    if (px(t.c.fontSize) >= px(title.c.fontSize) || Number(t.c.fontWeight) > Number(title.c.fontWeight)) add('TYPE-16', 'warn', t.el, t.text, `metadata ${r1(px(t.c.fontSize))}px/${t.c.fontWeight} against the title's ${r1(px(title.c.fontSize))}px/${title.c.fontWeight}; metadata is smaller or lighter`)
  }

  // message elements (TYPE-32, WRITE-05)
  const messages: Element[] = []
  for (const e of all) {
    if (e.localName === 'ui-section') {
      const f = e.shadowRoot?.querySelector('.footer')
      if (f && visible(f)) messages.push(f)
    } else if (e.localName === 'ui-alert') {
      const m = e.shadowRoot?.querySelector('#message')
      if (m && visible(m)) messages.push(m)
    } else if (e.localName === 'ui-banner') {
      const slot = e.shadowRoot?.querySelector('slot:not([name])') as HTMLSlotElement | null
      for (const n of slot?.assignedElements({ flatten: true }) ?? []) if (visible(n)) messages.push(n)
      if (slot && slot.assignedNodes().some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim()) && visible(e)) messages.push(e)
    } else if (e.localName === 'ui-empty-state') {
      const p = e.shadowRoot?.querySelector('p')
      if (p && visible(p)) messages.push(p)
    } else if (e.getAttribute('role') === 'alert' && visible(e)) messages.push(e)
  }
  for (const m of new Set(messages)) {
    const scope = [m, ...descendants(m)].filter((d) => d === m || tOf.has(d))
    for (const d of scope) {
      const c = cs(d)
      const clamp = c.getPropertyValue('-webkit-line-clamp') || 'none'
      if ((c.textOverflow === 'ellipsis' && c.whiteSpace === 'nowrap') || clamp !== 'none') {
        add('TYPE-32', 'error', d, rendered(m), clamp !== 'none' ? `-webkit-line-clamp ${clamp} on a message; messages wrap` : 'ellipsis with white-space nowrap on a message; messages wrap')
        break
      }
    }
    const txt = collapse(rendered(m))
    let caps = 0
    for (const sentence of txt.split(/[.!?]\s+/)) {
      const words = sentence.split(/\s+/).map((w) => w.replace(/^[^\p{L}]+|[^\p{L}]+$/gu, '')).filter(Boolean)
      for (const w of words.slice(1)) if (/^\p{Lu}/u.test(w) && w !== w.toUpperCase()) caps++
    }
    if (caps >= 3) add('WRITE-05', 'warn', m, txt, `${caps} capitalised words after the first of a sentence: is it sentence case?`)
  }

  // TYPE-35: numbers that line up
  for (const list of all.filter((e) => e.localName === 'ui-list' && visible(e))) {
    const NUM = /^[\d\s.,:%+\-−]+\s?[A-Za-z%]{0,3}$/
    const vals: Element[] = []
    for (const row of descendants(list).filter((d) => d.localName === 'ui-row' && visible(d))) {
      const v = row.shadowRoot?.querySelector('.value')
      if (v && NUM.test(collapse(v.textContent ?? ''))) vals.push(v)
      for (const tr of Array.from(row.children).filter((c) => c.getAttribute('slot') === 'trailing')) if (NUM.test(visibleText(tr))) vals.push(tr)
    }
    const prop = vals.filter((v) => !cs(v).fontVariantNumeric.includes('tabular-nums'))
    if (vals.length >= 2 && prop.length) add('TYPE-35', 'error', prop[0], prop.map((v) => collapse(v.textContent ?? '')).join(', '), `${prop.length} of ${vals.length} numeric row values without tabular-nums (font-variant-numeric ${cs(prop[0]).fontVariantNumeric})`)
  }

  // TYPE-47: heading order
  {
    const hs = all.filter((e) => (/^h[1-6]$/.test(e.localName) || (e.getAttribute('role') === 'heading' && e.hasAttribute('aria-level'))) && !/^(presentation|none)$/.test(e.getAttribute('role') ?? '') && visible(e) && e.getAttribute('aria-hidden') !== 'true')
    const level = (e: Element): number => (e.getAttribute('role') === 'heading' ? Number(e.getAttribute('aria-level')) : Number(e.localName[1]))
    let prev = 0
    const earlier: { lvl: number; size: number }[] = []
    for (const h of hs) {
      const lvl = level(h)
      const ht = tOf.get(h) ?? T.find((t) => contains(h, t.el))
      const size = px((ht ? ht.c : cs(h)).fontSize)
      const name = collapse(rendered(h))
      if (prev && lvl > prev + 1) add('TYPE-47', 'error', h, name, `h${lvl} after h${prev}: a level is skipped`)
      const bigger = earlier.find((x) => x.lvl < lvl && size > x.size + 0.5)
      if (bigger) add('TYPE-47', 'error', h, name, `h${lvl} at ${r1(size)}px is larger than an earlier h${bigger.lvl} at ${r1(bigger.size)}px`)
      earlier.push({ lvl, size })
      prev = lvl
    }
  }

  // ---- writing --------------------------------------------------------------------------------------------------
  for (const d of all.filter((e) => e.getAttribute('role') === 'dialog' && visible(e))) {
    const nm = nameOf(d)
    const fails = titleCase(nm)
    if (nm && fails.length) add('WRITE-01', 'error', d, nm, `dialog title not in title case: ${fails.map((w) => `“${w}”`).join(', ')}`)
  }
  for (const list of all.filter((e) => e.localName === 'ui-list' && e.getAttribute('variant') === 'sidebar' && visible(e))) {
    for (const row of descendants(list).filter((d) => d.localName === 'ui-row' && visible(d))) {
      const slotted = Array.from(row.children).filter((c) => !c.getAttribute('slot'))
      const first = slotted.length ? T.find((t) => slotted.some((c) => contains(c, t.el))) : undefined
      const title = slotted.length ? first?.text ?? '' : visibleText(row)
      const fails = titleCase(title)
      if (title && fails.length) add('WRITE-01', 'error', row, title, `sidebar title not in title case: ${fails.map((w) => `“${w}”`).join(', ')}`)
    }
  }
  for (const i of I) {
    const role = roleOf(i)
    const vt = visibleText(i)
    const isButton = role === 'button' || i.localName === 'ui-button'
    if (vt && (isButton || /^(menuitem|menuitemcheckbox|menuitemradio|tab)$/.test(role))) {
      const fails = titleCase(vt)
      if (fails.length) add('WRITE-02', 'error', i, vt, `not in title case: ${fails.map((w) => `“${w}”`).join(', ')}`)
    }
    if (vt && isButton && vt.split(/\s+/).length > 3) add('WRITE-12', 'warn', i, vt, `${vt.split(/\s+/).length} words on a button: a verb and its object`)
    if (isButton && /^(Yes|No)$/.test(nameOf(i))) add('WRITE-13', 'error', i, nameOf(i), `a button named “${nameOf(i)}”; name what it does`)
    if (/^(button|menuitem|menuitemcheckbox|menuitemradio|tab|radio)$/.test(role) || i.localName === 'ui-button') {
      const nm = nameOf(i)
      if (/[.:!]$/.test(nm) && !/…$/.test(nm)) add('WRITE-18', 'error', i, nm, `name ends in “${nm.slice(-1)}”; labels have no ending punctuation`)
    }
    if (i.matches('a[href]') && /^(click here|here|this|link|more)$/i.test(nameOf(i))) add('WRITE-11', 'error', i, nameOf(i), `link named “${nameOf(i)}”; say where it goes`)
  }
  for (const h of all.filter((e) => (/^h[1-6]$/.test(e.localName) || e.getAttribute('role') === 'heading') && visible(e))) {
    const nm = nameOf(h)
    if (/[.:!]$/.test(nm) && !/…$/.test(nm)) add('WRITE-18', 'error', h, nm, `heading ends in “${nm.slice(-1)}”`)
  }
  for (const row of all.filter((e) => e.localName === 'ui-row' && visible(e))) {
    const heading = (row as unknown as { heading?: string }).heading ?? ''
    if (/[.:!]$/.test(heading.trim()) && !/…$/.test(heading.trim())) add('WRITE-18', 'error', row, heading, `row heading ends in “${heading.trim().slice(-1)}”`)
    const trailing = Array.from(row.children).filter((c) => c.getAttribute('slot') === 'trailing')
    const hasTrailingI = trailing.some((tr) => I.some((i) => contains(tr, i)))
    if (((row as unknown as { value?: string }).value || row.hasAttribute('chevron') || hasTrailingI) && heading) {
      const fails = titleCase(heading)
      if (fails.length) add('WRITE-03', 'warn', row, heading, `row title not in title case: ${fails.map((w) => `“${w}”`).join(', ')} (a person's own content may be)`)
    }
  }
  // WRITE-13, WRITE-14: alerts
  for (const a of all.filter((e) => e.localName === 'ui-alert')) {
    const ua = a as unknown as { actionLabel?: string; cancelLabel?: string; open?: boolean }
    const state = ua.open ? '' : ' (closed)'
    if ((ua.actionLabel ?? 'OK').trim() === 'OK') add('WRITE-13', 'error', a, 'OK', `ui-alert action-label “OK”${state}; name the action with a verb`)
    if ((ua.cancelLabel ?? 'Cancel').trim() !== 'Cancel') add('WRITE-14', 'error', a, ua.cancelLabel ?? '', `cancel label “${ua.cancelLabel}”${state}; expected “Cancel”`)
  }
  for (const al of alerts) {
    const buttons = I.filter((i) => contains(al, i) && (roleOf(i) === 'button' || i.localName === 'ui-button'))
    if (buttons.length >= 2 && buttons.some((b) => nameOf(b) === 'OK')) add('WRITE-13', 'error', al, nameOf(al), `an alert with ${buttons.length} buttons, one “OK”; name the action`)
    for (const b of buttons) {
      const isCancel = b.classList.contains('cancel') || /-action-cancel$/.test(b.getAttribute('data-testid') ?? '')
      if (isCancel && nameOf(b) !== 'Cancel') add('WRITE-14', 'error', b, nameOf(b), `the cancel action is “${nameOf(b)}”; expected “Cancel”`)
    }
    const nm = nameOf(al)
    if (/^(error|warning|alert|confirm|are you sure\??|attention)$/i.test(nm)) add('WRITE-29', 'error', al, nm, `alert title “${nm}”; say the situation`)
  }
  for (const sh of sheets) {
    const bar = bars.find((b) => contains(sh, b))
    if (!bar) continue
    const items = barItems(bar).sort((a, b) => hitBox(a).left - hitBox(b).left)
    const lead = items.find((i) => roleOf(i) === 'button' || i.localName === 'ui-button')
    if (lead) {
      const nm = nameOf(lead)
      const back = /^(Back|‹)/.test(nm) || descendants(lead).some((d) => d.localName === 'ui-icon' && /chevron\.left/.test(d.getAttribute('name') ?? ''))
      if (!back && nm !== 'Cancel' && /^(cancel|close|dismiss)$/i.test(nm)) add('WRITE-14', 'error', lead, nm, `the sheet's leading bar button is “${nm}”; expected “Cancel”`)
    }
    // MODAL-16
    const names = items.map((i) => nameOf(i))
    const hasConfirm = names.some((n) => /^(Done|Save|Add)$/.test(n))
    const hasCancel = names.includes('Cancel')
    const hasBack = names.some((n) => n === 'Back' || n.startsWith('‹'))
    if (hasConfirm && !hasCancel && !hasBack) add('MODAL-16', 'error', bar, names.join(', '), `a sheet bar with “${names.find((n) => /^(Done|Save|Add)$/.test(n))}” and no Cancel or Back`)
    if (hasCancel && hasBack && names.includes('Done')) add('MODAL-16', 'error', bar, names.join(', '), 'a sheet bar with Cancel, Back and Done together')
  }
  // interface text
  for (const t of interfaceText) {
    const x = t.text
    const raw = t.nodes.map((n) => n.textContent ?? '').join('')
    const m8 = x.match(/\b(oops|uh-?oh|whoops|yikes|yay|woohoo)\b/i) ?? x.match(/\p{Extended_Pictographic}/u)
    if (m8) add('WRITE-08', 'error', t.el, x, `“${m8[0]}”: neutral and calm, no cute words or emoji`)
    for (const m of x.matchAll(/\b(we|we're|we’re|we've|we’ve|our|us)\b/gi)) {
      const before = x[m.index! - 1] ?? ''
      const after = x[m.index! + m[0].length] ?? ''
      if (/[-_./\d]/.test(before) || /[-_./\d]/.test(after)) continue // an identifier (us-east-1)
      add('WRITE-09', 'error', t.el, x, `“${m[0]}”: the interface does not speak as “we”`)
      break
    }
    const m10 = x.match(/\b(click|tap|right-click|double-click)(s|ed|ing)?\b/i)
    if (m10) add('WRITE-10', 'error', t.el, x, `“${m10[0]}”: use a device-neutral verb (choose, select, open)`)
    const m19 = raw.match(/\.\.\.|"|'/)
    if (m19) add('WRITE-19', 'error', t.el, x, `“${m19[0]}”: use typographic punctuation (… “ ” ’)`)
    if (/\(s\)/.test(x)) add('WRITE-21', 'error', t.el, x, '“(s)”: write the number and the right form')
    for (const m of x.matchAll(/(?<![\d.,])\d{5,}(?![\d.,])/g)) {
      const token = x.slice(0, m.index).split(/\s/).pop()! + x.slice(m.index).split(/\s/)[0]
      if (/[A-Za-z_-]\d|\d[A-Za-z_-]/.test(token)) continue
      add('WRITE-21', 'warn', t.el, x, `“${m[0]}”: an ungrouped number; is it formatted for the locale (12,345)?`)
      break
    }
    const m22 = x.match(/\d(KB|MB|GB|TB|ms)\b/) ?? x.match(/\d\s%/)
    if (m22) add('WRITE-22', 'error', t.el, x, `“${m22[0]}”: a space before units, none before %`)
    const m23 = x.match(/\d{4}-\d{2}-\d{2}T\d{2}:/) ?? x.match(/\b\d{10}(\d{3})?\b/)
    if (m23) add('WRITE-23', 'error', t.el, x, `“${m23[0]}”: a date as a person says it, not a timestamp`)
    const m26 = x.match(/\b(he or she|his or her|guys|whitelist|blacklist|master|slave)\b/i)
    if (m26) add('WRITE-26', 'error', t.el, x, `“${m26[0]}”: use inclusive language`)
    const m27 = x.match(/\b(undefined|null|NaN|Exception|Traceback)\b|\[object Object\]|\bE[A-Z]{4,}\b|-32\d{3}\b|^Error:/m)
    if (m27) add('WRITE-27', 'error', t.el, x, `“${m27[0]}”: developer text in the interface`)
  }
  for (const e of all.filter((x) => x.matches('ui-section[footer-tone=error], [role=alert], ui-banner[tone=error]') && visible(x))) {
    const txt = e.localName === 'ui-section' ? collapse(e.shadowRoot?.querySelector('.footer')?.textContent ?? '') : collapse(rendered(e))
    const m = txt.match(/\b(invalid|illegal|wrong|bad|you failed|not allowed)\b/i)
    if (m) add('WRITE-25', 'warn', e, txt, `“${m[0]}”: does the error blame the person?`)
  }
  colourChecks()

  // COLOR-18: colour as the only signal
  for (const el of all) {
    if (!visible(el)) continue
    const r = el.getBoundingClientRect()
    if (r.width > 16 || r.height > 16) continue
    const bg = rgba(cs(el).backgroundColor)
    if (bg[3] === 0 || !is(bg, el, SYSTEM)) continue
    if (visibleText(el) || el.getAttribute('aria-label') || el.getAttribute('title') || (el.getAttribute('role') === 'img' && nameOf(el))) continue
    const p = parentOf(el)
    const siblingText = p ? Array.from(p instanceof HTMLSlotElement ? p.assignedElements() : p.children).some((sb) => sb !== el && visibleText(sb)) || Array.from(p.childNodes).some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim()) : false
    if (!siblingText) add('COLOR-18', 'error', el, '', `a ${Math.round(r.width)}×${Math.round(r.height)} ${tokenNames(bg, el).find((n) => SYSTEM.includes(n))} mark with no text, name or label beside it: colour is the only signal`)
  }
  // COLOR-10, FORM-02: what a list sits on
  for (const list of all.filter((e) => e.localName === 'ui-list' && visible(e))) {
    const v = list.getAttribute('variant')
    const under = opaqueAncestor(list)
    if (!under) continue
    const grouped = is(under, list, ['--bg-grouped', '--bg-grouped-elevated'])
    if ((!v || v === 'insetGrouped' || v === 'grouped') && !grouped) add('COLOR-10', 'error', list, '', `a ${v ?? 'insetGrouped'} list on ${hex(under)}${tokenNames(under, list)[0] ? ` (${tokenNames(under, list)[0]})` : ''}; expected --bg-grouped or --bg-grouped-elevated`)
    if (v === 'inset' && grouped) add('FORM-02', 'error', list, '', `an inset list on the grouped background ${hex(under)}; an inset list sits on the plain background`)
  }
  // ---- layout -----------------------------------------------------------------------------------------------
  const vp = contentRoot.getBoundingClientRect()
  if (on('LAYOUT-01')) {
    for (const t of T) {
      if (inside(t.el, 'ui-row') || !contains(contentRoot, t.el)) continue
      if (layerOf(t.el) && inside(t.el, 'dialog, [role=dialog], [role=alertdialog], [role=menu], [popover]')) continue
      const q = local(t).find((x) => x.left < vp.left + 16 - 0.5 || x.right > vp.right - 16 + 0.5)
      if (q) add('LAYOUT-01', 'error', t.el, t.text, `text ${q.left < vp.left + 16 - 0.5 ? `${Math.round(q.left - vp.left)}px from the leading` : `${Math.round(vp.right - q.right)}px from the trailing`} edge of the window; keep 16px`, q)
    }
  }
  for (const el of all) {
    if (!visible(el) || !isA(el)) continue
    const c = cs(el)
    const r = el.getBoundingClientRect()
    // LAYOUT-02: the 4-point grid (computed values: an auto margin is not a length)
    if (o.surface === 'app' && !el.localName.startsWith('ui-') && on('LAYOUT-02')) {
      let map: StylePropertyMapReadOnly | null = null
      try {
        map = el.computedStyleMap()
      } catch {
        map = null
      }
      for (const p of ['margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left', 'row-gap', 'column-gap']) {
        const v = map?.get(p)
        if (!v || !(v instanceof CSSUnitValue) || v.unit !== 'px') continue
        const a = Math.abs(v.value)
        if ([0, 0.5, 1, 2].some((x) => Math.abs(a - x) < 0.01)) continue
        if (Math.abs(a - Math.round(a / 4) * 4) <= 0.5) continue
        add('LAYOUT-02', 'error', el, '', `${p} ${r1(v.value)}px; expected 0, 1, 2 or a multiple of 4`)
        break
      }
    }
    // LAYOUT-14: radii
    const corners = [c.borderTopLeftRadius, c.borderTopRightRadius, c.borderBottomRightRadius, c.borderBottomLeftRadius]
    if (corners.some((x) => px(x) > 0)) {
      for (const corner of corners) {
        const first = corner.split(/\s+/)[0]
        if (first.endsWith('%')) {
          if (!['50%', '22.37%', '0%'].includes(first)) {
            add('LAYOUT-14', 'error', el, '', `radius ${first}; expected a radius token, 50% or 22.37%`)
            break
          }
          continue
        }
        const v = px(first)
        if (v === 0) continue
        if ([6, 7, 8, 10, 12, 14, 18, 22, 30].some((x) => Math.abs(v - x) <= 0.5) || v >= Math.min(r.width, r.height) / 2 - 0.5) continue
        add('LAYOUT-14', 'error', el, '', `radius ${r1(v)}px; expected 6, 7, 8, 10, 12, 14, 18, 22, 30 or a capsule`)
        break
      }
      // LAYOUT-15: concentric corners
      const P = (() => {
        for (let at = parentOf(el); at; at = parentOf(at)) if ([cs(at).borderTopLeftRadius, cs(at).borderTopRightRadius, cs(at).borderBottomRightRadius, cs(at).borderBottomLeftRadius].some((x) => px(x) > 0)) return scaffold.has(at) && !(o.app && (at === contentRoot || at === appWindow)) ? null : at
        return null
      })()
      if (P && visible(P)) {
        const pr = P.getBoundingClientRect()
        if (r.left >= pr.left - 0.5 && r.right <= pr.right + 0.5 && r.top >= pr.top - 0.5 && r.bottom <= pr.bottom + 0.5) {
          const pc = cs(P)
          const rad = (v: string, b: DOMRect): number => (v.endsWith('%') ? (px(v) / 100) * Math.min(b.width, b.height) : Math.min(px(v), Math.min(b.width, b.height) / 2))
          const gaps = { tl: [r.left - pr.left, r.top - pr.top], tr: [pr.right - r.right, r.top - pr.top], br: [pr.right - r.right, pr.bottom - r.bottom], bl: [r.left - pr.left, pr.bottom - r.bottom] }
          const pairs: [string, string, number[]][] = [
            [c.borderTopLeftRadius, pc.borderTopLeftRadius, gaps.tl],
            [c.borderTopRightRadius, pc.borderTopRightRadius, gaps.tr],
            [c.borderBottomRightRadius, pc.borderBottomRightRadius, gaps.br],
            [c.borderBottomLeftRadius, pc.borderBottomLeftRadius, gaps.bl]
          ]
          for (const [a, b, g] of pairs) {
            const rr = rad(a.split(/\s+/)[0], r)
            const RR = rad(b.split(/\s+/)[0], pr)
            if (rr <= 0 || RR <= 0 || Math.max(...g) >= RR || rr >= Math.min(r.width, r.height) / 2 - 0.5) continue // a capsule or circle has no corner to match
            const gmin = Math.max(0, Math.min(...g))
            if (rr > RR - gmin + 1) {
              add('LAYOUT-15', 'error', el, '', `radius ${r1(rr)}px, ${r1(gmin)}px inside a ${r1(RR)}px corner of ${describe(P)}; concentric is ${r1(Math.max(0, RR - gmin))}px`)
              break
            }
          }
        }
      }
    }
    // LAYOUT-16: shadows
    if (c.boxShadow !== 'none' && !shadowValues.includes(c.boxShadow) && !c.boxShadow.includes('inset')) {
      const parts = c.boxShadow.split(/,(?![^(]*\))/)
      let hairline = false
      if (parts.length === 1) {
        const col = parts[0].match(/(rgba?|color|oklab|oklch|lab|lch|hsla?)\([^)]*\)|#[0-9a-f]+/i)?.[0] ?? ''
        const lens = parts[0].replace(col, '').trim().split(/\s+/).map(px)
        hairline = (lens[2] ?? 0) === 0 && (lens[3] ?? 0) === 0 && is(rgba(col), el, ['--separator', '--separator-opaque'])
      }
      if (!hairline) add('LAYOUT-16', 'error', el, '', `box-shadow ${c.boxShadow.slice(0, 90)}; expected a --shadow-* token or a separator hairline`)
    }
    // LAYOUT-18, LAYOUT-19: images
    if ((el instanceof HTMLImageElement && el.complete && el.naturalWidth > 0) || (el instanceof HTMLVideoElement && el.videoWidth > 0)) {
      const nw = el instanceof HTMLImageElement ? el.naturalWidth : el.videoWidth
      const nh = el instanceof HTMLImageElement ? el.naturalHeight : el.videoHeight
      const w = (el as HTMLElement).clientWidth - px(c.paddingLeft) - px(c.paddingRight)
      const h = (el as HTMLElement).clientHeight - px(c.paddingTop) - px(c.paddingBottom)
      if (c.objectFit === 'fill' && h > 0 && nh > 0 && Math.abs(w / h / (nw / nh) - 1) > 0.01) add('LAYOUT-18', 'error', el, '', `drawn ${Math.round(w)}×${Math.round(h)} from ${nw}×${nh} with object-fit fill: the image is stretched`)
      if (el instanceof HTMLImageElement && !/\.svg(\?|#|$)/i.test(el.currentSrc) && !el.currentSrc.startsWith('data:image/svg+xml')) {
        const dpr = Math.max(devicePixelRatio, 2)
        if (el.naturalWidth < 0.9 * r.width * dpr) add('LAYOUT-19', 'error', el, el.alt, `${el.naturalWidth}px wide drawn at ${Math.round(r.width)}px; expected at least ${Math.round(0.9 * r.width * dpr)}px for a @${dpr}x screen`)
      }
    }
  }
  // LAYOUT-03 rows, LAYOUT-04 hand-made row inset
  const ROW_ROLES = /^(listitem|option|row|menuitem|treeitem)$/
  const rowsL03 = new Set(all.filter((e) => visible(e) && !kitInternal(e) && (e.localName === 'ui-row' || ROW_ROLES.test(e.getAttribute('role') ?? ''))))
  for (const row of rowsL03) {
    const h = row.getBoundingClientRect().height
    if (h < 43.5) add('LAYOUT-03', 'error', row, row.localName === 'ui-row' ? visibleText(row) : collapse(rendered(row)).slice(0, 80), `row ${r1(h)}px high; expected at least 44px`)
  }
  for (const row of all.filter((e) => visible(e) && isA(e) && /^(listitem|option|row)$/.test(e.getAttribute('role') ?? '') && !inside(e, 'ui-row'))) {
    const rr = row.getBoundingClientRect()
    let leftmost = Infinity
    for (const t of T) if (contains(row, t.el)) for (const q of t.rects) leftmost = Math.min(leftmost, q.left)
    for (const d of descendants(row)) if (d.matches('svg, img, ui-icon, input, canvas') && visible(d)) leftmost = Math.min(leftmost, d.getBoundingClientRect().left)
    if (leftmost === Infinity) continue
    const inset = leftmost - rr.left
    if (inset < 15.5 || inset > 16.5) add('LAYOUT-04', 'error', row, collapse(rendered(row)), `content starts ${r1(inset)}px into the row; expected 16px`)
  }
  // LAYOUT-06 (review): heading over a list
  for (const h of all.filter((e) => /^h[2-4]$/.test(e.localName) && isA(e) && visible(e))) {
    const next = h.nextElementSibling
    if (!next || !next.matches('[role=list], ul, ol')) continue
    const ht = T.find((t) => contains(h, t.el))
    const it = T.find((t) => contains(next, t.el))
    if (!ht || !it) continue
    const d = Math.abs(ht.rects[0].left - it.rects[0].left)
    if (d > 1) add('LAYOUT-06', 'warn', h, ht.text, `heading text starts ${Math.round(d)}px off its list's first item: do they align?`)
  }
  // LAYOUT-08: under window chrome
  {
    const chrome = all.filter((e) => e.matches('[data-testid$="-multitasking"], [data-testid$="-resize"]') && visible(e))
    for (const ch of chrome) {
      const cr = ch.getBoundingClientRect()
      const hit = (q: DOMRect): boolean => Math.min(q.right, cr.right) - Math.max(q.left, cr.left) > 1 && Math.min(q.bottom, cr.bottom) - Math.max(q.top, cr.top) > 1
      for (const i of I) if (!contains(ch, i) && !contains(i, ch) && seenHit(i) && hit(seenHit(i)!)) add('LAYOUT-08', 'error', i, nameOf(i), `under the window chrome ${describe(ch)}`)
      for (const t of T) if (!contains(ch, t.el) && seen(t).some(hit)) add('LAYOUT-08', 'error', t.el, t.text, `text under the window chrome ${describe(ch)}`)
    }
  }
  // LAYOUT-17: no box inside a box
  if (on('LAYOUT-17')) {
    const isBox = (el: Element): boolean => {
      if (!visible(el) || !isA(el)) return false
      const r = el.getBoundingClientRect()
      if (r.width < 64 || r.height < 32) return false
      const c = cs(el)
      if (![c.borderTopLeftRadius, c.borderTopRightRadius, c.borderBottomRightRadius, c.borderBottomLeftRadius].some((x) => px(x) > 0)) return false
      const border = ['top', 'right', 'bottom', 'left'].some((sd) => px(c.getPropertyValue(`border-${sd}-width`)) > 0 && c.getPropertyValue(`border-${sd}-style`) !== 'none' && rgba(c.getPropertyValue(`border-${sd}-color`))[3] > 0)
      if (border) return true
      const b = rgba(c.backgroundColor)
      if (b[3] === 0) return false
      const p = parentOf(el)
      return !p || !same(over(b, backdrop(p).color), backdrop(p).color)
    }
    const exempt = (el: Element): boolean => el === contentRoot || scaffold.has(el) || modals.includes(el) || popovers.includes(el) || menus.includes(el) || el.localName === 'ui-section' || el.getAttribute('role') === 'menu'
    for (const el of all) {
      if (!isBox(el)) continue
      let anc: Element | null = null
      for (let at = parentOf(el); at; at = parentOf(at)) {
        if (exempt(at)) break
        if (isBox(at)) {
          anc = at
          break
        }
      }
      if (anc) add('LAYOUT-17', 'error', el, '', `a bordered or filled box inside the box ${describe(anc)}; group with spacing and sections`)
    }
  }

  // ---- navigation --------------------------------------------------------------------------------------------
  for (const sv of all.filter((e) => e.localName === 'ui-split-view' && !e.hasAttribute('collapsed') && visible(e))) {
    const kids = Array.from(sv.children)
    const detail = kids.filter((k) => !k.getAttribute('slot'))
    const shows = detail.some((d) => visible(d) && d.localName !== 'ui-empty-state' && !(d.querySelector('ui-empty-state') && !T.some((t) => contains(d, t.el) && !inside(t.el, 'ui-empty-state'))))
    if (!shows) continue
    for (const slot of ['sidebar', 'content']) {
      const col = kids.find((k) => k.getAttribute('slot') === slot)
      const list = col ? (col.localName === 'ui-list' ? col : col.querySelector('ui-list')) : null
      if (!list || !visible(list)) continue
      if (!list.querySelector('ui-row[selected]')) add('NAV-02', 'error', list, '', `the ${slot} list has no selected row while the detail shows content; the path stays highlighted`)
    }
  }
  if (on('NAV-06')) {
    const cr = contentRoot.getBoundingClientRect()
    for (const i of I) {
      if (!(roleOf(i) === 'button' || i.localName === 'ui-button')) continue
      if (bars.some((b) => contains(b, i)) || inside(i, 'dialog, [role=dialog], [role=alertdialog], [role=menu], [popover]') || popovers.some((p) => contains(p, i))) continue
      let floating = false
      for (let at: Element | null = i; at && at !== contentRoot; at = parentOf(at)) if (/^(fixed|absolute|sticky)$/.test(cs(at).position)) floating = true
      if (!floating) continue
      const h = hitBox(i)
      const drawn = i.localName === 'ui-button' ? i.shadowRoot?.querySelector('button') ?? i : i
      const dr2 = drawn.getBoundingClientRect()
      const rad = px(cs(drawn).borderTopLeftRadius.split(/\s+/)[0]) * (cs(drawn).borderTopLeftRadius.endsWith('%') ? Math.min(dr2.width, dr2.height) / 100 : 1)
      if ((cr.bottom - h.bottom <= 48 || cr.right - h.right <= 48) && rad >= Math.min(dr2.width, dr2.height) / 2 - 0.5) add('NAV-06', 'error', i, nameOf(i), 'a round floating button by the window corner; actions go in the bar')
    }
  }

  // ---- controls ----------------------------------------------------------------------------------------------
  if (o.surface === 'app') {
    for (const el of all.filter((e) => e.matches('button, [role=button], a[href]') && !kitInternal(e) && isA(e) && visible(e))) {
      const c = cs(el)
      const bg = rgba(c.backgroundColor)
      const border = ['top', 'right', 'bottom', 'left'].some((sd) => px(c.getPropertyValue(`border-${sd}-width`)) > 0 && c.getPropertyValue(`border-${sd}-style`) !== 'none' && rgba(c.getPropertyValue(`border-${sd}-color`))[3] > 0)
      const radius = [c.borderTopLeftRadius, c.borderTopRightRadius, c.borderBottomRightRadius, c.borderBottomLeftRadius].some((x) => px(x) > 0)
      if (bg[3] > 0 || border || radius) add('CTRL-01', 'error', el, visibleText(el) || nameOf(el), `a hand-made ${el.localName} with ${[bg[3] > 0 ? `background ${hex(bg)}` : '', border ? 'a border' : '', radius ? 'a radius' : ''].filter(Boolean).join(', ')}; use ui-button`)
    }
  }
  // CTRL-02: one prominent button per view (and per modal dialog)
  {
    const scopes: Element[] = [...modals]
    if (o.app) scopes.unshift(contentRoot)
    for (const sc of scopes) {
      const filled = all.filter((e) => e.localName === 'ui-button' && visible(e) && contains(sc, e) && !(sc === contentRoot && modals.some((m) => contains(m, e))) && !['tinted', 'gray', 'plain'].includes(e.getAttribute('variant') ?? ''))
      if (filled.length > 1) add('CTRL-02', 'error', filled[1], filled.map((f) => visibleText(f)).join(', '), `${filled.length} filled buttons in ${sc === contentRoot ? 'the view' : 'one dialog'} (${filled.slice(0, 6).map((f) => `“${visibleText(f)}”`).join(', ')}${filled.length > 6 ? ', …' : ''}); one is prominent, the rest tinted, gray or plain`)
    }
  }
  // CTRL-03: sizes within a set
  if (on('CTRL-03')) {
    const byParent = new Map<Element, Element[]>()
    for (const b of all.filter((e) => e.localName === 'ui-button' && visible(e))) {
      const p = parentOf(b)
      if (p) byParent.set(p, [...(byParent.get(p) ?? []), b])
    }
    for (const group of byParent.values()) {
      if (group.length < 2) continue
      const cy = (e: Element): number => (e.getBoundingClientRect().top + e.getBoundingClientRect().bottom) / 2
      // the size a person sees: the label's size, and the drawn height of the buttons that have a fill
      const inner = (b: Element): Element => b.shadowRoot?.querySelector('button') ?? b
      for (let a = 0; a < group.length; a++) {
        const row = group.filter((b) => Math.abs(cy(b) - cy(group[a])) <= 4)
        const fonts = new Set(row.map((b) => r1(px(cs(inner(b)).fontSize))))
        const heights = new Set(row.filter((b) => (b.getAttribute('variant') ?? 'filled') !== 'plain').map((b) => Math.round(inner(b).getBoundingClientRect().height)))
        if (row.length >= 2 && (fonts.size > 1 || heights.size > 1)) {
          const sizes = [...fonts].map((f) => `${f}px`).concat([...heights].map((h) => `${h}px high`))
          add('CTRL-03', 'error', group[a], row.map(visibleText).join(', '), `buttons side by side drawn at ${[...sizes].join(' and ')} (size ${[...new Set(row.map((b) => b.getAttribute('size') ?? 'medium'))].join(', ')}); rank them by style, not size`)
          break
        }
      }
    }
  }
  /** What had the focus 300 ms after the view opened, unless a play function has operated the page since. */
  const openingFocus = ((): Element | null => {
    const f = dr.focusAtOpen
    if (!f || !f.isConnected) return null
    if (!o.played) return f
    return modals.some((m) => contains(m, f)) ? f : null
  })()
  for (const b of all.filter((e) => e.localName === 'ui-button' && e.hasAttribute('destructive') && visible(e))) {
    const v = b.getAttribute('variant') ?? 'filled'
    if (v === 'filled') add('CTRL-04', 'error', b, visibleText(b), 'a destructive button in the filled style; never the prominent one')
    const f = openingFocus
    if (f && (f === b || contains(b, f))) add('CTRL-04', 'error', b, visibleText(b), 'the destructive button has the focus 300 ms after the view opened')
  }
  if (on('CTRL-07')) {
    for (const sw of all.filter((e) => (e.localName === 'ui-toggle' || (e.getAttribute('role') === 'switch' && !kitInternal(e))) && visible(e))) {
      if (!inside(sw, 'ui-row, [role=listitem]')) add('CTRL-07', 'error', sw, nameOf(sw), 'a switch outside a list row')
    }
  }
  for (const row of all.filter((e) => e.localName === 'ui-row' && e.querySelector('ui-toggle') && visible(e))) {
    const value = ((row as unknown as { value?: string }).value ?? '').trim()
    const trailingText = Array.from(row.children).filter((c) => c.getAttribute('slot') === 'trailing' && c.localName !== 'ui-toggle').map(visibleText).join(' ').trim()
    const hit = [value, trailingText].find((x) => /^(On|Off|Enabled|Disabled)$/i.test(x))
    if (hit) add('CTRL-08', 'error', row, hit, `“${hit}” beside a switch; the switch says it`)
  }
  for (const el of all.filter((e) => e.matches('input[type=checkbox], input[type=radio]') && !kitInternal(e) && visible(e))) add('CTRL-11', 'error', el, nameOf(el), `a native ${(el as HTMLInputElement).type}; use ui-check, ui-toggle or a list with selection`)
  for (const g of all.filter((e) => e.localName === 'ui-segmented' || (e.getAttribute('role') === 'radiogroup' && !kitInternal(e)))) {
    if (!visible(g)) continue
    const n = descendants(g).filter((d) => d.getAttribute('role') === 'radio' || d.matches('input[type=radio]')).length
    if (n < 2 || n > 5) add('CTRL-12', 'error', g, '', `${n} segments; expected two to five`)
  }
  for (const el of all.filter((e) => e.localName === 'select' && !kitInternal(e) && visible(e))) add('CTRL-15', 'error', el, nameOf(el), 'a native select; use ui-picker')
  for (const p of all.filter((e) => e.localName === 'ui-picker' && !e.hasAttribute('required') && visible(e))) {
    const sel = p.shadowRoot?.querySelector('select')
    if (sel && sel.selectedIndex === -1) add('CTRL-16', 'error', p, nameOf(p), `value “${(p as unknown as { value?: string }).value ?? ''}” matches no option: the picker shows no choice`)
  }
  for (const el of all.filter((e) => (e.matches('ui-text-field[readonly], ui-text-area[readonly]') || (e.matches('input[readonly], textarea[readonly]') && !kitInternal(e))) && visible(e))) add('CTRL-23', 'error', el, nameOf(el), 'a read-only field; read-only text is a label (or a value in a row)')
  for (const el of all.filter((e) => isA(e) && visible(e))) {
    if (el.localName === 'a' && !el.hasAttribute('href')) add('CTRL-24', 'error', el, collapse(rendered(el)), 'an a without href; a link goes somewhere, a button does something')
    else if (el.getAttribute('role') === 'link' && !el.matches('a[href]')) add('CTRL-24', 'error', el, collapse(rendered(el)), 'role=link on something that is not a link')
  }
  for (const m of all.filter((e) => e.localName === 'ui-meter')) {
    const um = m as unknown as { value?: number; label?: string }
    if (typeof um.value === 'number' && Number.isFinite(um.value) && !(um.label ?? '').trim()) add('CTRL-25', 'error', m, '', 'a meter with a value and no label')
  }
  // CTRL-26: glyphs from the icon set
  {
    const EP_ONLY = /^[\p{Extended_Pictographic}\u{FE0F}\u{200D}\s]+$/u
    const glyphOnly = (scope: Element[]): string => {
      const text = collapse(scope.map((e) => (isK(e) ? visibleText(e) : rendered(e))).join(' '))
      if (text) return EP_ONLY.test(text) && /\p{Extended_Pictographic}/u.test(text) ? `the emoji “${text}”` : ''
      const content = scope.flatMap((e) => [e, ...descendants(e)]).filter((d) => visible(d) && (d.localName === 'img' || d.localName === 'svg' || d.localName === 'ui-icon'))
      if (content.some((d) => d.localName === 'ui-icon' || d.matches('svg.ios-icon'))) return '' // the kit's and the shell's icon set
      const top = content.filter((d) => !content.some((x) => x !== d && contains(x, d)))
      if (top.length === 1 && top[0].localName === 'img') return 'an img'
      if (top.length === 1 && top[0].localName === 'svg' && !inside(top[0], 'ui-icon')) return 'an svg outside ui-icon'
      return ''
    }
    for (const i of I) {
      if (kitInternal(i) && !isK(i)) continue
      const g = glyphOnly(isK(i) ? Array.from(i.children).filter((c) => !c.getAttribute('slot') || c.getAttribute('slot') === 'icon') : Array.from(i.children))
      if (g) add('CTRL-26', 'error', i, nameOf(i), `the control's only content is ${g}; glyphs come from ui-icon`)
    }
    for (const row of all.filter((e) => e.localName === 'ui-row' && visible(e))) {
      const lead = Array.from(row.children).filter((c) => c.getAttribute('slot') === 'leading')
      if (!lead.length || lead.some((c) => c.localName.startsWith('ui-') && c.localName !== 'ui-icon')) continue // a kit control, not a glyph
      const g = glyphOnly(lead)
      if (g) add('CTRL-26', 'error', lead[0], visibleText(row), `the row's leading content is ${g}; glyphs come from ui-icon`)
    }
  }
  // CTRL-28: rows of one list
  for (const list of all.filter((e) => e.localName === 'ui-list' && visible(e))) {
    const groups = new Map<string, Element[]>()
    for (const row of descendants(list).filter((d) => d.localName === 'ui-row')) {
      for (const ic of Array.from(row.children).filter((c) => c.localName === 'ui-icon' && visible(c))) {
        const k = ic.getAttribute('slot') ?? ''
        groups.set(k, [...(groups.get(k) ?? []), ic])
      }
    }
    for (const [slot, icons] of groups) {
      const sizes = new Set(icons.map((e) => `${(e as unknown as { size: number }).size}/${(e as unknown as { weight: number }).weight}`))
      if (icons.length > 1 && sizes.size > 1) add('CTRL-28', 'error', list, '', `glyphs in the rows' ${slot || 'default'} slot at ${[...sizes].join(', ')} (size/weight); one size and weight per list`)
    }
  }

  // ---- forms -------------------------------------------------------------------------------------------------
  for (const f of all.filter((e) => e.localName === 'ui-text-field' && visible(e))) {
    const row = inside(parentOf(f), 'ui-row')
    if (!row || !(row as unknown as { heading?: string }).heading) continue
    const title = row.shadowRoot?.querySelector('.title')
    if (title && visible(title) && f.getBoundingClientRect().top > title.getBoundingClientRect().bottom + 0.5) add('FORM-05', 'error', f, nameOf(f), "the field sits under the row's label; the label leads, the value trails on one line")
  }
  {
    const fields = all.filter((e) => visible(e) && (e.localName === 'ui-text-field' || (e.localName === 'input' && !kitInternal(e) && !/^(hidden|checkbox|radio|range|button|submit|reset|image|color|file)$/.test((e as HTMLInputElement).type))))
    for (const f of fields) {
      const inner = f.localName === 'ui-text-field' ? (innerTarget(f) as HTMLInputElement | null) : (f as HTMLInputElement)
      if (!inner) continue
      const nm = nameOf(f)
      if (!nm) continue
      const type = inner.type
      const im = inner.getAttribute('inputmode') ?? ''
      if (/e-?mail/i.test(nm) && type !== 'email') add('FORM-09', 'error', f, nm, `type ${type} for “${nm}”; expected email`)
      else if (/\b(url|website|link)\b/i.test(nm) && type !== 'url') add('FORM-09', 'error', f, nm, `type ${type} for “${nm}”; expected url`)
      else if (/\b(port|count|number|pid|ttl|size|replicas|priority)\b/i.test(nm) && type !== 'number' && im !== 'numeric' && im !== 'decimal') add('FORM-09', 'error', f, nm, `type ${type} for “${nm}”; expected number or inputmode numeric`)
      const ac = (inner.getAttribute('autocapitalize') ?? '').toLowerCase()
      if ((type === 'email' || type === 'url' || /\b(host|user ?name|path|domain|token|key|id|image|tag)\b/i.test(nm)) && ac !== 'off' && ac !== 'none') add('FORM-10', 'error', f, nm, `autocapitalize ${ac || 'unset'} on an identifier field; expected off`)
      if (/\b(password|passphrase|secret|token|api key|private key)\b/i.test(nm) && type !== 'password') add('FORM-11', 'error', f, nm, `type ${type} for a secret; expected password`)
      if (type === 'password' && inner.value && run === 'default') add('FORM-11', 'error', f, nm, 'a password field already holds a value when its view opens')
    }
    for (const f of all.filter((e) => e.matches('input[type=password]') && visible(e))) {
      if (!(f.getAttribute('autocomplete') ?? '').trim()) {
        const host = hostOf(f)
        add('FORM-12', 'error', host && isK(host) ? host : f, nameOf(f), 'a password field without autocomplete (current-password or new-password)')
      }
    }
  }
  // FORM-15: disabled until there is something to send
  {
    const REQ = 'input[required], textarea[required], select[required], ui-text-field[required], ui-text-area[required], ui-picker[required]'
    const empty = (f: Element): boolean => {
      const v = (f as unknown as { value?: unknown }).value
      return v === undefined || v === null || String(v).trim() === ''
    }
    const off = (b: Element): boolean => b.hasAttribute('disabled') || (b as unknown as { disabled?: boolean }).disabled === true
    for (const form of all.filter((e) => e.localName === 'form' && visible(e))) {
      const req = Array.from(form.querySelectorAll(REQ)).filter((f) => !kitInternal(f) && empty(f))
      if (!req.length) continue
      const submit = form.querySelector('[type=submit], ui-button[type=submit]')
      if (submit && !off(submit)) add('FORM-15', 'error', submit, visibleText(submit) || nameOf(submit), `the submit button is enabled while ${req.length} required field${req.length > 1 ? 's are' : ' is'} empty`)
    }
    for (const list of all.filter((e) => e.localName === 'ui-list' && visible(e) && !inside(e, 'form'))) {
      const req = Array.from(list.querySelectorAll(REQ)).filter(empty)
      if (!req.length) continue
      let submit: Element | null = null
      for (let sib = list.nextElementSibling; sib; sib = sib.nextElementSibling) {
        const bs = sib.localName === 'ui-button' ? [sib] : Array.from(sib.querySelectorAll('ui-button'))
        if (bs.length) submit = bs[bs.length - 1]
      }
      if (submit && !off(submit)) add('FORM-15', 'error', submit, visibleText(submit), `the commit button is enabled while ${req.length} required field${req.length > 1 ? 's are' : ' is'} empty`)
    }
  }
  const invalidIn = (e: Element): boolean => e.hasAttribute('invalid') || e.getAttribute('aria-invalid') === 'true' || (isK(e) && innerTarget(e)?.getAttribute('aria-invalid') === 'true')
  for (const sec of all.filter((e) => e.localName === 'ui-section' && e.getAttribute('footer-tone') === 'error')) {
    const foot = sec.shadowRoot?.querySelector('.footer')
    if (!foot || !visible(foot)) continue
    if (!descendants(sec).some(invalidIn)) add('FORM-18', 'error', sec, foot.textContent ?? '', 'an error footer with no control marked invalid')
  }
  for (const ctl of all.filter((e) => !kitInternal(e) && invalidIn(e) && visible(e))) {
    const sec = inside(parentOf(ctl), 'ui-section') as (Element & { footer?: string; footerTone?: string }) | null
    if (!sec) continue
    const footer = (sec.footer ?? '').trim()
    if (sec.footerTone !== 'error' || !footer) add('FORM-19', 'error', ctl, nameOf(ctl), `an invalid control in a section whose footer is ${footer ? `tone ${sec.footerTone}` : 'empty'}; the sentence goes in the error footer`)
    const inner = isK(ctl) ? innerTarget(ctl) : ctl
    const desc = (inner?.getAttribute('aria-description') ?? '').trim()
    if (!desc || desc !== footer) add('FORM-21', 'error', ctl, nameOf(ctl), desc ? `aria-description “${clip80(desc)}” differs from the footer “${clip80(footer)}”` : 'the invalid control has no aria-description with the footer\'s sentence')
  }

  // ---- lists -------------------------------------------------------------------------------------------------
  for (const el of all.filter((e) => isA(e) && !kitInternal(e) && visible(e) && !inside(e, 'ui-row, .ios-row'))) {
    const role = el.getAttribute('role') ?? ''
    if (/^(listitem|option|row)$/.test(role)) add('LIST-01', 'error', el, collapse(rendered(el)), `a hand-made list item (role ${role}); use ui-list and ui-row`)
    else if (el.localName === 'li') {
      const c = cs(el)
      const border = ['top', 'right', 'bottom', 'left'].some((sd) => px(c.getPropertyValue(`border-${sd}-width`)) > 0 && c.getPropertyValue(`border-${sd}-style`) !== 'none' && rgba(c.getPropertyValue(`border-${sd}-color`))[3] > 0)
      if (rgba(c.backgroundColor)[3] > 0 || border) add('LIST-01', 'error', el, collapse(rendered(el)), 'an li drawn as a row (background or border); use ui-list and ui-row')
    }
  }
  for (const row of all.filter((e) => e.localName === 'ui-row' && visible(e))) {
    const ur = row as unknown as { heading?: string }
    const trailing = Array.from(row.children).filter((c) => c.getAttribute('slot') === 'trailing')
    const trailingI = I.filter((i) => trailing.some((tr) => contains(tr, i)))
    if (row.hasAttribute('chevron')) {
      const why = !row.hasAttribute('tappable') ? 'not tappable' : row.hasAttribute('action') ? 'an action row' : row.hasAttribute('destructive') ? 'destructive' : trailingI.length ? 'a control at its end' : ''
      if (why) add('LIST-04', 'error', row, ur.heading ?? '', `a chevron on a row that is ${why}; a chevron means it navigates`)
    }
    if (row.hasAttribute('tappable') && /^(Add|New|Create|Restart|Start|Stop|Sign|Log|Connect|Disconnect|Reset|Export|Import|Run|Check|Install)\b/.test(ur.heading ?? '') && !row.hasAttribute('action') && !row.hasAttribute('destructive') && !row.hasAttribute('chevron')) add('LIST-06', 'warn', row, ur.heading ?? '', 'an action-like row that is neither action, destructive nor chevroned: should it be tinted?')
    if (trailingI.length > 1) add('LIST-07', 'error', row, ur.heading ?? '', `${trailingI.length} controls at the row's end; one at most`)
    const c = cs(row)
    const border = ['top', 'right', 'bottom', 'left'].some((sd) => px(c.getPropertyValue(`border-${sd}-width`)) > 0 && c.getPropertyValue(`border-${sd}-style`) !== 'none')
    if (border || c.boxShadow !== 'none' || px(c.marginTop) !== 0 || px(c.marginBottom) !== 0) add('LIST-11', 'error', row, ur.heading ?? '', `${border ? 'a border' : c.boxShadow !== 'none' ? 'a box-shadow' : `vertical margin ${c.marginTop}/${c.marginBottom}`} on a ui-row; the kit draws the separators`)
    const p = row.parentElement
    if (p && p.localName !== 'ui-section' && p.localName !== 'ui-list' && isA(p)) {
      const pc = cs(p)
      const pb = ['top', 'right', 'bottom', 'left'].some((sd) => px(pc.getPropertyValue(`border-${sd}-width`)) > 0 && pc.getPropertyValue(`border-${sd}-style`) !== 'none')
      if (pb || pc.boxShadow !== 'none') add('LIST-11', 'error', p, '', `${pb ? 'a border' : 'a box-shadow'} on the rows' container; the kit draws the separators`)
    }
  }
  // an empty list is laid out at no height: drawn (it has a box), with nothing in it
  for (const list of all.filter((e) => e.localName === 'ui-list' && e.getClientRects().length > 0 && chainShown(e) && cs(e).visibility === 'visible')) {
    if (descendants(list).some((d) => d.localName === 'ui-row' && visible(d))) continue
    const sv = inside(parentOf(list), 'ui-split-view')
    let scope: Element[] = []
    if (sv) {
      let col: Element | null = list
      while (col && col.parentElement !== sv) col = col.parentElement
      const slot = col?.getAttribute('slot') ?? ''
      scope = col ? Array.from(sv.children).filter((k) => (k.getAttribute('slot') ?? '') === slot) : list.parentElement ? [list.parentElement] : []
    } else if (list.parentElement) scope = [list.parentElement]
    const hasEmpty = scope.some((sc) => (sc.localName === 'ui-empty-state' && visible(sc)) || Array.from(sc.querySelectorAll('ui-empty-state')).some(visible))
    if (!hasEmpty) add('LIST-16', 'error', list, '', 'a list with no rows and no empty state beside it')
  }

  // ---- modals ------------------------------------------------------------------------------------------------
  if (on('MODAL-01')) {
    const open = [...modals, ...popovers, ...menus]
    const ok = open.length === 2 && open.filter(isAlert).length === 1 && sheets.length === 1
    if (open.length > 1 && !ok) add('MODAL-01', 'error', open[1], '', `${open.length} modals, popovers and menus open at once (${open.map((x) => describe(x).split(' » ').pop()).join(', ')}); one at a time`)
  }
  if (on('MODAL-02')) for (const call of dr.dialogs) findings.push({ check: 'MODAL-02', severity: 'error', element: 'window', text: call, detail: `${call.split('(')[0]}() called: a browser dialog; use ui-alert`, box: { x: 0, y: 0, w: 0, h: 0 } })
  if (o.surface === 'app' && on('MODAL-02')) {
    const area = vp.width * vp.height
    for (const el of all) {
      if (!isA(el) || el.localName.startsWith('ui-') || !visible(el) || !contains(contentRoot, el)) continue
      const c = cs(el)
      if (c.position !== 'fixed' && c.position !== 'absolute') continue
      const r = el.getBoundingClientRect()
      const cover = Math.max(0, Math.min(r.right, vp.right) - Math.max(r.left, vp.left)) * Math.max(0, Math.min(r.bottom, vp.bottom) - Math.max(r.top, vp.top))
      if (cover < 0.8 * area || rgba(c.backgroundColor)[3] < 0.2) continue
      // a hand-made modal lies over other content; an app's own root view holds everything it covers
      const overlays = T.some((t) => !contains(el, t.el) && !contains(t.el, el) && t.rects.some((q) => q.left < r.right && q.right > r.left && q.top < r.bottom && q.bottom > r.top))
      if (overlays) add('MODAL-02', 'error', el, '', `a hand-made overlay covering ${Math.round((cover / area) * 100)}% of the window; modals are ui-alert or the shell's`)
    }
  }
  for (const m of modals) {
    if (m.localName === 'dialog') {
      const bd = getComputedStyle(m, '::backdrop')
      const b = rgba(bd.backgroundColor)
      if (!is(b, m, ['--dimming'])) add('MODAL-03', 'error', m, nameOf(m), `the backdrop is ${hex(b)}; expected --dimming`)
      if (bd.backdropFilter && bd.backdropFilter !== 'none') add('MOTION-05', 'error', m, nameOf(m), `the backdrop is blurred (${bd.backdropFilter}); a dim is only a dim`)
    }
  }
  for (const scrim of all.filter((e) => e.classList.contains('ios-scrim') && visible(e))) {
    const b = rgba(cs(scrim).backgroundColor)
    if (!is(b, scrim, ['--dimming'])) add('MODAL-03', 'error', scrim, '', `the scrim is ${hex(b)}; expected --dimming`)
    if (cs(scrim).backdropFilter !== 'none' && !inside(scrim, '.spotlight, [data-testid*=spotlight]')) add('MOTION-05', 'error', scrim, '', `the scrim is blurred (${cs(scrim).backdropFilter}); a dim is only a dim`)
  }
  for (const pm of [...popovers, ...menus]) {
    const pr = pm.getBoundingClientRect()
    const scrim = all.find((e) => {
      if (e === pm || contains(e, pm) || contains(pm, e) || !visible(e)) return false
      const r = e.getBoundingClientRect()
      return r.width * r.height >= 0.8 * innerWidth * innerHeight && rgba(cs(e).backgroundColor)[3] > 0 && /^(fixed|absolute)$/.test(cs(e).position) && ID.get(e)! < ID.get(pm)! && !contains(e, contentRoot) && e !== contentRoot && !scaffold.has(e)
    })
    if (scrim && pr.width) add('MODAL-03', 'error', pm, nameOf(pm), `a scrim (${describe(scrim)}) behind a ${pm.getAttribute('role') === 'menu' ? 'menu' : 'popover'}; only a modal dims`)
  }
  for (const d of all.filter((e) => e.getAttribute('role') === 'dialog' && visible(e))) if (!nameOf(d)) add('MODAL-06', 'error', d, '', 'an open dialog with no accessible name')
  for (const al of alerts) {
    const title = descendants(al).find((d) => /^h[1-6]$/.test(d.localName) && visible(d)) ?? (al.getAttribute('aria-labelledby') ? (al.getRootNode() as Document | ShadowRoot).getElementById?.(al.getAttribute('aria-labelledby')!.split(/\s+/)[0]) : null)
    if (title && run === 'default') {
      const tc = cs(title)
      const lh = tc.lineHeight === 'normal' ? px(tc.fontSize) * 1.2 : px(tc.lineHeight)
      const h = title.getBoundingClientRect().height
      if (h > 2 * lh + 1) add('MODAL-09', 'error', title, title.textContent ?? '', `alert title on ${Math.round(h / lh)} lines (${Math.round(h)}px); at most two`)
      const box = title.parentElement
      if (box && box.scrollHeight > box.clientHeight + 1) add('MODAL-09', 'error', box, rendered(box), `the alert's text scrolls (${box.scrollHeight}px in ${box.clientHeight}px)`)
    }
    const buttons = I.filter((i) => contains(al, i))
    const cy = (e: Element): number => (hitBox(e).top + hitBox(e).bottom) / 2
    if (buttons.length > 3) add('MODAL-10', 'error', al, nameOf(al), `${buttons.length} buttons in an alert; three at most`)
    else if (buttons.length === 3 && Math.max(...buttons.map(cy)) - Math.min(...buttons.map(cy)) <= 4) add('MODAL-10', 'error', al, nameOf(al), 'three buttons side by side; three stack')
    const cancel = buttons.find((b) => nameOf(b) === 'Cancel')
    if (cancel) {
      const inRow = buttons.length === 2 && Math.abs(cy(buttons[0]) - cy(buttons[1])) <= 4
      if (inRow && buttons.some((b) => b !== cancel && hitBox(b).left < hitBox(cancel).left)) add('MODAL-11', 'error', cancel, 'Cancel', 'Cancel is not the leading button of the pair')
      if (!inRow && buttons.length >= 2 && buttons.some((b) => b !== cancel && cy(b) > cy(cancel) + 4)) add('MODAL-11', 'error', cancel, 'Cancel', 'Cancel is not the lowest of the stacked buttons')
    }
    const textColour = (b: Element): RGBA => {
      const t = T.find((x) => contains(b, x.el) || (x.st !== x.el && contains(b, x.st)))
      return rgba((t ? t.c : cs(isK(b) ? innerTarget(b) ?? b : b)).color)
    }
    const red = buttons.filter((b) => is(textColour(b), b, ['--ios-red']))
    const f = openingFocus
    const focused = f ? buttons.find((b) => b === f || contains(b, f)) : undefined
    if (red.length) {
      if (!cancel) add('MODAL-12', 'error', al, nameOf(al), 'a destructive alert with no Cancel')
      if (focused && red.includes(focused)) add('MODAL-12', 'error', focused, nameOf(focused), 'the red button has the focus 300 ms after the alert opened')
    } else if (buttons.length === 2) {
      if (focused && nameOf(focused) === 'Cancel') add('MODAL-13', 'error', focused, 'Cancel', 'Cancel has the focus in an alert that destroys nothing; the likely answer is the default')
      const bold = buttons.find((b) => textWeight(b) === 600)
      if (bold && nameOf(bold) === 'Cancel') add('MODAL-13', 'error', bold, 'Cancel', 'Cancel is the semibold button in an alert that destroys nothing')
    }
  }
  for (const p of popovers) {
    const pr = p.getBoundingClientRect()
    const id = p.id
    const opener = (id ? all.find((e) => (e.getAttribute('aria-controls') ?? '').split(/\s+/).includes(id)) : undefined) ?? (() => {
      const ex = all.filter((e) => e.getAttribute('aria-expanded') === 'true' && visible(e))
      return ex.length === 1 ? ex[0] : undefined
    })()
    if (opener) {
      const h = hitBox(opener)
      if (Math.min(h.right, pr.right) - Math.max(h.left, pr.left) > 1 && Math.min(h.bottom, pr.bottom) - Math.max(h.top, pr.top) > 1) add('MODAL-19', 'error', p, nameOf(p), `the popover covers its opener ${describe(opener)}; it points at it`)
    }
    const vw = o.app ? vp.width : innerWidth
    if (pr.width > 400 || pr.width > 0.5 * vw) add('MODAL-21', 'error', p, nameOf(p), `a popover ${Math.round(pr.width)}px wide in a ${Math.round(vw)}px window; at most 400px and half the width`)
    else if (vw < 700) add('MODAL-21', 'error', p, nameOf(p), `a popover in a ${Math.round(vw)}px-wide window; compact width uses a sheet`)
  }

  // ---- menus -------------------------------------------------------------------------------------------------
  for (const m of all.filter((e) => e.getAttribute('role') === 'menu' && !kitInternal(e) && e.getClientRects().length)) {
    if (o.surface === 'app') add('MENU-01', 'error', m, nameOf(m), 'a menu drawn by the app; menus are the shell\'s')
    else if (!isContextMenu(m) && !m.classList.contains('mt-menu')) add('MENU-01', 'warn', m, nameOf(m), 'a menu that is not the shell\'s ContextMenu or MultitaskingMenu')
  }
  {
    const targets = new Set<Element>()
    for (const t of dr.contextmenu) if (t instanceof Element) targets.add(t)
    // addEventListener (recorded before the page ran), an oncontextmenu handler, or Svelte's delegated handler
    for (const el of all) if (Object.prototype.hasOwnProperty.call(el, '__contextmenu') || (el as HTMLElement).oncontextmenu) targets.add(el)
    for (const el of targets) {
      if (!el.isConnected || el === root || el === document.body || el === storyRoot || scaffold.has(el) || !visible(el)) continue
      if (menus.some((m) => contains(el, m) || contains(m, el))) continue // the menu's own press-catching layer
      const has = el.hasAttribute('aria-haspopup') || descendants(el).some((d) => d.hasAttribute('aria-haspopup') && interactiveish(d))
      if (!has) add('MENU-02', 'error', el, collapse(rendered(el)).slice(0, 80), 'a context menu on right-click with no aria-haspopup: how do the keyboard and a long press open it?')
    }
  }
  for (const m of menus) {
    const items = menuItems(m)
    // groups: split by separators or a visible gap of 6px
    const groups: Element[][] = [[]]
    const seq = descendants(m).filter((d) => (d.getAttribute('role') === 'separator' || /^menuitem/.test(d.getAttribute('role') ?? '')) && (visible(d) || d.getAttribute('role') === 'separator'))
    let prevBottom: number | null = null
    for (const d of seq) {
      if (d.getAttribute('role') === 'separator') {
        if (groups[groups.length - 1].length) groups.push([])
        prevBottom = null
        continue
      }
      const r = d.getBoundingClientRect()
      if (prevBottom !== null && r.top - prevBottom >= 6 && groups[groups.length - 1].length) groups.push([])
      groups[groups.length - 1].push(d)
      prevBottom = r.bottom
    }
    const real = groups.filter((g) => g.length)
    if (items.length > 12 || real.length > 3) add('MENU-05', 'error', m, nameOf(m), `${items.length} items in ${real.length} groups; at most 12 items in 3 groups`)
    const red = (it: Element): boolean => {
      const t = T.find((x) => contains(it, x.el))
      return !!t && is(rgba(t.c.color), t.st, ['--ios-red'])
    }
    const di = items.findIndex((it) => /^(Delete|Remove|Erase|Discard|Move to Trash)\b/.test(nameOf(it)))
    if (di >= 0) {
      const after = items.slice(di + 1).find((it) => !red(it))
      if (after) add('MENU-07', 'error', items[di], nameOf(items[di]), `“${nameOf(items[di])}” is followed by “${nameOf(after)}”; destructive items go last`)
    }
    for (const g of real) {
      const withGlyph = g.filter((it) => descendants(it).some((d) => (d.localName === 'svg' || d.localName === 'ui-icon') && visible(d)))
      if (withGlyph.length && withGlyph.length < g.length) add('MENU-09', 'error', g[0], nameOf(g[0]), `${withGlyph.length} of ${g.length} items in a group have a glyph; all or none`)
    }
    if (isContextMenu(m)) {
      for (const it of descendants(m).filter((d) => /^menuitem/.test(d.getAttribute('role') ?? ''))) {
        if (it.hasAttribute('disabled') || it.getAttribute('aria-disabled') === 'true') add('MENU-10', 'error', it, nameOf(it), 'a disabled item in a context menu; hide what does not apply')
        const vt = visibleText(it)
        if (/[⌘⌥⌃⇧]/.test(vt)) add('MENU-11', 'error', it, vt, 'a keyboard shortcut in a context menu')
      }
    }
    const opener = all.find((e) => /^menuitem/.test(e.getAttribute('role') ?? '') && m.id && (e.getAttribute('aria-controls') ?? '').split(/\s+/).includes(m.id))
    const sub = !!opener || !!inside(parentOf(m), '[role=menu], [role^=menuitem]')
    if (sub) for (const it of items) if (it.hasAttribute('aria-haspopup')) add('MENU-12', 'error', it, nameOf(it), 'a submenu inside a submenu; one level only')
  }
  for (const it of all.filter((e) => /^menuitem/.test(e.getAttribute('role') ?? '') && visible(e))) {
    const nm = nameOf(it)
    if (/\s(the|a|an)\s/i.test(nm)) add('MENU-08', 'error', it, nm, `“${nm}” has an article; a verb and its object`)
  }
  // COLOR-09: what destroys is red
  {
    const items = [...all.filter((e) => /^menuitem/.test(e.getAttribute('role') ?? '') && visible(e)), ...alerts.flatMap((al) => I.filter((i) => contains(al, i))), ...all.filter((e) => e.matches('ui-row[tappable]') && visible(e))]
    for (const it of new Set(items)) {
      const nm = nameOf(it)
      if (!/^(Delete|Erase|Remove|Discard|Empty Trash|Reset)\b/.test(nm)) continue
      const title = it.localName === 'ui-row' ? it.shadowRoot?.querySelector('.title') ?? null : null
      const t = title ? { c: cs(title), st: title } : T.find((x) => contains(it, x.el) || (x.st !== x.el && contains(it, x.st)))
      if (!t) continue
      if (!is(rgba(t.c.color), t.st, ['--ios-red'])) add('COLOR-09', 'error', it, nm, `“${nm}” in ${hex(rgba(t.c.color))}; what destroys is --ios-red (destructive)`)
    }
  }

  // ---- feedback ----------------------------------------------------------------------------------------------
  for (const sp of all.filter((e) => e.localName === 'ui-spinner' && visible(e))) {
    const label = ((sp as unknown as { label?: string }).label ?? '').trim()
    if (!label || label === 'Loading') add('FEED-07', 'error', sp, label, `spinner label “${label}”; say what is loading`)
  }
  for (const t of T) if (/^Loading(…|\.\.\.)?$/.test(t.text)) add('FEED-07', 'error', t.el, t.text, '“Loading” alone; say what is loading')
  if (on('FEED-09') && dr.blank === true) findings.push({ check: 'FEED-09', severity: 'error', element: describe(contentRoot), text: '', detail: '200 ms after the view was shown it held no text, spinner or empty state', box: boxOf(contentRoot) })
  if (on('FEED-10') && run === 'default' && !o.played) {
    const total = await new Promise<number>((res) => {
      let sum = 0
      try {
        const po = new PerformanceObserver((list) => {
          for (const e of list.getEntries() as (PerformanceEntry & { value: number; hadRecentInput: boolean })[]) if (!e.hadRecentInput) sum += e.value
        })
        po.observe({ type: 'layout-shift', buffered: true })
        setTimeout(() => {
          po.disconnect()
          res(sum)
        }, 50)
      } catch {
        res(0)
      }
    })
    if (total > 0.1) findings.push({ check: 'FEED-10', severity: 'error', element: describe(contentRoot), text: '', detail: `layout shift ${r1(total)} since the view opened (${Math.round(performance.now())} ms); expected at most 0.1`, box: boxOf(contentRoot) })
  }
  if (on('FEED-17')) {
    const banners = all.filter((e) => e.localName === 'ui-banner' && visible(e) && contains(contentRoot, e))
    if (banners.length > 1) for (const b of banners.slice(1)) add('FEED-17', 'error', b, rendered(b), `${banners.length} banners in one window; one at a time`)
  }
  {
    const fed: Element[] = []
    for (const e of all) {
      if (e.localName === 'ui-banner' && visible(e)) fed.push(e)
      else if (e.localName === 'ui-section' && e.getAttribute('footer-tone') === 'error') {
        const f = e.shadowRoot?.querySelector('.footer')
        if (f && visible(f)) fed.push(f)
      } else if (e.classList.contains('ios-toast') && visible(e)) fed.push(e)
    }
    for (const e of fed) {
      const ts = e.localName === 'ui-banner' ? T.filter((t) => contains(e, t.el) || (t.st !== t.el && contains(e, t.st))) : T.filter((t) => contains(e, t.el))
      for (const t of ts) {
        const live = inside(t.st !== t.el ? t.st : t.el, '[role=status], [role=alert], [aria-live]:not([aria-live=off])')
        if (!live) {
          add('FEED-20', 'error', t.el, t.text, 'feedback text outside a live region (role status or alert, or aria-live): a screen reader is not told')
          break
        }
      }
    }
  }

  // ---- help --------------------------------------------------------------------------------------------------
  for (const i of I) {
    if (!iconOnly(i)) continue
    const target = isK(i) ? innerTarget(i) : null
    const titled = [i, target, isK(i) ? i : hostOf(i)].some((e) => e && (e.getAttribute('title') ?? '').trim())
    // its name written beside it (a Home Screen icon's label) or shown on hover (the Dock's) is the shell's equivalent
    const nm0 = nameOf(i).toLowerCase()
    const p = parentOf(i)
    const labelled = !!nm0 && !!p && Array.from(p instanceof HTMLSlotElement ? p.assignedElements() : p.children).some((sb) => sb !== i && !contains(sb, i) && collapse(sb.textContent ?? '').toLowerCase() === nm0)
    if (!titled && !labelled) add('HELP-10', 'error', i, nameOf(i), 'a glyph-only control without a title tooltip')
    const nm = nameOf(i)
    if (nm && /^(button|icon|image|img|svg|graphic|\W+|[a-z0-9]+(\.[a-z0-9]+)+)$/i.test(nm)) add('A11Y-04', 'error', i, nm, `glyph-only control named “${nm}”; name what it does`)
  }
  for (const el of all.filter((e) => (e.getAttribute('title') ?? '').trim() && visible(e))) {
    const vt = visibleText(el)
    if (vt && vt.toLowerCase() === (el.getAttribute('title') ?? '').trim().toLowerCase()) add('HELP-11', 'error', el, vt, `title “${el.getAttribute('title')}” repeats the visible text; a tooltip adds`)
  }

  // ---- accessibility -----------------------------------------------------------------------------------------
  for (const i of I) {
    const nm = nameOf(i)
    const h = hitBox(i)
    const inRunningText = i.matches('a[href]') && (() => {
      const p = parentOf(i)
      return !!p && tOf.has(p) && collapse(tOf.get(p)!.text).length > 0
    })()
    const fromContext = isK(i) || kitInternal(i) // a kit control's hit area comes from `hit` or the row it sits in
    if (!rowsL03.has(i) && !inRunningText && (o.app || !fromContext) && (h.width < 43.5 || h.height < 43.5)) add('A11Y-01', 'error', i, nm, `hit box ${Math.round(h.width)}×${Math.round(h.height)}; expected at least 44×44`)
    if (!nm) add('A11Y-03', 'error', i, '', `an interactive ${roleOf(i) || i.localName} with no accessible name`)
    const vt = visibleText(i)
    const field = i.matches('input, textarea, select, ui-text-field, ui-text-area, ui-picker, [contenteditable]') || /^(textbox|combobox|searchbox|spinbutton|slider)$/.test(roleOf(i))
    if (vt && nm && !field) {
      const norm = (x: string): string => x.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim()
      if (!norm(nm).includes(norm(vt))) add('A11Y-05', 'error', i, nm, `the name “${clip80(nm)}” does not contain the visible label “${clip80(vt)}”`)
    }
    if (nm && /\b(button|link|switch|checkbox)$/i.test(nm)) add('A11Y-05', 'error', i, nm, `the name ends in its role (“${nm.split(/\s+/).pop()}”); the role is announced already`)
    // INPUT-01: the pointer
    const textInput = i.matches('textarea, select, [contenteditable]:not([contenteditable=false]), ui-text-field, ui-text-area, ui-picker') || (i.localName === 'input' && /^(text|search|email|url|tel|password|number|date|time|datetime-local|month|week)$/.test((i as HTMLInputElement).type))
    const onlyTabindex = !i.matches(I_SEL.replace(', [tabindex]:not([tabindex="-1"])', '')) && !isK(i)
    if (!textInput && !onlyTabindex) {
      const cur = cs(isK(i) ? innerTarget(i) ?? i : i).cursor
      if (cur === 'auto' || cur === 'default') add('INPUT-01', 'error', i, nm, `cursor ${cur} on a control; expected pointer`)
    }
  }
  for (const el of all) if (visible(el) && cs(el).cursor.startsWith('url(')) add('INPUT-01', 'error', el, '', 'a custom cursor image; the system draws the pointer')
  // A11Y-02: overlapping targets (within one layer)
  {
    const reported = new Set<Element>()
    for (let a = 0; a < I.length; a++) {
      const ha = localHit(I[a])
      if (!ha) continue
      const va = seenHit(I[a])
      const sa = scrollerOf(I[a])
      const la = layerOf(I[a])
      for (let b = a + 1; b < I.length; b++) {
        if (reported.has(I[b]) || layerOf(I[b]) !== la || contains(I[a], I[b]) || contains(I[b], I[a])) continue
        // in one scroller both move together; across scrollers only what is in view can meet
        const same2 = scrollerOf(I[b]) === sa
        const hb = same2 ? localHit(I[b]) : seenHit(I[b])
        const h1 = same2 ? ha : va
        if (!hb || !h1) continue
        const ix = Math.min(h1.right, hb.right) - Math.max(h1.left, hb.left)
        const iy = Math.min(h1.bottom, hb.bottom) - Math.max(h1.top, hb.top)
        if (ix > 1 && iy > 1) {
          reported.add(I[b])
          add('A11Y-02', 'error', I[b], nameOf(I[b]), `hit box overlaps “${clip80(nameOf(I[a]))}” (${describe(I[a])}) by ${Math.round(ix)}×${Math.round(iy)}`)
        }
      }
    }
  }
  // A11Y-06: what can be clicked is a control
  for (const el of all) {
    if (!visible(el) || !isA(el) || cs(el).cursor !== 'pointer') continue
    if (insideInteractive(el) || (el.localName === 'label' && (el as HTMLLabelElement).control)) continue
    const p = parentOf(el)
    if (p && isA(p) && cs(p).cursor === 'pointer' && !insideInteractive(p)) continue // reported on the outermost
    add('A11Y-06', 'error', el, collapse(rendered(el)).slice(0, 80), 'cursor pointer on something that is not a control')
  }
  // A11Y-07: state exposed
  for (const el of all.filter((e) => e.hasAttribute('aria-haspopup') && visible(e))) {
    const ids = (el.getAttribute('aria-controls') ?? '').split(/\s+/).filter(Boolean)
    const r0 = el.getRootNode() as Document | ShadowRoot
    const opened = ids.map((id) => r0.getElementById?.(id) ?? document.getElementById(id)).some((t) => t && visible(t))
    if (opened && el.getAttribute('aria-expanded') !== 'true') add('A11Y-07', 'error', el, nameOf(el), 'its menu or popover is open but aria-expanded is not true')
  }
  for (const el of all.filter((e) => (e.localName === 'ui-row' || (e.getAttribute('role') === 'option' && !kitInternal(e))) && visible(e))) {
    const face = el.localName === 'ui-row' ? el.shadowRoot?.querySelector('.face') ?? el : el
    const b = rgba(cs(face).backgroundColor)
    if (b[3] > 0 && is(b, face, ['--tint', '--tint-fill']) && el.getAttribute('aria-selected') !== 'true') add('A11Y-07', 'error', el, el.localName === 'ui-row' ? visibleText(el) : nameOf(el), 'drawn selected (tint fill) without aria-selected="true"')
  }
  // A11Y-08: images
  for (const img of all.filter((e) => e.localName === 'img' && visible(e))) if (!img.hasAttribute('alt')) add('A11Y-08', 'error', img, '', 'an img without alt (alt="" when it is decorative)')
  for (const svg of all.filter((e) => e.localName === 'svg' && visible(e) && !inside(e, 'ui-icon'))) {
    if (inside(svg, '[aria-hidden=true]') || /^(presentation|none)$/.test(svg.getAttribute('role') ?? '') || nameOf(svg) || insideInteractive(svg)) continue
    if (inside(parentOf(svg), 'svg')) continue
    add('A11Y-08', 'error', svg, '', 'an svg with no name and not aria-hidden="true"')
  }
  // A11Y-09: reading order
  for (const el of all) {
    if (!isA(el) || !visible(el)) continue
    const c = cs(el)
    if (!holdsI.has(el)) continue
    if (c.order !== '0' && /flex|grid/.test(cs(parentOf(el) ?? root).display)) add('A11Y-09', 'error', el, '', `order ${c.order} on something holding a control; the focus order no longer follows what is seen`)
    if (/reverse/.test(c.flexDirection) && /flex/.test(c.display)) add('A11Y-09', 'error', el, '', `flex-direction ${c.flexDirection} around controls`)
  }
  if (run === 'default' && !root.lang.trim()) findings.push({ check: 'A11Y-10', severity: 'error', element: 'html', text: '', detail: 'the document has no lang', box: { x: 0, y: 0, w: 0, h: 0 } })
  for (const el of all) if ((el as HTMLElement).tabIndex > 0) add('A11Y-12', 'error', el, nameOf(el), `tabindex ${(el as HTMLElement).tabIndex}; never positive`)

  motionChecks()

  // MOTION-08: nothing loops but progress
  if (run === 'default') {
    for (const el of all) {
      const c = cs(el)
      if (c.animationName === 'none' || !c.animationIterationCount.includes('infinite')) continue
      if (inside(el, 'ui-spinner, .ios-spinner, [role=progressbar], [role=status]')) continue
      const frames = el.getAnimations().flatMap((a) => ((a.effect as KeyframeEffect | null)?.getKeyframes() ?? []))
      const spin = frames.length > 0 && frames.every((f) => Object.keys(f).filter((k) => !['offset', 'computedOffset', 'easing', 'composite'].includes(k)).every((k) => (k === 'transform' && /^(none|rotate(Z)?\([^)]*\))$/.test(String(f[k]).trim())) || k === 'rotate'))
      if (!spin) add('MOTION-08', 'error', el, '', `animation ${c.animationName} loops forever; only progress loops (a spinner)`)
    }
  }
  // MOTION-02, MOTION-04: bars over their scroller; the materials of floating surfaces
  if (run === 'default') {
    for (const sf of [...alerts, ...popovers, ...menus]) {
      const layer = [sf, ...descendants(sf)].find((d) => rgba(cs(d).backgroundColor)[3] > 0.005)
      if (!layer) continue
      const b = rgba(cs(layer).backgroundColor)
      if (!is(b, layer, ['--material-thick'])) add('MOTION-02', 'error', layer, nameOf(sf), `${isAlert(sf) ? 'an alert' : sf.getAttribute('role') === 'menu' ? 'a menu' : 'a popover'} on ${hex(b)}${tokenNames(b, layer).length ? ` (${tokenNames(b, layer).filter((n) => n !== 'white').join(' = ')})` : ''}; expected --material-thick`)
    }
    const frame = (): Promise<void> => new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(() => res())))
    for (const bar of bars) {
      let scroller: Element | null = null
      for (let at = parentOf(bar); at; at = parentOf(at)) {
        const c = cs(at)
        if (/auto|scroll/.test(c.overflowY) && at.scrollHeight > at.clientHeight + 1) {
          scroller = at
          break
        }
      }
      const b0 = rgba(cs(bar).backgroundColor)
      if (!scroller || scroller.scrollTop === 0) if (b0[3] >= 0.995) add('MOTION-04', 'error', bar, '', `the bar is opaque (${hex(b0)}) at the scroll edge; bars are clear until content scrolls under them`)
      if (!scroller || scroller.scrollHeight - scroller.clientHeight < 100) continue
      const before = scroller.scrollTop
      scroller.scrollTop = before + 100
      scroller.dispatchEvent(new Event('scroll'))
      await frame()
      await new Promise((res) => setTimeout(res, 50))
      for (const a of bar.getAnimations()) {
        try {
          a.finish()
        } catch {
          /* infinite */
        }
      }
      const b1 = rgba(getComputedStyle(bar).backgroundColor)
      if (!is(b1, bar, ['--material-chrome'])) add('MOTION-02', 'error', bar, '', `scrolled 100px, the bar is ${hex(b1)}; expected --material-chrome`)
      scroller.scrollTop = before
      scroller.dispatchEvent(new Event('scroll'))
      await frame()
    }
  }

  // ---- input -------------------------------------------------------------------------------------------------
  for (const g of all.filter((e) => /^(listbox|radiogroup|menu|grid|tablist|tree)$/.test(e.getAttribute('role') ?? '') || e.localName === 'ui-list')) {
    if (!visible(g)) continue
    const ITEM = /^(option|radio|menuitem|menuitemcheckbox|menuitemradio|tab|row|gridcell|treeitem)$/
    const isItem = (e: Element): boolean => e.localName === 'ui-row' || ITEM.test(e.getAttribute('role') ?? '')
    const d = descendants(g)
    let stops = 0
    for (const e of d) {
      if ((e as HTMLElement).tabIndex !== 0 || !visible(e) || disabledSelf(e) || !(e.matches(I_SEL) || e.hasAttribute('tabindex') || e.matches('a[href], button, input, select, textarea'))) continue
      // an item counts once; a control inside an item (a switch at a row's end) is the item's, not the group's
      let item: Element | null = null
      for (let at: Element | null = e; at && at !== g; at = parentOf(at)) if (isItem(at)) item = at
      if (item && item !== e && !(item.localName === 'ui-row' && hostOf(e) === item)) continue
      stops++
    }
    if (stops > 1) add('INPUT-07', 'error', g, nameOf(g), `${stops} tab stops in one ${g.localName === 'ui-list' ? 'list' : g.getAttribute('role')}; one stop, the arrows inside`)
  }
  for (const el of all) {
    if (!isA(el) || !visible(el)) continue
    const c = cs(el)
    for (const axis of ['y', 'x'] as const) {
      const ov = axis === 'y' ? c.overflowY : c.overflowX
      const scrolls = (e: Element, ax: 'x' | 'y'): boolean => /auto|scroll/.test(ax === 'y' ? cs(e).overflowY : cs(e).overflowX) && (ax === 'y' ? e.scrollHeight > e.clientHeight + 1 : e.scrollWidth > e.clientWidth + 1)
      if (!/auto|scroll/.test(ov) || !scrolls(el, axis)) continue
      for (let at = parentOf(el); at && at !== root && at !== document.body && at !== document.scrollingElement; at = parentOf(at)) {
        if (scrolls(at, axis)) {
          add('INPUT-16', 'error', el, '', `scrolls ${axis === 'y' ? 'vertically' : 'sideways'} inside ${describe(at)}, which scrolls the same way`)
          break
        }
      }
    }
  }

  probeWrap.remove()
  return { findings, texts }
}
