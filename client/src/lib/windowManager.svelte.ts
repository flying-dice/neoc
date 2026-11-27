import type { Frame, StageSummary, WindowLayout } from '@vos/ui'
import { appOf, MIN_WIDTH, type AppDefinition, type AppId } from './apps.svelte'
import { sizeClassOf } from '../shell/sizeClass'
import { randomId } from './randomId'

/**
 * Stage Manager–style window manager.
 *
 *  • Windows belong to a *stage* (an app group). One stage is on screen; the
 *    rest sit in the recent-apps strip, most recent first.
 *  • A window's layout is floating, full screen, split left/right, or slide
 *    over. Non-floating frames are derived from the workspace bounds, so they
 *    re-flow on resize; floating windows keep their own frame.
 *  • Slide Over lives outside stages and floats above everything.
 *
 * Sizes: an app's minimum window is never broken by a layout. Two panes of a Split View always have at least their
 * app's minimum width: the divider stops there (a quarter to three quarters otherwise), the ratio is re-checked when
 * the second pane joins and when the workspace changes. Where the two minimums cannot share the screen the split is
 * not made: `unavailableLayouts` tells the multitasking menu to show Split View disabled (and `setLayout` ignores it
 * for any other caller), an app that will not fit beside the waiting one opens on its own (the waiting pane goes back
 * to floating and `notice` says so), and a screen that shrinks past that dissolves the split into floating windows.
 * Slide Over is the same: not offered where the app's minimum width does not fit the screen.
 * Where the screen is shorter than an app's minimum height (a waiting split and Slide Over stop above the Dock) the
 * minimum wins, as it does for a floating window.
 *
 * Compact screens (a phone): floating windows, Split View and Slide Over are what iPadOS does on a regular-size screen;
 * on a compact one iOS shows one app at a time, full screen. A workspace narrower or shorter than the size-class
 * breakpoint (`COMPACT_BELOW`) is `compact`, and there every window is
 * presented full screen (`layoutOf`, `frameOf`), whatever layout it has, with the Dock put away: as on iOS, an app has
 * the whole screen, and the Dock is on the home screen, which the window's ··· menu goes back to (Minimise or Close)
 * (`dockHidden`). The recent-apps strip steps back to the screen's edge (`stripObscured`).
 * An app's minimum size is a regular-size window's: on a compact screen the window is the screen's size. Nothing a
 * window remembers is used or changed while the screen is compact: its layout, its floating frame, a split and its
 * divider wait, untouched, and come back when the screen has room again (`setLayout` and `setFrame` do nothing, and a
 * change of size fits nothing until the screen is regular again). The multitasking menu offers no other layout
 * (`unavailableLayouts`): Full Screen, the one shown, and Minimise and Close, which go back to the home screen.
 */

export interface WindowState {
  id: string
  appId: AppId
  stageId: string | null
  layout: WindowLayout
  /** Frame used when floating (kept while in other layouts, to restore). */
  frame: Frame
  z: number
}

interface Stage {
  id: string
  windowIds: string[]
  /** Left pane fraction in split view. */
  splitRatio: number
  /** Side waiting for a second app after choosing Split View. */
  awaitingSplit: 'split-left' | 'split-right' | null
  lastActive: number
}

const INSET = 8
const GAP = 10
const STRIP_WIDTH = 150
/** Room kept clear for the Dock: 58px icons + 28px tray padding + 12px margin + 6px air. */
const DOCK_SPACE = 104
const SLIDE_OVER_WIDTH = 380

const uid = (): string => randomId().slice(0, 8)

export class WindowManager {
  windows = $state<WindowState[]>([])
  stages = $state<Stage[]>([])
  activeStageId = $state<string | null>(null)
  focusedId = $state<string | null>(null)
  slideOverHidden = $state(false)
  /** A one-line explanation of a layout the manager changed on its own (shown by the desktop, cleared by it). */
  notice = $state<string | null>(null)
  #bounds = $state({ width: 1280, height: 800 })
  #topZ = 10

