/**
 * Stepper value handling: clamp into `[min, max]` and snap to the step grid anchored at `min` (or 0), with the
 * floating-point noise of repeated `0.1` steps removed. Pure, tested without a DOM.
 */

const decimals = (n: number): number => {
  const s = String(n)
  const dot = s.indexOf('.')
  return dot === -1 ? 0 : s.length - dot - 1
}

export function clampStep(value: number, min: number | undefined, max: number | undefined, step: number): number {
  if (!Number.isFinite(value)) return min ?? 0
  const s = Number.isFinite(step) && step > 0 ? step : 1
  const origin = min ?? 0
  const snapped = origin + Math.round((value - origin) / s) * s
  const places = Math.max(decimals(s), decimals(origin))
  let v = Number(snapped.toFixed(places))
  if (min !== undefined && v < min) v = min
  if (max !== undefined && v > max) {
    // The largest on-grid value that fits under `max`.
    v = Number((origin + Math.floor((max - origin) / s) * s).toFixed(places))
  }
  return v
}
