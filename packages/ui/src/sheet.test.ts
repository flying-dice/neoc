import { describe, expect, test } from 'bun:test'
import { detentHeight, nearestDetent, normaliseDetents, shouldDismiss, stepDetent } from './sheet'

describe('Sheet detents', () => {
  test('heights grow small < medium < large and never exceed the window minus the gap', () => {
    for (const h of [240, 400, 720, 1200]) {
      const [s, m, l] = (['small', 'medium', 'large'] as const).map((d) => detentHeight(d, h)) as [number, number, number]
      expect(s).toBeLessThanOrEqual(m)
      expect(m).toBeLessThanOrEqual(l)
      expect(l).toBeLessThanOrEqual(Math.max(120, h - 44))
    }
    expect(detentHeight('large', 720)).toBe(676)
  })

  test('a minimum window still gets a usable small sheet', () => {
    expect(detentHeight('small', 240)).toBeGreaterThanOrEqual(160)
    expect(detentHeight('small', 240)).toBeLessThanOrEqual(196)
  })

  test('the detent list is normalised: known, unique, smallest first, default large', () => {
    expect(normaliseDetents(['large', 'small', 'large', 'huge'])).toEqual(['small', 'large'])
    expect(normaliseDetents([])).toEqual(['large'])
    expect(normaliseDetents(undefined)).toEqual(['large'])
    expect(normaliseDetents('medium')).toEqual(['large'])
  })

  test('a drag settles on the nearest detent', () => {
    const all = ['small', 'medium', 'large'] as const
    expect(nearestDetent(all, 150, 720)).toBe('small')
    expect(nearestDetent(all, 400, 720)).toBe('medium')
    expect(nearestDetent(all, 700, 720)).toBe('large')
  })

  test('stepping goes up and down, and wraps only when asked', () => {
    const all = ['small', 'medium', 'large'] as const
    expect(stepDetent(all, 'small', 1)).toBe('medium')
    expect(stepDetent(all, 'large', 1)).toBe('large')
    expect(stepDetent(all, 'large', 1, true)).toBe('small')
    expect(stepDetent(all, 'small', -1)).toBe('small')
    expect(stepDetent(['medium', 'large'], 'small' as never, 1)).toBe('large')
  })

  test('dragging well below the smallest detent dismisses', () => {
    expect(shouldDismiss(60, ['small', 'large'], 720)).toBe(true)
    expect(shouldDismiss(200, ['small', 'large'], 720)).toBe(false)
  })
})