  /**
   * The workspace size. When it changes the splits are checked against it (re-clamped, or dissolved when their two
   * minimums no longer fit) and every window's own frame, whatever its layout, is brought inside it: a frame kept from
   * a bigger screen must not come back off a smaller one.
   */
  get bounds(): { width: number; height: number } {
    return this.#bounds
  }
  set bounds(next: { width: number; height: number }) {
    if (next.width === this.#bounds.width && next.height === this.#bounds.height) return
    this.#bounds = next
    if (this.compact) return // nothing is laid out by what it remembers: fitting it now would change what it comes back to
    for (const stage of this.stages) this.#fitSplit(stage)
    // a Slide Over whose app no longer fits the screen goes back to a floating window (never off the right edge)
    const slideOver = this.slideOver
    if (slideOver && !this.#canSlideOver(slideOver)) this.#dropSlideOver(slideOver)
    for (const win of this.windows) win.frame = this.#fitFrame(win, this.#onScreen(win.frame))
  }

  // ── Queries ────────────────────────────────────────────────────────────

  /** A screen too small for windows (narrower or shorter than the size-class breakpoint): one app at a time, full screen. */
  get compact(): boolean {
    return (
      sizeClassOf(this.#bounds.width) === 'compact' ||
      sizeClassOf(this.#bounds.height) === 'compact'
    )
  }

  /** The layout a window is shown in: its own, or full screen on a compact screen (its own is kept for a regular one). */
  layoutOf(win: WindowState): WindowLayout {
    return this.compact ? 'fullscreen' : win.layout
  }

  get activeStage(): Stage | undefined {
    return this.stages.find((s) => s.id === this.activeStageId)
  }

  /** Windows currently on stage, back to front. */
  get onStage(): WindowState[] {
    const stage = this.activeStage
    if (!stage) return []
    return this.windows.filter((w) => stage.windowIds.includes(w.id))
  }

  get slideOver(): WindowState | undefined {
    return this.windows.find((w) => w.layout === 'slideover')
  }

  get focused(): WindowState | undefined {
    return this.windows.find((w) => w.id === this.focusedId)
  }

  /** Stages for the recent-apps strip, most recent first. */
  get recentStages(): StageSummary[] {
    return this.stages
      .filter((s) => s.id !== this.activeStageId)
      .sort((a, b) => b.lastActive - a.lastActive)
      .map((s) => ({
        id: s.id,
        apps: s.windowIds
          .map((id) => this.windows.find((w) => w.id === id))
          .filter((w): w is WindowState => !!w)
          .flatMap((w) => {
            const app = appOf(w.appId)
            return app
              ? [{ id: w.appId, name: app.name, icon: app.icon, background: app.background }]
              : []
          })
      }))
  }

  /** Full screen or split windows take the whole stage: hide strip and dock. */
  get immersive(): boolean {
    return this.onStage.some((w) => this.layoutOf(w) !== 'floating')
  }

  /**
   * The dock hides when immersive, except while a split waits for its second app. On a compact screen it is only on the
   * home screen: an app there has the whole screen.
   */
  get dockHidden(): boolean {
    if (this.compact) return !this.atHome
    return this.immersive && !this.activeStage?.awaitingSplit
  }

  /** Strip auto-hides when immersive, on a compact screen (iOS has no Stage Manager), or a floating window overlaps it. */
  get stripObscured(): boolean {
    return this.compact || this.immersive || this.onStage.some((w) => w.frame.x < STRIP_WIDTH - 10)
  }

  /** The Home Screen is what is on show: no group is on stage and no Slide Over is out. */
  get atHome(): boolean {
    return this.onStage.length === 0 && !(this.slideOver && !this.slideOverHidden)
  }

  isOpen(appId: AppId): boolean {
    return this.windows.some((w) => w.appId === appId)
  }

  /**
   * The on-screen frame for a window, derived from its layout. A layout never gives less than the app's minimum size
   * (a short screen's Dock edge or a narrow Slide Over does not cut the app off, as a floating window never is).
   */
  frameOf(win: WindowState): Frame {
    if (this.compact) return this.#compactFrame(win)
    const frame = this.#layoutFrame(win)
    if (win.layout === 'floating') return frame
    const { minWidth, minHeight } = this.#minOf(win)
    return {
      ...frame,
      width: Math.max(frame.width, minWidth),
      height: Math.max(frame.height, minHeight)
    }
  }

  #layoutFrame(win: WindowState): Frame {
    const { width: W, height: H } = this.bounds
    const fullH = H - INSET * 2
    switch (win.layout) {
      case 'fullscreen':
        return { x: INSET, y: INSET, width: W - INSET * 2, height: fullH }
      case 'split-left':
      case 'split-right': {
        const ratio = this.stages.find((s) => s.id === win.stageId)?.splitRatio ?? 0.5
        // while a split waits for its second app the Dock is up, so the first pane stops above it
        return this.#splitPane(win.layout, ratio, this.dockHidden ? fullH : this.#heightAboveDock)
      }
      case 'slideover': {
        const width = this.#slideOverWidth(win)
        return {
          x: this.slideOverHidden ? W + 20 : Math.max(INSET, W - width - INSET), // never off the left of the screen
          y: INSET,
          width,
          height: this.#heightAboveDock // above the Dock, which a narrow screen would otherwise cover it with
        }
      }
      default:
        return win.frame
    }
  }

  /** The whole screen (the Dock is put away), whatever the app's minimum (a hidden Slide Over stays off screen). */
  #compactFrame(win: WindowState): Frame {
    const { width: W, height: H } = this.bounds
    const width = Math.max(0, W - INSET * 2)
    const x = win.layout === 'slideover' && this.slideOverHidden ? W + 20 : INSET
    return { x, y: INSET, width, height: Math.max(0, H - INSET * 2) }
  }

  /** One side of a split at the divider's position. */
  #splitPane(side: 'split-left' | 'split-right', ratio: number, height: number): Frame {
    const usable = this.#splitWidth
    const left = Math.round(usable * ratio)
    return side === 'split-left'
      ? { x: INSET, y: INSET, width: left, height }
      : { x: INSET + left + GAP, y: INSET, width: usable - left, height }
  }

  /** The width the two panes of a split share (the gap between them is not theirs). */
  get #splitWidth(): number {
    return this.bounds.width - INSET * 2 - GAP
  }

