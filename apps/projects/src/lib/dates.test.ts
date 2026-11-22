import { expect, test } from 'bun:test'
import { moment } from './dates'

test('a moment reads as a day and a time', () => {
  // local time: the same text wherever the test runs
  expect(moment(new Date(2026, 5, 9, 9, 41).toISOString())).toBe('9 June 2026 at 09:41')
})

test('text that is not a moment reads as nothing', () => {
  expect(moment('soon')).toBe('')
})
