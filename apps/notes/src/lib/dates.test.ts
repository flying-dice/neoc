import { describe, expect, test } from 'bun:test'
import { daysAgo, groupOf, moment, stamp } from './dates'

const NOW = new Date(2026, 5, 9, 9, 41)
const at = (year: number, month: number, day: number, hour = 12, minute = 0): string => new Date(year, month - 1, day, hour, minute).toISOString()

describe('how long ago a note was edited', () => {
  test('counts days, not hours: late yesterday is one day ago', () => {
    expect(daysAgo(at(2026, 6, 9, 0, 5), NOW)).toBe(0)
    expect(daysAgo(at(2026, 6, 8, 23, 59), NOW)).toBe(1)
    expect(daysAgo(at(2026, 6, 2), NOW)).toBe(7)
  })

  test('a moment ahead of the clock, or one that is not a moment, is today', () => {
    expect(daysAgo(at(2026, 6, 10), NOW)).toBe(0)
    expect(daysAgo('soon', NOW)).toBe(0)
  })
})

describe('the heading a note is listed under', () => {
  test.each([
    [at(2026, 6, 9, 8), 'Today'],
    [at(2026, 6, 8), 'Yesterday'],
    [at(2026, 6, 7), 'Previous 7 Days'],
    [at(2026, 6, 2), 'Previous 7 Days'],
    [at(2026, 6, 1), 'Previous 30 Days'],
    [at(2026, 5, 10), 'Previous 30 Days'],
    [at(2026, 3, 14), 'March'],
    [at(2025, 12, 25), '2025']
  ])('%s is under %s', (edited, heading) => {
    expect(groupOf(edited, NOW)).toBe(heading)
  })
})

describe('how an edit reads', () => {
  test('next to a note: the time today, Yesterday, the weekday this week, then the date', () => {
    expect(stamp(at(2026, 6, 9, 8, 5), NOW)).toBe('08:05')
    expect(stamp(at(2026, 6, 8), NOW)).toBe('Yesterday')
    expect(stamp(at(2026, 6, 5), NOW)).toBe('Friday')
    expect(stamp(at(2026, 6, 2), NOW)).toBe('02/06/2026')
    expect(stamp('soon', NOW)).toBe('')
  })

  test('over the note: the day and the time', () => {
    expect(moment(at(2026, 6, 9, 8, 5))).toBe('9 June 2026 at 08:05')
    expect(moment('soon')).toBe('')
  })
})
