import { expect, test } from 'bun:test'
import { relativeTime } from './time'

const now = Date.parse('2026-11-22T12:00:00Z')

test('a recent time is relative, an old one is a date, and a time that cannot be read is shown as given', () => {
  expect(relativeTime('2026-11-22T11:59:50Z', now)).toBe(
    new Intl.RelativeTimeFormat(undefined, { numeric: 'auto', style: 'short' }).format(0, 'second')
  )
  expect(relativeTime('2026-11-22T11:55:00Z', now)).toContain('5')
  expect(relativeTime('2026-11-01T12:00:00Z', now)).toBe(
    new Date('2026-11-01T12:00:00Z').toLocaleDateString()
  )
  expect(relativeTime('not a time', now)).toBe('not a time')
})
