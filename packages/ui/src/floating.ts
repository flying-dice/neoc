/**
 * Placement of a floating surface (popover, menu, context menu) next to an anchor, inside a container. Pure
 * geometry, tested without a DOM: prefer the requested edge, flip to the opposite one when it does not fit, then
 * clamp into the container so the surface is never clipped (the window minimum is 320x240).
 */

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export type Edge = 'top' | 'bottom' | 'leading' | 'trailing'

export interface Placement {
  /** Top-left of the surface, relative to the container. */
  x: number
  y: number
  /** The edge of the anchor it ended up on (after any flip). */
  edge: Edge
  /** Offset of the arrow along the surface's edge, from its start, clamped inside the corners. */
  arrow: number
  /** Largest size that fits, so the surface can scroll instead of overflowing. */
  maxWidth: number
  maxHeight: number
}

const OPPOSITE: Record<Edge, Edge> = {
  top: 'bottom',
  bottom: 'top',
  leading: 'trailing',
  trailing: 'leading'
}

/** Where the surface goes. `edge` is where it sits relative to the anchor ("bottom" = below the anchor). */
export function place(
  anchor: Rect,
  size: { width: number; height: number },
  container: { width: number; height: number },
  edge: Edge = 'bottom',
  gap = 8,
  margin = 8,
  rtl = false
): Placement {
  // `leading` is the left in left-to-right text.
  const physical = (e: Edge): 'top' | 'bottom' | 'left' | 'right' =>
    e === 'leading' ? (rtl ? 'right' : 'left') : e === 'trailing' ? (rtl ? 'left' : 'right') : e

  const room = (e: Edge): number => {
    switch (physical(e)) {
      case 'top':
        return anchor.y - margin
      case 'bottom':
        return container.height - (anchor.y + anchor.height) - margin
      case 'left':
        return anchor.x - margin
      case 'right':
        return container.width - (anchor.x + anchor.width) - margin
    }
  }
  const need = (e: Edge): number =>
    (physical(e) === 'top' || physical(e) === 'bottom' ? size.height : size.width) + gap

  let chosen = edge
  if (room(edge) < need(edge) && room(OPPOSITE[edge]) > room(edge)) chosen = OPPOSITE[edge]

  const side = physical(chosen)
  let x: number
  let y: number
  if (side === 'bottom' || side === 'top') {
    x = anchor.x + anchor.width / 2 - size.width / 2
    y = side === 'bottom' ? anchor.y + anchor.height + gap : anchor.y - size.height - gap
  } else {
    y = anchor.y + anchor.height / 2 - size.height / 2
    x = side === 'right' ? anchor.x + anchor.width + gap : anchor.x - size.width - gap
  }
  const maxWidth = Math.max(0, container.width - margin * 2)
  const maxHeight = Math.max(0, container.height - margin * 2)
  const w = Math.min(size.width, maxWidth)
  const h = Math.min(size.height, maxHeight)
  x = Math.min(Math.max(x, margin), Math.max(margin, container.width - w - margin))
  y = Math.min(Math.max(y, margin), Math.max(margin, container.height - h - margin))

  const vertical = side === 'bottom' || side === 'top'
  const centre = vertical ? anchor.x + anchor.width / 2 - x : anchor.y + anchor.height / 2 - y
  const length = vertical ? w : h
  const corner = 16
  const arrow = Math.min(Math.max(centre, corner), Math.max(corner, length - corner))
  return { x, y, edge: chosen, arrow, maxWidth, maxHeight }
}

/** A context menu opens at a point (the pointer, or the focused element's centre for the keyboard). */
export function placeAtPoint(
  point: { x: number; y: number },
  size: { width: number; height: number },
  container: { width: number; height: number },
  margin = 8
): { x: number; y: number } {
  const x = point.x + size.width + margin > container.width ? point.x - size.width : point.x
  const y = point.y + size.height + margin > container.height ? point.y - size.height : point.y
  return {
    x: Math.min(Math.max(x, margin), Math.max(margin, container.width - size.width - margin)),
    y: Math.min(Math.max(y, margin), Math.max(margin, container.height - size.height - margin))
  }
}

/** Is a submenu better opened on the left of its parent? True when it would overflow the container's right. */
export const submenuFlips = (
  parent: Rect,
  submenuWidth: number,
  containerWidth: number,
  margin = 8
): boolean => parent.x + parent.width + submenuWidth + margin > containerWidth
