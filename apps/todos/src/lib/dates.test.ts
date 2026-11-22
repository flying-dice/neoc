import { describe, expect, test } from 'bun:test'
import { dayOf, dueLabel, isOverdue, shift } from './dates'

const TODAY = '2026-06-09'

describe('a day', () => {
  test('is the date where the person is, not in UTC', () => {
    expect(dayOf(new Date(2026, 5, 9, 0, 5))).toBe('2026-06-09')
    expect(dayOf(new Date(2026, 5, 9, 23, 55))).toBe('2026-06-09')
    expect(dayOf(new Date(2026, 0, 3, 12))).toBe('2026-01-03')
  })

  test('shifts across the end of a month and of a year', () => {
    expect(shift('2026-06-30', 1)).toBe('2026-07-01')
    expect(shift('2026-01-01', -1)).toBe('2025-12-31')
    expect(shift(TODAY, 0)).toBe(TODAY)
  })
})

describe('how a due day reads', () => {
  test.each([
    ['', ''],
    ['2026-06-09', 'Today'],
    ['2026-06-10', 'Tomorrow'],
    ['2026-06-08', 'Yesterday'],
    ['2026-06-11', '11 Jun 2026'],
    ['2025-12-01', '1 Dec 2025']
  ])('%p reads %p', (due, label) => {
    expect(dueLabel(due, TODAY)).toBe(label)
  })

  test('text that is not a day is shown as it is', () => {
    expect(dueLabel('soon', TODAY)).toBe('soon')
  })
})

describe('overdue', () => {
  test('is a day before today, never today and never no day', () => {
    expect(isOverdue('2026-06-08', TODAY)).toBe(true)
    expect(isOverdue(TODAY, TODAY)).toBe(false)
    expect(isOverdue('2026-06-10', TODAY)).toBe(false)
    expect(isOverdue('', TODAY)).toBe(false)
  })
})
