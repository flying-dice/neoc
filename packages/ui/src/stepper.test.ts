import { describe, expect, test } from 'bun:test'
import { clampStep } from './stepper'

describe('Stepper values', () => {
  test('clamps into the bounds', () => {
    expect(clampStep(12, 0, 10, 1)).toBe(10)
    expect(clampStep(-3, 0, 10, 1)).toBe(0)
    expect(clampStep(5, undefined, undefined, 1)).toBe(5)
  })

  test('snaps to the step grid anchored at min', () => {
    expect(clampStep(7, 0, 100, 5)).toBe(5)
    expect(clampStep(8, 0, 100, 5)).toBe(10)
    expect(clampStep(4, 1, 100, 5)).toBe(6)
  })

  test('a max off the grid lands on the largest value that fits', () => {
    expect(clampStep(10, 0, 10, 3)).toBe(9)
  })

  test('repeated 0.1 steps carry no floating-point noise', () => {
    let v = 0
    for (let i = 0; i < 3; i++) v = clampStep(v + 0.1, 0, 1, 0.1)
    expect(v).toBe(0.3)
  })

  test('nonsense falls back instead of throwing', () => {
    expect(clampStep(Number.NaN, 2, 9, 1)).toBe(2)
    expect(clampStep(Number.NaN, undefined, undefined, 1)).toBe(0)
    expect(clampStep(5, 0, 10, 0)).toBe(5)
    expect(clampStep(5, 0, 10, -2)).toBe(5)
  })
})
