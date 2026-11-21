/**
 * Text `fit` (the Calculator display behaviour): the font scale that makes a readout of `natural` width (at the
 * style's own size) fit `available` width. 1 when it already fits, shrinking in proportion down to `min`; below
 * that the readout truncates with an ellipsis instead of shrinking further, and never clips a glyph.
 */
export const FIT_MIN = 0.3

export function fitScale(natural: number, available: number, min = FIT_MIN): number {
  if (!(natural > 0) || !(available > 0)) return 1
  const exact = available / natural
  if (exact >= 1) return 1
  // Under the exact ratio: scrollWidth is a whole number and glyph widths do not scale in exact proportion with the
  // font size, so a sub-pixel overshoot would still end the readout in an ellipsis ("123,456,78…"). 1% is cheap.
  return Math.max(min, Math.floor(exact * 1000) / 1000 - 0.01)
}
