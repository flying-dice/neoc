/** Grid track lists: the schema's `GridColumn {fixed | flex | min,max}` or a plain count, as CSS. Pure. */

export interface GridTrack {
  fixed?: number
  flex?: number
  min?: number
  max?: number
}

const px = (n: number): string => `${Math.max(0, Math.round(n))}px`
const fr = (n: number): string => `${Math.max(0, n)}fr`

/**
 * `3` -> `repeat(3, minmax(0, 1fr))` (equal columns that may shrink: nothing forces a horizontal scroll);
 * a list maps track by track. Returns undefined when nothing usable was given (the grid then auto-flows).
 */
export function gridTemplate(spec: unknown): string | undefined {
  if (typeof spec === 'number' && Number.isFinite(spec) && spec >= 1)
    return `repeat(${Math.min(Math.floor(spec), 24)}, minmax(0, 1fr))`
  if (!Array.isArray(spec) || spec.length === 0) return undefined
  return spec
    .slice(0, 24)
    .map((t: GridTrack) => {
      if (typeof t.fixed === 'number') return px(t.fixed)
      if (typeof t.flex === 'number') return `minmax(0, ${fr(t.flex)})`
      if (typeof t.min === 'number' || typeof t.max === 'number')
        return `minmax(${px(t.min ?? 0)}, ${typeof t.max === 'number' ? px(t.max) : '1fr'})`
      return 'minmax(0, 1fr)'
    })
    .join(' ')
}

/** `span`, `rowStart`, `colStart` of a grid child as CSS (1-based, clamped to sane values). */
export function cellPlacement(c: { span?: unknown; rowStart?: unknown; colStart?: unknown }): {
  column?: string
  row?: string
  /** Columns spanned (1 when it does not span). */
  span: number
} {
  const int = (v: unknown, max: number): number | undefined =>
    typeof v === 'number' && Number.isFinite(v) && v >= 1 ? Math.min(Math.floor(v), max) : undefined
  const span = int(c.span, 24)
  const col = int(c.colStart, 24)
  const row = int(c.rowStart, 1000)
  return {
    column: col !== undefined || span ? `${col ?? 'auto'} / span ${span ?? 1}` : undefined,
    row: row !== undefined ? `${row}` : undefined,
    span: span ?? 1
  }
}
