/**
 * Gesture maths: swipe actions on a row and pull to refresh. Pure; the components feed it pointer deltas.
 */

export type SwipeSide = 'leading' | 'trailing'
export type SwipeOpen = SwipeSide | 'none'

export interface SwipeGeometry {
  /** Width of the revealed leading and trailing action groups (0 = none). */
  leading: number
  trailing: number
  /** The row's width: a swipe past 60% of it triggers the first action ("full swipe"). */
  row: number
  /** Whether the outermost action runs on a full swipe. */
  fullSwipe: { leading: boolean; trailing: boolean }
}

/** The row's x offset while dragging: a rubber band past the action widths, none where there are no actions. */
export function swipeOffset(startOffset: number, dx: number, g: SwipeGeometry): number {
  const raw = startOffset + dx
  const band = (over: number): number => Math.sqrt(Math.max(0, over)) * 3 // resistance past the end
  if (raw > 0) {
    if (g.leading === 0) return 0
    const limit = g.fullSwipe.leading ? g.row : g.leading
    return raw <= g.leading ? raw : Math.min(g.leading + band(raw - g.leading), limit)
  }
  if (g.trailing === 0) return 0
  const limit = g.fullSwipe.trailing ? g.row : g.trailing
  return -(-raw <= g.trailing ? -raw : Math.min(g.trailing + band(-raw - g.trailing), limit))
}

export interface SwipeSettle {
  open: SwipeOpen
  /** The outermost action should run (a full swipe). */
  full: SwipeSide | null
}

/** Where a released swipe comes to rest. `velocity` is px/ms (negative = leftwards). */
export function swipeSettle(offset: number, velocity: number, g: SwipeGeometry): SwipeSettle {
  const fast = Math.abs(velocity) > 0.5
  if (offset < 0) {
    if (g.fullSwipe.trailing && -offset > g.row * 0.6) return { open: 'none', full: 'trailing' }
    const open = fast ? velocity < 0 : -offset > g.trailing / 2
    return { open: open ? 'trailing' : 'none', full: null }
  }
  if (offset > 0) {
    if (g.fullSwipe.leading && offset > g.row * 0.6) return { open: 'none', full: 'leading' }
    const open = fast ? velocity > 0 : offset > g.leading / 2
    return { open: open ? 'leading' : 'none', full: null }
  }
  return { open: 'none', full: null }
}

/** The resting x offset of an open state. */
export const restingOffset = (open: SwipeOpen, g: SwipeGeometry): number =>
  open === 'leading' ? g.leading : open === 'trailing' ? -g.trailing : 0

/** A horizontal drag has started once it moves 8 px and is more horizontal than vertical. */
export function isHorizontalDrag(dx: number, dy: number): boolean {
  return Math.abs(dx) > 8 && Math.abs(dx) > Math.abs(dy) * 1.2
}

export const PULL_THRESHOLD = 64
const PULL_MAX = 120

/** How far the content follows a downward pull (rubber band: halves, then flattens). */
export function pullDistance(dy: number): number {
  if (dy <= 0) return 0
  return Math.min(PULL_MAX, dy * 0.5)
}

export const pullArmed = (distance: number): boolean => distance >= PULL_THRESHOLD
