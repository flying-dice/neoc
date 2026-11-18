import { describe, expect, test } from 'bun:test'
import { cellPlacement, gridTemplate } from './grid'

describe('gridTemplate', () => {
  test('a count is that many equal columns that may shrink', () => {
    expect(gridTemplate(3)).toBe('repeat(3, minmax(0, 1fr))')
    expect(gridTemplate(1)).toBe('repeat(1, minmax(0, 1fr))')
  })

  test('a count is clamped and must be at least one', () => {
    expect(gridTemplate(99)).toBe('repeat(24, minmax(0, 1fr))')
    expect(gridTemplate(0)).toBeUndefined()
    expect(gridTemplate(2.7)).toBe('repeat(2, minmax(0, 1fr))')
    expect(gridTemplate(Number.NaN)).toBeUndefined()
  })

  test('a track list maps fixed, flex and min/max tracks', () => {
    expect(gridTemplate([{ fixed: 80 }, { flex: 2 }, { min: 40, max: 90 }])).toBe(
      '80px minmax(0, 2fr) minmax(40px, 90px)'
    )
    expect(gridTemplate([{ min: 100 }])).toBe('minmax(100px, 1fr)')
    expect(gridTemplate([{}])).toBe('minmax(0, 1fr)')
  })

  test('nothing usable means the grid auto-flows', () => {
    expect(gridTemplate(undefined)).toBeUndefined()
    expect(gridTemplate([])).toBeUndefined()
    expect(gridTemplate('3')).toBeUndefined()
  })
})

describe('cellPlacement', () => {
  test('span, column start and row start become grid lines', () => {
    expect(cellPlacement({ span: 2 })).toEqual({ column: 'auto / span 2', row: undefined, span: 2 })
    expect(cellPlacement({ colStart: 3 })).toEqual({ column: '3 / span 1', row: undefined, span: 1 })
    expect(cellPlacement({ colStart: 2, span: 2, rowStart: 4 })).toEqual({ column: '2 / span 2', row: '4', span: 2 })
  })

  test('bad values are ignored, big ones clamped', () => {
    expect(cellPlacement({ span: 0, colStart: -1, rowStart: 'x' })).toEqual({ column: undefined, row: undefined, span: 1 })
    expect(cellPlacement({ span: 500 }).column).toBe('auto / span 24')
    expect(cellPlacement({})).toEqual({ column: undefined, row: undefined, span: 1 })
  })
})
