import { describe, expect, test } from 'bun:test'
import { FIT_MIN, fitScale } from './text'

describe('Text fit scale', () => {
  test('a readout that fits stays at full size', () => {
    expect(fitScale(100, 200)).toBe(1)
    expect(fitScale(200, 200)).toBe(1)
  })

  test('a wider readout shrinks in proportion, always a little under so no glyph is clipped or ellipsised', () => {
    const s = fitScale(400, 200)
    expect(s).toBeLessThan(0.5)
    expect(s).toBeGreaterThan(0.48)
    expect(400 * s).toBeLessThanOrEqual(200)
  })

  test('there is a floor: past it the readout truncates instead of shrinking further', () => {
    expect(fitScale(10_000, 100)).toBe(FIT_MIN)
  })

  test('nonsense measurements leave it alone', () => {
    expect(fitScale(0, 100)).toBe(1)
    expect(fitScale(100, 0)).toBe(1)
    expect(fitScale(Number.NaN, 100)).toBe(1)
  })
})
