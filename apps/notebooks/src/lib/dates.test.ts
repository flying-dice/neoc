import { expect, test } from 'bun:test'
import { changedLabel, dayOf } from './dates'

const NOW = new Date(2026, 5, 9, 9, 41)

test("a day is written as the title of that day's note", () => {
  expect(dayOf(NOW)).toBe('2026-06-09')
  expect(dayOf(new Date(2026, 0, 3))).toBe('2026-01-03')
})

test('a change today reads as its time, yesterday as Yesterday, before that as its date', () => {
  expect(changedLabel(new Date(2026, 5, 9, 8, 5).toISOString(), NOW)).toBe('08:05')
  expect(changedLabel(new Date(2026, 5, 8, 23, 0).toISOString(), NOW)).toBe('Yesterday')
  expect(changedLabel(new Date(2026, 4, 30, 12, 0).toISOString(), NOW)).toBe('30 May 2026')
})

test('text that is not a moment reads as nothing', () => {
  expect(changedLabel('soon', NOW)).toBe('')
})