  /** The height of a window that starts at the top inset and ends at the Dock's top edge, where floating windows stop too. */
  get #heightAboveDock(): number {
    return this.bounds.height - INSET - DOCK_SPACE
  }

  /** The empty side of a split that waits for its second app: the pane the first one left, down to the same edge. */
  get splitPlaceholderFrame(): Frame | null {
    const stage = this.activeStage
    if (!stage?.awaitingSplit || this.compact) return null
    const first = this.onStage.find((w) => w.layout.startsWith('split'))
    return this.#splitPane(
      stage.awaitingSplit,
      stage.splitRatio,
      first ? this.frameOf(first).height : this.#heightAboveDock // the Dock is up while a split waits
    )
  }

  /** x position of the split divider, when the active stage is split. */
  get splitDividerX(): number | null {
    if (this.compact) return null
    const left = this.onStage.find((w) => w.layout === 'split-left')
    const right = this.onStage.find((w) => w.layout === 'split-right')
    if (!left && !right) return null
    const f = this.frameOf((left ?? right)!)
    return left ? f.x + f.width : f.x - GAP
  }

  // ── Opening & focus ────────────────────────────────────────────────────

  /**
   * Open (or focus) an app. `alongside` adds a new window to the current
   * stage instead of starting a new one — used when an app is opened on the
   * user's behalf (e.g. by an agent tool) so it appears next to what they're
   * looking at rather than replacing it. An id that is not an app the shell can
   * show (not installed, disabled, failed) is ignored.
   */
  open(appId: AppId, options: { alongside?: boolean } = {}): void {
    const app = appOf(appId)
    if (!app) return
    const existing = this.windows.find((w) => w.appId === appId)
    if (existing) {
      // An open app chosen for the empty side of a pending Split View leaves where it was and fills it, as a new one
      // would; where the two do not fit side by side it is shown where it is, as any open app chosen is.
      const pending = this.activeStage
      if (pending?.awaitingSplit && !this.compact && existing.stageId !== pending.id) {
        const waiting = this.onStage.find((w) => w.layout.startsWith('split'))
        if (waiting && this.#fits([this.#minOf(waiting).minWidth, this.#minOf(existing).minWidth])) {
          this.#detach(existing)
          return this.#fillSplit(existing, pending)
        }
      }
      if (existing.layout === 'slideover') {
        this.slideOverHidden = false
        return this.focus(existing.id)
      }
      if (existing.stageId) this.activateStage(existing.stageId)
      return this.focus(existing.id)
    }

    const win = this.#createWindow(app)
    const stage = this.activeStage

    // Filling the empty side of a pending Split View, when the two apps fit side by side at their minimums. On a compact
    // screen the split waits, untouched, for a screen with room for it, and this app opens as a stage of its own.
    if (stage?.awaitingSplit && !this.compact) {
      const waiting = this.onStage.find((w) => w.layout.startsWith('split'))
      if (waiting && this.#fits([this.#minOf(waiting).minWidth, app.minWidth])) {
        this.windows.push(win)
        return this.#fillSplit(win, stage)
      }
      // No room for both: the waiting pane gives up, this app opens as anything else would, and the person is told.
      if (waiting) {
        waiting.layout = 'floating'
        // where the waiting app went: beside the new one when it joins the stage (`alongside`), else into the strip
        const where = options.alongside ? 'is a normal window again' : 'is in your recent apps'
        this.notice = `${app.name} needs more room than half the screen, so it opened on its own. ${appOf(waiting.appId)?.name ?? 'The other app'} ${where}.`
      }
      stage.awaitingSplit = null
    }

    // Joining the current group, placed beside the existing windows.
    if (options.alongside && stage && stage.windowIds.length) {
      win.stageId = stage.id
      if (!this.compact) win.frame = this.#besideStage(win.frame) // which moves the others' floating frames
      stage.windowIds.push(win.id)
      this.windows.push(win)
      return this.focus(win.id)
    }

    // Otherwise iPadOS opens the app as a new stage; the old one joins the strip.
    const next: Stage = {
      id: uid(),
      windowIds: [win.id],
      splitRatio: 0.5,
      awaitingSplit: null,
      lastActive: Date.now()
    }
    win.stageId = next.id
    this.windows.push(win)
    this.stages.push(next)
    this.activeStageId = next.id
    this.focus(win.id)
  }

  /** `win` (in no stage) takes the empty side of `stage`'s pending Split View. */
  #fillSplit(win: WindowState, stage: Stage): void {
    win.layout = stage.awaitingSplit ?? 'split-right'
    win.stageId = stage.id
    stage.windowIds.push(win.id)
    stage.awaitingSplit = null
    stage.splitRatio = this.#clampRatio(stage, stage.splitRatio) // the divider may have been left where this app is too narrow
    this.focus(win.id)
  }

  focus(id: string): void {
    const win = this.windows.find((w) => w.id === id)
    if (!win) return
    if (this.focusedId !== id) win.z = ++this.#topZ
    this.focusedId = id
  }

  activateStage(id: string): void {
    const stage = this.stages.find((s) => s.id === id)
    if (!stage) return
    stage.lastActive = Date.now()
    this.activeStageId = id
    const top = this.onStage.reduce<WindowState | null>((a, w) => (!a || w.z > a.z ? w : a), null)
    if (top) this.focus(top.id)
  }

  /** Drag a strip group onto the stage: its windows join the active group. */
  mergeStage(id: string): void {
    const source = this.stages.find((s) => s.id === id)
    const target = this.activeStage
    if (!source || !target || source === target) return this.activateStage(id)
    for (const winId of source.windowIds) {
      const win = this.windows.find((w) => w.id === winId)
      if (!win) continue
      win.stageId = target.id
      // Joining a group returns windows to floating, cascaded into view.
      if (win.layout !== 'floating') win.layout = 'floating'
      win.frame = this.#cascade(target.windowIds.length)
      target.windowIds.push(winId)
      this.focus(winId)
    }
    this.#removeStage(source.id)
  }

  // ── Layout changes ─────────────────────────────────────────────────────

  /** Floating windows stay on screen and clear of the Dock (never smaller than the app allows). */
  setFrame(id: string, frame: Frame): void {
    const win = this.windows.find((w) => w.id === id)
    if (!win || win.layout !== 'floating' || this.compact) return
    win.frame = this.#fitFrame(win, frame)
  }

  /** The layouts the multitasking menu should show disabled for a window: those this screen has no room for. */
  unavailableLayouts(id: string): WindowLayout[] {
    const win = this.windows.find((w) => w.id === id)
    if (!win) return []
    // a compact screen shows every window full screen: the menu's other layouts have no room
    if (this.compact) return ['split-left', 'split-right', 'slideover', 'floating']
    return [
      ...(this.#canSplit(win) ? [] : (['split-left', 'split-right'] as const)),
      ...(this.#canSlideOver(win) ? [] : (['slideover'] as const))
    ].filter((layout) => layout !== win.layout) // the layout a window is in is never one it cannot have
  }

  setLayout(id: string, layout: WindowLayout): void {
    const win = this.windows.find((w) => w.id === id)
    if (!win || win.layout === layout || this.compact) return // compact: the layout it has is kept for a regular screen
    if (this.unavailableLayouts(id).includes(layout)) return // no room for it: the layout stays as it is

    if (layout === 'slideover') {
      // Only one Slide Over; an existing one returns to its own stage.
      const current = this.slideOver
      if (current) this.#toOwnStage(current, 'floating')
      this.#detach(win)
      win.layout = 'slideover'
      this.slideOverHidden = false
      return this.focus(win.id)
    }

    if (win.layout === 'slideover') this.#dropSlideOver(win)

    const stage = this.stages.find((s) => s.id === win.stageId)
    if (!stage) return

    if (layout === 'fullscreen') {
      // A full-screen app owns the stage; companions move to the strip.
      for (const other of this.#siblings(win)) this.#toOwnStage(other, 'floating')
      stage.awaitingSplit = null
    }

    if (layout === 'split-left' || layout === 'split-right') {
      const otherSide = layout === 'split-left' ? 'split-right' : 'split-left'
      const siblings = this.#siblings(win)
      // Keep one partner (the most recently focused); others go to the strip.
      const partner = this.#partnerOf(siblings)
      for (const other of siblings) if (other !== partner) this.#toOwnStage(other, 'floating')
      if (partner) partner.layout = otherSide
      stage.awaitingSplit = partner ? null : otherSide
    }

    if (layout === 'floating') {
      // Un-splitting returns the partner to floating too.
      for (const other of this.#siblings(win))
        if (other.layout !== 'floating') other.layout = 'floating'
      stage.awaitingSplit = null
    }

    win.layout = layout
    if (layout.startsWith('split')) stage.splitRatio = this.#clampRatio(stage, 0.5) // an even split, unless a minimum says otherwise
    this.focus(win.id)
  }

  /**
   * Between a quarter and three quarters, and never leaving a pane narrower than its app's minimum (a divider dragged
   * further would cut the app off). The minimums win over the quarter: a 700 px app leaves its neighbour less than that.
   */
  setSplitRatio(ratio: number): void {
    const stage = this.activeStage
    if (stage) stage.splitRatio = this.#clampRatio(stage, ratio)
  }

  /** Split divider dragged to `x` within the workspace. */
  moveSplitDivider(x: number): void {
    this.setSplitRatio((x - INSET) / this.#splitWidth)
  }

  toggleSlideOver(): void {
    this.slideOverHidden = !this.slideOverHidden
  }

  // ── Minimize & close ───────────────────────────────────────────────────

  /** Minimize sends a window to the strip as its own group. */
  minimize(id: string): void {
    const win = this.windows.find((w) => w.id === id)
    if (!win) return
    if (win.layout === 'slideover') {
      this.slideOverHidden = true
      return
    }
    const wasActive = win.stageId === this.activeStageId
    this.#toOwnStage(win, 'floating', Date.now() - 1)
    if (wasActive && this.activeStage && this.activeStage.windowIds.length === 0) {
      this.activeStageId = null
    }
    this.#focusTopmost()
  }

  /**
   * Leave the stage for the Home Screen. Nothing is closed or rearranged: the group that was on stage waits in the
   * recent-apps strip as it is (a split stays a split, first in the strip), a Slide Over steps aside, and no window
   * has the keys.
   */
  showHome(): void {
    const stage = this.activeStage
    if (stage) stage.lastActive = Date.now()
    this.activeStageId = null
    if (this.slideOver) this.slideOverHidden = true
    this.focusedId = null
  }

  // what each window has to finish before it goes (an app's frame telling its page to wrap up)
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping nothing renders from
  #closing = new Map<string, () => Promise<void>>()

  /**
   * Register work a window finishes before it is closed: `close` runs it, and the window goes when it settles. The
   * work must bound itself (the window waits for it). Returns a function that takes the registration back.
   */
  beforeClose(id: string, work: () => Promise<void>): () => void {
    this.#closing.set(id, work)
    return () => {
      if (this.#closing.get(id) === work) this.#closing.delete(id)
    }
  }

  close(id: string): void {
    const work = this.#closing.get(id)
    if (work) {
      // once: a second close while it wraps up, or the close this leads to, removes the window at once
      this.#closing.delete(id)
      void work().then(
        () => this.close(id),
        () => this.close(id)
      )
      return
    }
    const win = this.windows.find((w) => w.id === id)
    if (!win) return
    this.#detach(win)
    this.windows = this.windows.filter((w) => w.id !== id)
    this.#focusTopmost()
  }

  // ── Internals ──────────────────────────────────────────────────────────

  /** Only called for an app `open` has checked. */
  #createWindow(app: AppDefinition): WindowState {
    const frame = this.#cascade(this.onStage.length)
    const size = {
      width: Math.max(app.minWidth, Math.min(app.width, this.bounds.width - frame.x - INSET * 3)),
      height: Math.max(
        app.minHeight,
        Math.min(app.height, this.bounds.height - frame.y - DOCK_SPACE)
      )
    }
    return {
      id: `${app.id}-${uid()}`,
      appId: app.id,
      stageId: null,
      layout: 'floating',
      // an app's minimum can be wider than the room right of the strip: the window moves left rather than past the edge
      frame: this.#onScreen({ ...frame, ...size }),
      z: ++this.#topZ
    }
  }

  /** Default floating frame: centred in the area right of the strip, cascaded. */
  #cascade(index: number): Frame {
    const { width: W, height: H } = this.bounds
    const width = Math.min(900, W - STRIP_WIDTH - 80)
    const height = Math.min(600, H - DOCK_SPACE - 40)
    const x = Math.round(STRIP_WIDTH + (W - STRIP_WIDTH - width) / 2) + index * 36
    const y = Math.max(INSET, Math.round((H - DOCK_SPACE - height) / 2)) + index * 28
    return { x, y, width, height }
  }

  /**
   * Place a frame to the right of the stage's floating windows. When there
   * isn't room, slide those windows left first (into the strip's space — the
   * strip auto-hides when covered) so the new window doesn't hide them.
   */
  #besideStage(frame: Frame): Frame {
    const { width: W, height: H } = this.bounds
    const floating = this.onStage.filter((w) => w.layout === 'floating')
    const rightEdge = (): number =>
      Math.max(INSET, ...floating.map((w) => w.frame.x + w.frame.width))
    const overflow = rightEdge() + GAP + frame.width - (W - INSET)
    if (overflow > 0 && floating.length) {
      const leftmost = Math.min(...floating.map((w) => w.frame.x))
      const shift = Math.min(overflow, leftmost - INSET)
      for (const w of floating) w.frame = { ...w.frame, x: w.frame.x - shift }
    }
    const x = Math.min(rightEdge() + GAP, W - frame.width - INSET)
    const y = Math.max(INSET, Math.round((H - DOCK_SPACE - frame.height) / 2))
    return { ...frame, x, y }
  }

  /** Move a frame back inside the workspace (size is clamped by setFrame). */
  #onScreen(frame: Frame): Frame {
    const { width: W, height: H } = this.#bounds
    return {
      ...frame,
      x: Math.max(INSET, Math.min(frame.x, W - frame.width - INSET)),
      y: Math.max(INSET, Math.min(frame.y, H - frame.height - DOCK_SPACE))
    }
  }

  #siblings(win: WindowState): WindowState[] {
    return this.windows.filter((w) => w.stageId === win.stageId && w.id !== win.id)
  }

  #minOf(win: WindowState): { minWidth: number; minHeight: number } {
    return appOf(win.appId) ?? { minWidth: 0, minHeight: 0 }
  }

  /** `frame` inside the workspace, clear of the Dock and never smaller than the app's minimum (which wins over both). */
  #fitFrame(win: WindowState, frame: Frame): Frame {
    const { minWidth, minHeight } = this.#minOf(win)
    const maxWidth = this.bounds.width - frame.x - INSET
    const maxHeight = this.bounds.height - frame.y - (this.dockHidden ? INSET : DOCK_SPACE)
    return {
      ...frame,
      width: Math.max(minWidth, Math.min(frame.width, maxWidth)),
      height: Math.max(minHeight, Math.min(frame.height, maxHeight))
    }
  }

  // ── Split sizing ──

  /** The window that stays beside the one choosing Split View: the most recently focused. */
  #partnerOf(others: WindowState[]): WindowState | undefined {
    return [...others].sort((a, b) => b.z - a.z)[0]
  }

  /** Whether panes with these minimum widths fit side by side on this workspace. */
  #fits(minWidths: number[]): boolean {
    return minWidths.reduce((sum, w) => sum + w, 0) <= this.#splitWidth
  }

  /**
   * Whether `win` can take a side of a Split View. With a partner already on stage the two minimums must fit; with
   * none yet the second app is not known, so the least any app gets (a 320 px window) stands in for it.
   */
  #canSplit(win: WindowState): boolean {
    const others = win.layout === 'slideover' ? this.onStage : this.#siblings(win)
    const partner = this.#partnerOf(others)
    return this.#fits([
      this.#minOf(win).minWidth,
      partner ? this.#minOf(partner).minWidth : MIN_WIDTH
    ])
  }

  /**
   * Leaving Slide Over: a floating window on the active stage, or on a stage of its own that is put on screen (not in the
   * strip). Nothing else changes: the stage's windows keep their layouts and focus stays where it is, so a screen that
   * shrinks under a Slide Over does not rearrange what is beside it.
   */
  #dropSlideOver(win: WindowState): void {
    win.layout = 'floating'
    const stage = this.activeStage
    if (stage) {
      win.stageId = stage.id
      stage.windowIds.push(win.id)
    } else {
      this.#toOwnStage(win, 'floating')
      this.activeStageId = win.stageId
    }
  }

  /** Whether the app's minimum width fits the screen as a Slide Over (with the inset each side). */
  #canSlideOver(win: WindowState): boolean {
    return this.#minOf(win).minWidth <= this.bounds.width - INSET * 2
  }

  /** 380, or the screen's width where that is less, or the app's minimum where that is more. */
  #slideOverWidth(win: WindowState): number {
    return Math.max(
      Math.min(SLIDE_OVER_WIDTH, this.bounds.width - INSET * 2),
      this.#minOf(win).minWidth
    )
  }

  /** The two panes of a stage's split, by side (a side still waiting for its app is missing). */
  #panes(stage: Stage): { left?: WindowState; right?: WindowState } {
    const inStage = this.windows.filter((w) => w.stageId === stage.id)
    return {
      left: inStage.find((w) => w.layout === 'split-left'),
      right: inStage.find((w) => w.layout === 'split-right')
    }
  }

  /** `ratio` held between a quarter and three quarters, and where a pane's minimum asks for more, at that instead. */
  #clampRatio(stage: Stage, ratio: number): number {
    const { left, right } = this.#panes(stage)
    const needLeft = (left ? this.#minOf(left).minWidth : 0) / this.#splitWidth
    const needRight = (right ? this.#minOf(right).minWidth : 0) / this.#splitWidth
    const lowest = Math.max(needLeft, 0.25)
    const highest = Math.min(1 - needRight, 0.75)
    return lowest <= highest
      ? Math.min(highest, Math.max(lowest, ratio))
      : Math.min(1 - needRight, Math.max(needLeft, ratio)) // the minimums beat the quarter
  }

  /** After the workspace changed: keep the divider within its limits, or dissolve a split that no longer fits. */
  #fitSplit(stage: Stage): void {
    const { left, right } = this.#panes(stage)
    const panes = [left, right].filter((w): w is WindowState => !!w)
    if (panes.length === 0) return
    const minimums = panes.map((w) => this.#minOf(w).minWidth)
    // a pane still waiting for its app is counted at the least any app gets
    if (this.#fits(panes.length === 2 ? minimums : [...minimums, MIN_WIDTH])) {
      stage.splitRatio = this.#clampRatio(stage, stage.splitRatio)
      return
    }
    for (const w of panes) w.layout = 'floating'
    stage.awaitingSplit = null
  }

  /** Remove a window from its stage, deleting the stage if it empties. */
  #detach(win: WindowState): void {
    const stage = this.stages.find((s) => s.id === win.stageId)
    win.stageId = null
    if (!stage) return
    stage.windowIds = stage.windowIds.filter((id) => id !== win.id)
    if (stage.windowIds.length === 0) {
      this.#removeStage(stage.id)
    } else if (stage.windowIds.length === 1) {
      // A lone split partner goes back to floating.
      const last = this.windows.find((w) => w.id === stage.windowIds[0])
      if (last && last.layout.startsWith('split')) last.layout = 'floating'
    }
  }

  #toOwnStage(win: WindowState, layout: WindowLayout, lastActive = Date.now() - 1): void {
    this.#detach(win)
    const stage: Stage = {
      id: uid(),
      windowIds: [win.id],
      splitRatio: 0.5,
      awaitingSplit: null,
      lastActive
    }
    this.stages.push(stage)
    win.stageId = stage.id
    win.layout = layout
  }

  #removeStage(id: string): void {
    this.stages = this.stages.filter((s) => s.id !== id)
    if (this.activeStageId === id) this.activeStageId = null
  }

  #focusTopmost(): void {
    const candidates = this.onStage.length
      ? this.onStage
      : this.windows.filter((w) => w.layout === 'slideover')
    const top = candidates.reduce<WindowState | null>((a, w) => (!a || w.z > a.z ? w : a), null)
    this.focusedId = top?.id ?? null
  }
}

export const windowManager = new WindowManager()
