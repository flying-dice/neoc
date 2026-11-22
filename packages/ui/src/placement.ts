/**
 * Where a context menu goes: beside the thing it was opened on, wholly inside the screen. A pure function, so the
 * component only measures and the rule is tested without a DOM.
 */
import type { AnchorRect } from './types'

export interface Placement {
  left: number
  top: number
  /** The side of the anchor the menu is on: it grows from there. */
  side: 'below' | 'above'
}

/** The air between the anchor and the menu, and between the menu and the edge of the screen. */
export const MENU_GAP = 8

/**
 * A menu of `size` beside `anchor`, inside `bounds` (all three in the same coordinates): below the anchor and centred
 * on it, above when there is no room below (and more above), then moved sideways and vertically to stay `MENU_GAP`
 * from every edge. A menu bigger than the bounds starts at the top left gap.
 */
export function placeMenu(
  anchor: AnchorRect,
  size: { width: number; height: number },
  bounds: { width: number; height: number }
): Placement {
  const below = bounds.height - (anchor.top + anchor.height) - MENU_GAP * 2
  const above = anchor.top - MENU_GAP * 2
  const side = size.height <= below || below >= above ? 'below' : 'above'
  const wantTop =
    side === 'below' ? anchor.top + anchor.height + MENU_GAP : anchor.top - MENU_GAP - size.height
  const wantLeft = anchor.left + anchor.width / 2 - size.width / 2
  const clamp = (value: number, room: number): number =>
    Math.max(MENU_GAP, Math.min(value, room - MENU_GAP))
  return {
    left: Math.round(clamp(wantLeft, bounds.width - size.width)),
    top: Math.round(clamp(wantTop, bounds.height - size.height)),
    side
  }
}
