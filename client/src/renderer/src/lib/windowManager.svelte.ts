import type { Frame, StageSummary, WindowLayout } from '@agentos/ui'
import { apps, type AppId } from './apps.svelte'

/**
 * Stage Manager–style window manager.
 *
 *  • Windows belong to a *stage* (an app group). One stage is on screen; the
 *    rest sit in the recent-apps strip, most recent first.
 *  • A window's layout is floating, full screen, split left/right, or slide
 *    over. Non-floating frames are derived from the workspace bounds, so they
 *    re-flow on resize; floating windows keep their own frame.
 *  • Slide Over lives outside stages and floats above everything.
 */

export interface WindowState {
  id: string
  appId: AppId
  /** The window kind an installed app declared (`app.window("main", ...)`). */
  kind: string
  /** The deep link the app's `onOpen(w, params)` receives. */
  params?: Record<string, unknown>
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

const uid = (): string => crypto.randomUUID().slice(0, 8)

class WindowManager {
  windows = $state<WindowState[]>([])
  stages = $state<Stage[]>([])
  activeStageId = $state<string | null>(null)
  focusedId = $state<string | null>(null)
  slideOverHidden = $state(false)
  #bounds = $state({ width: 1280, height: 800 })
  #topZ = 10

  /** The workspace size. When it changes, floating windows are kept on screen. */
  get bounds(): { width: number; height: number } {
    return this.#bounds
  }
  set bounds(next: { width: number; height: number }) {
    if (next.width === this.#bounds.width && next.height === this.#bounds.height) return
    this.#bounds = next
    for (const win of this.windows)
      if (win.layout === 'floating') this.setFrame(win.id, this.#onScreen(win.frame))
  }

  // ── Queries ────────────────────────────────────────────────────────────

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
          .map((w) => ({
            id: w.appId,
            name: apps[w.appId].name,
            icon: apps[w.appId].icon,
            background: apps[w.appId].background
          }))
      }))
  }

  /** Full screen or split windows take the whole stage: hide strip and dock. */
  get immersive(): boolean {
    return this.onStage.some((w) => w.layout !== 'floating')
  }

  /** The dock hides when immersive — except while a split waits for its second app. */
  get dockHidden(): boolean {
    return this.immersive && !this.activeStage?.awaitingSplit
  }

  /** Strip auto-hides when immersive or a floating window overlaps it. */
  get stripObscured(): boolean {
    return this.immersive || this.onStage.some((w) => w.frame.x < STRIP_WIDTH - 10)
  }

  isOpen(appId: AppId): boolean {
    return this.windows.some((w) => w.appId === appId)
  }

  /** The on-screen frame for a window, derived from its layout. */
  frameOf(win: WindowState): Frame {
    const { width: W, height: H } = this.bounds
    const fullH = H - INSET * 2
    switch (win.layout) {
      case 'fullscreen':
        return { x: INSET, y: INSET, width: W - INSET * 2, height: fullH }
      case 'split-left':
      case 'split-right': {
        const ratio = this.stages.find((s) => s.id === win.stageId)?.splitRatio ?? 0.5
        const usable = W - INSET * 2 - GAP
        const left = Math.round(usable * ratio)
        return win.layout === 'split-left'
          ? { x: INSET, y: INSET, width: left, height: fullH }
          : { x: INSET + left + GAP, y: INSET, width: usable - left, height: fullH }
      }
      case 'slideover':
        return this.slideOverHidden
          ? { x: W + 20, y: INSET, width: SLIDE_OVER_WIDTH, height: fullH - DOCK_SPACE + 40 }
          : {
              x: W - SLIDE_OVER_WIDTH - INSET,
              y: INSET,
              width: SLIDE_OVER_WIDTH,
              height: fullH - DOCK_SPACE + 40
            }
      default:
        return win.frame
    }
  }

  /** x position of the split divider, when the active stage is split. */
  get splitDividerX(): number | null {
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
   * looking at rather than replacing it.
   */
  open(
    appId: AppId,
    options: { alongside?: boolean; kind?: string; params?: Record<string, unknown> } = {}
  ): void {
    const existing = this.windows.find((w) => w.appId === appId)
    if (existing) {
      if (existing.layout === 'slideover') {
        this.slideOverHidden = false
        return this.focus(existing.id)
      }
      if (existing.stageId) this.activateStage(existing.stageId)
      return this.focus(existing.id)
    }

    const win = this.#createWindow(appId, options.kind ?? 'main', options.params)
    const stage = this.activeStage

    // Filling the empty side of a pending Split View.
    if (stage?.awaitingSplit) {
      win.layout = stage.awaitingSplit
      win.stageId = stage.id
      stage.windowIds.push(win.id)
      stage.awaitingSplit = null
      this.windows.push(win)
      return this.focus(win.id)
    }

    // Joining the current group, placed beside the existing windows.
    if (options.alongside && stage && stage.windowIds.length) {
      win.stageId = stage.id
      win.frame = this.#besideStage(win.frame)
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
    if (!win || win.layout !== 'floating') return
    const { minWidth, minHeight } = apps[win.appId]
    const maxWidth = this.bounds.width - frame.x - INSET
    const maxHeight = this.bounds.height - frame.y - (this.dockHidden ? INSET : DOCK_SPACE)
    win.frame = {
      ...frame,
      width: Math.max(minWidth, Math.min(frame.width, maxWidth)),
      height: Math.max(minHeight, Math.min(frame.height, maxHeight))
    }
  }

  setLayout(id: string, layout: WindowLayout): void {
    const win = this.windows.find((w) => w.id === id)
    if (!win || win.layout === layout) return

    if (layout === 'slideover') {
      // Only one Slide Over; an existing one returns to its own stage.
      const current = this.slideOver
      if (current) this.#toOwnStage(current, 'floating')
      this.#detach(win)
      win.layout = 'slideover'
      this.slideOverHidden = false
      return this.focus(win.id)
    }

    // Leaving Slide Over: join the active stage (or a new one).
    if (win.layout === 'slideover') {
      win.layout = 'floating'
      const stage = this.activeStage
      if (stage) {
        win.stageId = stage.id
        stage.windowIds.push(win.id)
      } else this.#toOwnStage(win, 'floating')
    }

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
      const partner = siblings.sort((a, b) => b.z - a.z)[0]
      for (const other of siblings) if (other !== partner) this.#toOwnStage(other, 'floating')
      if (partner) partner.layout = otherSide
      stage.awaitingSplit = partner ? null : otherSide
      stage.splitRatio = 0.5
    }

    if (layout === 'floating') {
      // Un-splitting returns the partner to floating too.
      for (const other of this.#siblings(win))
        if (other.layout !== 'floating') other.layout = 'floating'
      stage.awaitingSplit = null
    }

    win.layout = layout
    this.focus(win.id)
  }

  setSplitRatio(ratio: number): void {
    const stage = this.activeStage
    if (stage) stage.splitRatio = Math.min(0.75, Math.max(0.25, ratio))
  }

  /** Split divider dragged to `x` within the workspace. */
  moveSplitDivider(x: number): void {
    this.setSplitRatio((x - INSET) / (this.bounds.width - INSET * 2 - GAP))
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

  close(id: string): void {
    const win = this.windows.find((w) => w.id === id)
    if (!win) return
    this.#detach(win)
    this.windows = this.windows.filter((w) => w.id !== id)
    this.#focusTopmost()
  }

  // ── Internals ──────────────────────────────────────────────────────────

  #createWindow(appId: AppId, kind: string, params?: Record<string, unknown>): WindowState {
    const app = apps[appId]
    const frame = this.#cascade(this.onStage.length)
    return {
      id: `${appId}-${uid()}`,
      appId,
      kind,
      params,
      stageId: null,
      layout: 'floating',
      frame: {
        ...frame,
        width: Math.max(app.minWidth, Math.min(app.width, this.bounds.width - frame.x - INSET * 3)),
        height: Math.max(
          app.minHeight,
          Math.min(app.height, this.bounds.height - frame.y - DOCK_SPACE)
        )
      },
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
