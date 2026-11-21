import { describe, expect, test } from 'bun:test'
import {
  insertionIndex,
  moveAnnouncement,
  moveBefore,
  moveBy,
  withInserted
} from './reorder'

const keys = ['a', 'b', 'c', 'd']

describe('moveBefore', () => {
  test('puts a row in front of another', () => {
    expect(moveBefore(keys, 'd', 'b')).toEqual({
      key: 'd',
      beforeKey: 'b',
      keys: ['a', 'd', 'b', 'c'],
      from: 3,
      to: 1
    })
  })

  test('null means last', () => {
    expect(moveBefore(keys, 'a', null)?.keys).toEqual(['b', 'c', 'd', 'a'])
  })

  test('dropping where it was is not a move', () => {
    expect(moveBefore(keys, 'b', 'c')).toBeNull()
    expect(moveBefore(keys, 'd', null)).toBeNull()
    expect(moveBefore(keys, 'b', 'b')).toBeNull()
  })

  test('unknown keys are ignored', () => {
    expect(moveBefore(keys, 'z', 'a')).toBeNull()
    expect(moveBefore(keys, 'a', 'z')).toBeNull()
  })
})

describe('moveBy (Alt+Arrow)', () => {
  test('up swaps with the previous row', () => {
    expect(moveBy(keys, 'c', -1)).toMatchObject({ beforeKey: 'b', keys: ['a', 'c', 'b', 'd'], to: 1 })
  })

  test('down swaps with the next row', () => {
    expect(moveBy(keys, 'b', 1)).toMatchObject({ beforeKey: 'd', keys: ['a', 'c', 'b', 'd'], to: 2 })
  })

  test('down from the penultimate row goes last (beforeKey null)', () => {
    expect(moveBy(keys, 'c', 1)).toMatchObject({ beforeKey: null, keys: ['a', 'b', 'd', 'c'] })
  })

  test('nothing past either end', () => {
    expect(moveBy(keys, 'a', -1)).toBeNull()
    expect(moveBy(keys, 'd', 1)).toBeNull()
  })

  test('works with numeric keys', () => {
    expect(moveBy([10, 20, 30], 20, 1)?.keys).toEqual([10, 30, 20])
  })
})

describe('drag', () => {
  const rects = [
    { top: 0, bottom: 40 },
    { top: 40, bottom: 80 },
    { top: 80, bottom: 120 }
  ]

  test('insertion index is in front of the first row whose midpoint is below the pointer', () => {
    expect(insertionIndex(rects, 5)).toBe(0)
    expect(insertionIndex(rects, 25)).toBe(1)
    expect(insertionIndex(rects, 61)).toBe(2)
    expect(insertionIndex(rects, 500)).toBe(3)
  })

  test('withInserted shows the order during a drag', () => {
    expect(withInserted(keys, 'd', 1)).toEqual(['a', 'd', 'b', 'c'])
    expect(withInserted(keys, 'a', 99)).toEqual(['b', 'c', 'd', 'a'])
    expect(withInserted(keys, 'c', -3)).toEqual(['c', 'a', 'b', 'd'])
  })

  test('announcements are 1-based', () => {
    expect(moveAnnouncement('Note 3', 1, 10)).toBe('Note 3 moved to position 2 of 10')
  })
})
