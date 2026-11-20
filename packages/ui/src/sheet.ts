/**
 * Sheet detents: the heights a bottom sheet can rest at. `small` and `medium` are fractions of the window (with a
 * floor, so a 320x240 window still shows something usable), `large` is the window minus the top gap. Pure logic
 * for the resize grabber (drag, click, arrow keys), tested without a DOM.
 */

export type Detent = 'small' | 'medium' | 'large'

export const DETENT_ORDER: readonly Detent[] = ['small', 'medium', 'large']

/** Space kept above a `large` sheet so the window behind still peeks out. */
export const TOP_GAP = 44

export function detentHeight(detent: Detent, containerHeight: number): number {
  const large = Math.max(120, containerHeight - TOP_GAP)
  switch (detent) {
    case 'large':
      return large
    case 'medium':
      return Math.min(large, Math.max(200, Math.round(containerHeight * 0.55)))
    case 'small':
      return Math.min(large, Math.max(160, Math.round(containerHeight * 0.3)))
  }
}

/** Keep only known detents, once each, smallest first. An empty or invalid list is `['large']`. */
export function normaliseDetents(value: unknown): Detent[] {
  const list = Array.isArray(value) ? value : []
  const wanted = DETENT_ORDER.filter((d) => list.includes(d))
  return wanted.length ? [...wanted] : ['large']
}

/** The detent whose height is closest to `height` (the release point of a drag). */
export function nearestDetent(detents: readonly Detent[], height: number, containerHeight: number): Detent {
  let best = detents[0]!
  for (const d of detents)
    if (Math.abs(detentHeight(d, containerHeight) - height) < Math.abs(detentHeight(best, containerHeight) - height))
      best = d
  return best
}

/** The next detent up or down; `wrap` goes from the largest back to the smallest (a click on the grabber). */
export function stepDetent(detents: readonly Detent[], current: Detent, dir: 1 | -1, wrap = false): Detent {
  const i = Math.max(0, detents.indexOf(current))
  const next = i + dir
  if (next >= detents.length) return wrap ? detents[0]! : detents[detents.length - 1]!
  if (next < 0) return detents[0]!
  return detents[next]!
}

/** A drag that ends this far below the smallest detent dismisses the sheet (when it can be dismissed). */
export const DISMISS_DRAG = 72

export const shouldDismiss = (height: number, detents: readonly Detent[], containerHeight: number): boolean =>
  height < detentHeight(detents[0]!, containerHeight) - DISMISS_DRAG
