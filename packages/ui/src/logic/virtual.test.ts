import { describe, expect, test } from 'bun:test'
import {
  anchorOf,
  anchorShift,
  covered,
  HeightIndex,
  scrollTopFor,
  visibleRange,
  windowFor
} from './virtual'

describe('HeightIndex', () => {
  test('fixed rows: offsets and lookups are arithmetic', () => {
    const h = new HeightIndex(5000, 44)
    expect(h.totalHeight()).toBe(220_000)
    expect(h.offsetOf(10)).toBe(440)
    expect(h.indexAt(439)).toBe(9)
    expect(h.indexAt(440)).toBe(10)
    expect(h.indexAt(1e9)).toBe(4999)
    expect(h.indexAt(-5)).toBe(0)
  })

  test('measured rows move everything below them', () => {
    const h = new HeightIndex(100, 40)
    expect(h.set(2, 80)).toBe(true)
    expect(h.set(2, 80)).toBe(false)
    expect(h.offsetOf(2)).toBe(80)
    expect(h.offsetOf(3)).toBe(160)
    expect(h.totalHeight()).toBe(99 * 40 + 80)
    expect(h.indexAt(100)).toBe(2)
    expect(h.indexAt(159)).toBe(2)
    expect(h.indexAt(160)).toBe(3)
  })

  test('a measurement equal to the estimate changes nothing', () => {
    const h = new HeightIndex(10, 44)
    expect(h.set(3, 44)).toBe(false)
    expect(h.set(3, 0)).toBe(false)
    expect(h.set(99, 50)).toBe(false)
  })

  test('shrinking the total forgets measurements past the end', () => {
    const h = new HeightIndex(10, 10)
    h.set(8, 30)
    h.setTotal(5)
    expect(h.totalHeight()).toBe(50)
    h.setTotal(10)
    expect(h.heightOf(8)).toBe(10)
  })

  test('an empty list is height 0 and indexes 0', () => {
    const h = new HeightIndex(0, 44)
    expect(h.totalHeight()).toBe(0)
    expect(h.indexAt(100)).toBe(0)
  })
})

describe('visibleRange', () => {
  const heights = new HeightIndex(5000, 44)

  test('renders the viewport plus overscan, never the whole list', () => {
    const r = visibleRange(heights, { scrollTop: 0, height: 440, listOffset: 0 }, 4)
    expect(r).toEqual({ start: 0, end: 15 })
    expect(r.end - r.start).toBeLessThan(30)
  })

  test('follows the scroll position', () => {
    const r = visibleRange(heights, { scrollTop: 44_000, height: 440, listOffset: 0 }, 4)
    expect(r.start).toBe(996)
    expect(r.end).toBe(1015)
  })

  test('content above the list pushes the window down', () => {
    // 200 px of header above the list: scrolling 200 px shows row 0 at the top.
    const r = visibleRange(heights, { scrollTop: 200, height: 440, listOffset: 200 }, 0)
    expect(r.start).toBe(0)
    expect(r.end).toBe(11)
  })

  test('the end of the list clamps', () => {
    const r = visibleRange(heights, { scrollTop: 219_800, height: 440, listOffset: 0 }, 6)
    expect(r.end).toBe(5000)
  })

  test('a list scrolled out of view keeps a short stub', () => {
    const r = visibleRange(heights, { scrollTop: 0, height: 100, listOffset: 5000 }, 3)
    expect(r).toEqual({ start: 0, end: 3 })
  })

  test('empty', () => {
    expect(visibleRange(new HeightIndex(0, 44), { scrollTop: 0, height: 100, listOffset: 0 })).toEqual({
      start: 0,
      end: 0
    })
  })
})

describe('windowFor and covered', () => {
  test('aligns to pages and adds one page each side', () => {
    expect(windowFor({ start: 130, end: 160 }, 5000, 60)).toEqual({ from: 60, to: 240 })
    expect(windowFor({ start: 0, end: 20 }, 5000, 60)).toEqual({ from: 0, to: 120 })
    expect(windowFor({ start: 4990, end: 5000 }, 5000, 60)).toEqual({ from: 4920, to: 5000 })
  })

  test('the same window is asked for while scrolling inside a page', () => {
    const a = windowFor({ start: 130, end: 160 }, 5000, 60)
    const b = windowFor({ start: 140, end: 170 }, 5000, 60)
    expect(a).toEqual(b)
  })

  test('covered says when the resident window already holds the rows', () => {
    expect(covered({ start: 10, end: 30 }, { from: 0, count: 60 })).toBe(true)
    expect(covered({ start: 50, end: 70 }, { from: 0, count: 60 })).toBe(false)
    expect(covered({ start: 5, end: 5 }, { from: 100, count: 0 })).toBe(true)
  })
})

describe('scroll anchoring', () => {
  test('an insert above the viewport shifts the scroller by one row', () => {
    const heights = new HeightIndex(100, 44)
    const keys = ['a', 'b', 'c', 'd']
    const anchor = anchorOf(keys, 0, 2)!
    expect(anchor).toEqual({ key: 'c', index: 2 })
    const shift = anchorShift(anchor, ['new', 'a', 'b', 'c', 'd'], 0, heights)
    expect(shift).toBe(44)
  })

  test('no shift when the anchor did not move, or left the window', () => {
    const heights = new HeightIndex(100, 44)
    const anchor = { key: 'c', index: 2 }
    expect(anchorShift(anchor, ['a', 'b', 'c'], 0, heights)).toBeNull()
    expect(anchorShift(anchor, ['x', 'y'], 0, heights)).toBeNull()
  })
})

describe('scrollTopFor', () => {
  // A large title and padding put the list 101px down its scroller; a 50px sticky bar covers the scroller's top edge.
  const frame = { listOffset: 101, viewportHeight: 600, padTop: 50 }
  const heights = new HeightIndex(1000, 44)

  test('the first row at start is the top of the scroller: the large title stays, row 1 clears the bar', () => {
    expect(scrollTopFor(heights, 0, 'start', frame)).toBe(0)
    expect(frame.listOffset).toBeGreaterThanOrEqual(frame.padTop) // row 1 then sits at 101px, below the 50px bar
  })

  test('any other row at start stops just below the sticky bar', () => {
    expect(scrollTopFor(heights, 10, 'start', frame)).toBe(101 + 440 - 50)
    expect(scrollTopFor(heights, 10, 'start', { ...frame, padTop: 0 })).toBe(101 + 440)
  })

  test('end puts the last row at the bottom edge, and never scrolls below the top', () => {
    expect(scrollTopFor(heights, 999, 'end', frame)).toBe(101 + 1000 * 44 - 600)
    expect(scrollTopFor(new HeightIndex(2, 44), 1, 'end', frame)).toBe(0)
  })

  test('an index past the list is clamped to its rows', () => {
    expect(scrollTopFor(heights, 5000, 'end', frame)).toBe(scrollTopFor(heights, 999, 'end', frame))
    expect(scrollTopFor(heights, -3, 'start', frame)).toBe(0)
  })

  test('an empty list scrolls to the top', () => {
    expect(scrollTopFor(new HeightIndex(0, 44), 0, 'end', frame)).toBe(0)
  })
})
