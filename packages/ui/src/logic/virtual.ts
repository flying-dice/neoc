/**
 * DOM virtualization maths for lists of thousands of rows (spec 3.7). Pure: no DOM, no Svelte.
 *
 * A list of `total` rows renders only the rows near the viewport. Every row has a height: `estimate` until the
 * component measures it, then the measured value. With no measurements (fixed-height rows) the maths is O(1);
 * with measurements (rows that wrap) offsets come from a prefix-sum table rebuilt lazily after a change.
 */

export class HeightIndex {
  private measured = new Map<number, number>()
  private prefix: Float64Array | null = null
  private count: number
  private guess: number

  constructor(total: number, estimate: number) {
    this.count = Math.max(0, Math.floor(total))
    this.guess = Math.max(1, estimate)
  }

  get total(): number {
    return this.count
  }
  get estimate(): number {
    return this.guess
  }

  setTotal(total: number): void {
    const next = Math.max(0, Math.floor(total))
    if (next === this.count) return
    this.count = next
    for (const i of [...this.measured.keys()]) if (i >= next) this.measured.delete(i)
    this.prefix = null
  }

  /** The default for rows not measured yet (the first measured row, for a fixed-height list). */
  setEstimate(estimate: number): void {
    const next = Math.max(1, estimate)
    if (next === this.guess) return
    this.guess = next
    this.prefix = null
  }

  /** Record a measured height. Returns true when it changed what the table knows. */
  set(index: number, height: number): boolean {
    if (index < 0 || index >= this.count || !(height > 0)) return false
    const h = Math.round(height * 100) / 100
    const known = this.measured.get(index)
    if (known === h || (known === undefined && h === this.guess)) return false
    this.measured.set(index, h)
    this.prefix = null
    return true
  }

  heightOf(index: number): number {
    return this.measured.get(index) ?? this.guess
  }

  private table(): Float64Array {
    if (!this.prefix) {
      const t = new Float64Array(this.count + 1)
      for (let i = 0; i < this.count; i++) t[i + 1] = t[i] + this.heightOf(i)
      this.prefix = t
    }
    return this.prefix
  }

  /** Distance from the top of the list to the top of row `index` (`index === total` is the full height). */
  offsetOf(index: number): number {
    const i = Math.min(Math.max(0, Math.floor(index)), this.count)
    return this.measured.size === 0 ? i * this.guess : this.table()[i]
  }

  totalHeight(): number {
    return this.offsetOf(this.count)
  }

  /** The row that contains vertical position `y` (clamped to the list). */
  indexAt(y: number): number {
    if (this.count === 0 || y <= 0) return 0
    if (this.measured.size === 0) return Math.min(this.count - 1, Math.floor(y / this.guess))
    const t = this.table()
    if (y >= t[this.count]) return this.count - 1
    let lo = 0
    let hi = this.count - 1
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1
      if (t[mid] <= y) lo = mid
      else hi = mid - 1
    }
    return lo
  }
}

export interface Viewport {
  /** The scroller's scrollTop. */
  scrollTop: number
  /** The scroller's visible height. */
  height: number
  /** Where the list starts inside the scroller's content (other content may sit above it). */
  listOffset: number
}

export interface RowRange {
  /** First row to render. */
  start: number
  /** One past the last row to render. */
  end: number
}

/** The rows to render: those intersecting the viewport, plus `overscan` rows each side. */
export function visibleRange(heights: HeightIndex, view: Viewport, overscan = 6): RowRange {
  const total = heights.total
  if (total === 0) return { start: 0, end: 0 }
  const top = view.scrollTop - view.listOffset
  const bottom = top + view.height
  if (bottom < 0 || top > heights.totalHeight()) {
    // The list is off screen: render a short stub at the nearest edge so there is something to hold.
    const edge = top > 0 ? Math.max(0, total - overscan) : 0
    return { start: edge, end: Math.min(total, edge + overscan) }
  }
  const first = heights.indexAt(Math.max(0, top))
  const last = heights.indexAt(Math.max(0, bottom))
  return { start: Math.max(0, first - overscan), end: Math.min(total, last + 1 + overscan) }
}

/**
 * The window of the source to ask the host for: the rendered rows plus one page each side, aligned to page
 * boundaries so a slow scroll asks for the same window many times (the caller dedupes) and a fast one coalesces.
 */
export function windowFor(
  range: RowRange,
  total: number,
  page: number
): { from: number; to: number } {
  const size = Math.max(1, page)
  const from = Math.max(0, Math.floor(range.start / size) * size - size)
  const to = Math.min(total, Math.ceil(range.end / size) * size + size)
  return { from, to: Math.max(from, to) }
}

/** True when every row of `range` is inside the resident window `[from, from + count)`. */
export function covered(range: RowRange, resident: { from: number; count: number }): boolean {
  return (
    range.end <= range.start ||
    (range.start >= resident.from && range.end <= resident.from + resident.count)
  )
}

/** The row to hold still while rows change above it (an agent adds a note): its key and index in the old window. */
export interface Anchor<K> {
  key: K
  index: number
}

export function anchorOf<K>(keys: readonly K[], from: number, firstVisible: number): Anchor<K> | null {
  const key = keys[firstVisible - from]
  return key === undefined ? null : { key, index: firstVisible }
}

/**
 * How far to move the scroller so the anchor row stays where it was: the distance its row index moved, at the
 * current heights. Null when it did not move or left the window.
 */
export function anchorShift<K>(
  anchor: Anchor<K>,
  keys: readonly K[],
  from: number,
  heights: HeightIndex
): number | null {
  const at = keys.indexOf(anchor.key)
  if (at < 0) return null
  const delta = heights.offsetOf(from + at) - heights.offsetOf(anchor.index)
  return delta === 0 ? null : delta
}
