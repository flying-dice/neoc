import { describe, expect, test } from 'bun:test'
import {
  isHorizontalDrag,
  pullArmed,
  pullDistance,
  restingOffset,
  swipeOffset,
  swipeSettle,
  type SwipeGeometry
} from './gestures'

const g: SwipeGeometry = {
  leading: 80,
  trailing: 160,
  row: 400,
  fullSwipe: { leading: false, trailing: true }
}

describe('swipe offset', () => {
  test('follows the finger up to the action widths', () => {
    expect(swipeOffset(0, -100, g)).toBe(-100)
    expect(swipeOffset(0, 60, g)).toBe(60)
    expect(swipeOffset(-160, 40, g)).toBe(-120) // closing from open
  })

  test('rubber bands past the widths, and is capped at the row for a full swipe', () => {
    const over = swipeOffset(0, -260, g)
    expect(over).toBeLessThan(-160)
    expect(over).toBeGreaterThan(-260)
    expect(swipeOffset(0, -9999, g)).toBe(-400)
    expect(swipeOffset(0, 9999, g)).toBeLessThanOrEqual(80 + 40) // no full swipe leading: capped near the width
  })

  test('a side without actions does not move', () => {
    expect(swipeOffset(0, 50, { ...g, leading: 0 })).toBe(0)
    expect(swipeOffset(0, -50, { ...g, trailing: 0 })).toBe(0)
  })
})

describe('swipe settle', () => {
  test('opens past half the width, closes before it', () => {
    expect(swipeSettle(-100, 0, g)).toEqual({ open: 'trailing', full: null })
    expect(swipeSettle(-60, 0, g)).toEqual({ open: 'none', full: null })
    expect(swipeSettle(50, 0, g)).toEqual({ open: 'leading', full: null })
  })

  test('a flick decides by direction', () => {
    expect(swipeSettle(-30, -0.8, g).open).toBe('trailing')
    expect(swipeSettle(-120, 0.8, g).open).toBe('none')
  })

  test('a full swipe past 60% of the row runs the outer action', () => {
    expect(swipeSettle(-300, 0, g)).toEqual({ open: 'none', full: 'trailing' })
    expect(swipeSettle(300, 0, g)).toEqual({ open: 'leading', full: null }) // leading has no full swipe
  })

  test('resting offsets', () => {
    expect(restingOffset('none', g)).toBe(0)
    expect(restingOffset('leading', g)).toBe(80)
    expect(restingOffset('trailing', g)).toBe(-160)
  })

  test('a drag becomes a swipe only when clearly horizontal', () => {
    expect(isHorizontalDrag(20, 2)).toBe(true)
    expect(isHorizontalDrag(5, 0)).toBe(false)
    expect(isHorizontalDrag(20, 30)).toBe(false) // scrolling
  })
})

describe('pull to refresh', () => {
  test('resists and caps', () => {
    expect(pullDistance(-10)).toBe(0)
    expect(pullDistance(100)).toBe(50)
    expect(pullDistance(1000)).toBe(120)
  })

  test('arms at the threshold', () => {
    expect(pullArmed(pullDistance(100))).toBe(false)
    expect(pullArmed(pullDistance(140))).toBe(true)
  })
})
